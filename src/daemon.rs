use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::adaptive;
use crate::config;
use crate::control::{ApplyResponse, ControlServer, Request};
use crate::logging;
use crate::network::{self, IfaceMode};
use crate::profile;
use crate::proxy;
use crate::sysctl;

const NETWORK_SETTLE_MS: u64 = 180;
const ADAPTIVE_FAST_CYCLES: u32 = 3;
const SLEEP_FAST: u64 = 2;
const SLEEP_NORMAL: u64 = 30;
const QDISC_CHECK_WIFI: u64 = 60;
const QDISC_CHECK_CELLULAR: u64 = 120;
const ADAPTIVE_STATE_PERSIST: u64 = 30;
const WEBUI_FAST_SLEEP: u64 = 2;
const WEBUI_ACTIVE_WINDOW: u64 = 12;
const WEBUI_DETAILS_PERSIST: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedPolicy {
    pub algorithm: String,
    pub qdisc: String,
    pub pacing_ca: u32,
    pub pacing_ss: u32,
    pub wifi_frequency_mhz: Option<u32>,
}

pub fn run() -> io::Result<()> {
    let _daemon_guard = DaemonGuard::acquire()?;
    logging::ensure_flag();
    reset_description();

    if let Err(error) = profile::refresh_managed_profile() {
        logging::log_print(&format!(
            "[WARN] Auto profile refresh failed at startup: {error}"
        ));
    }
    for error in sysctl::apply_base_sysctls() {
        logging::log_print(&format!("[WARN] startup sysctl apply failed: {error}"));
    }

    // BBRv1 is built in on PJZ110. Register the audited BBRv3 KO once at
    // startup when necessary; the v4 hot-switch path never reloads it.
    if crate::kernel_module::bundled_algorithms()
        .iter()
        .any(|algorithm| algorithm == "bbr3")
        && !sysctl::algo_available("bbr3").unwrap_or(false)
    {
        match crate::kernel_module::ensure_algorithm("bbr3") {
            Ok(true) if sysctl::algo_available("bbr3").unwrap_or(false) => {
                let _ = crate::kernel_module::clear_algorithm_unavailable("bbr3");
                logging::log_print("[INFO] BBRv3 module registered and ready");
            }
            Ok(_) => logging::log_print(
                "[WARN] BBRv3 module was present but did not register as a TCP congestion control",
            ),
            Err(error) => {
                logging::log_print(&format!("[WARN] BBRv3 warm registration failed: {error}"))
            }
        }
    }

    // Run the expensive BTF/KO fingerprint audit once during daemon startup,
    // never while the user is waiting for an algorithm switch.
    if sysctl::algo_available("bbr3").unwrap_or(false) {
        match crate::kernel_module::verify_bbr3_runtime_abi() {
            Ok(()) => logging::log_print("[INFO] BBRv3 ColorOS 17 ABI gate verified"),
            Err(error) => logging::log_print(&format!(
                "[WARN] BBRv3 remains blocked by runtime ABI gate: {error}"
            )),
        }
    }

    let control_server = ControlServer::bind()?;
    logging::log_print("[INFO] v4 event-driven control plane ready");

    let mut route_monitor = match network::RouteMonitor::new() {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            logging::log_print(&format!(
                "[WARN] rtnetlink monitor unavailable; using timeout polling: {error}"
            ));
            None
        }
    };

    let mut last_mode = IfaceMode::Unknown;
    let mut last_iface = String::new();
    let mut network_dirty = true;
    let mut route_unavailable = false;
    let mut adaptive_count: u32 = 0;
    let mut last_qdisc_check: Option<Instant> = None;
    let mut adaptive_observer = adaptive::RuntimeObserver::default();
    let mut last_adaptive_persist: Option<Instant> = None;
    let mut last_adaptive_state = adaptive::PathState::Unknown;
    let mut last_snapshot_persist: Option<Instant> = None;
    let mut last_webui_details_persist: Option<Instant> = None;

    loop {
        let legacy_force_path = config::module_dir().join("force_apply");
        let legacy_force = legacy_force_path.exists();
        let mut mode_changed = false;

        if network_dirty || legacy_force || last_iface.is_empty() {
            if legacy_force {
                if let Err(error) = profile::refresh_managed_profile() {
                    logging::log_print(&format!(
                        "[WARN] legacy force profile refresh failed: {error}"
                    ));
                }
                for error in sysctl::apply_base_sysctls() {
                    logging::log_print(&format!(
                        "[WARN] legacy force sysctl apply failed: {error}"
                    ));
                }
                let _ = fs::remove_file(&legacy_force_path);
            }

            match network::active_iface() {
                Ok(iface) => {
                    let new_mode = network::iface_mode(&iface);
                    if iface != last_iface {
                        network::record_active_iface(&iface);
                    }
                    if route_unavailable {
                        logging::log_print(&format!("[INFO] Network route restored on {iface}"));
                        route_unavailable = false;
                    }

                    // v4 deliberately applies Wi-Fi/cellular policy immediately.
                    // The old 10-second debounce and VoWiFi wait did not change
                    // the selected policy and only added user-visible latency.
                    if new_mode != IfaceMode::Unknown {
                        apply_interface_settings(&iface, new_mode);
                        last_qdisc_check = Some(Instant::now());
                        adaptive_count = ADAPTIVE_FAST_CYCLES;
                        mode_changed = true;
                    } else {
                        last_qdisc_check = None;
                    }

                    last_mode = new_mode;
                    last_iface = iface;
                    network_dirty = false;
                }
                Err(error) => {
                    if !route_unavailable {
                        logging::log_print(&format!(
                            "[WARN] Network route lost ({error}); waiting for rtnetlink recovery"
                        ));
                        route_unavailable = true;
                        network::clear_cached_active_iface();
                    }
                    last_mode = IfaceMode::Unknown;
                    last_iface.clear();
                    last_qdisc_check = None;
                    adaptive_observer.clear();
                    adaptive::clear_runtime_state();
                    last_adaptive_persist = None;
                    last_adaptive_state = adaptive::PathState::Unknown;
                }
            }
        }

        if last_mode != IfaceMode::Unknown && !last_iface.is_empty() {
            if last_qdisc_check
                .map(|checked| checked.elapsed() >= qdisc_check_interval(last_mode))
                .unwrap_or(false)
            {
                if let Err(error) = reconcile_interface_qdisc(&last_iface, last_mode) {
                    logging::log_print(&format!("[WARN] qdisc reconciliation failed: {error}"));
                }
                last_qdisc_check = Some(Instant::now());
            }

            if let Some(state) = adaptive_observer.tick(&last_iface) {
                if state.stable_state != last_adaptive_state {
                    logging::log_print(&format!(
                        "[INFO] Adaptive observer: {:?} -> {:?} (confidence={})",
                        last_adaptive_state, state.stable_state, state.latest.confidence
                    ));
                    last_adaptive_state = state.stable_state;
                }

                let should_persist = last_adaptive_persist
                    .map(|saved| saved.elapsed() >= Duration::from_secs(ADAPTIVE_STATE_PERSIST))
                    .unwrap_or(true);
                if should_persist {
                    if let Err(error) = adaptive::persist_runtime_state(&state) {
                        logging::log_print(&format!(
                            "[WARN] adaptive observer state persist failed: {error}"
                        ));
                    } else {
                        last_adaptive_persist = Some(Instant::now());
                    }
                }
            }

            let snapshot_due = mode_changed
                || last_snapshot_persist
                    .map(|saved| saved.elapsed() >= Duration::from_secs(SLEEP_FAST))
                    .unwrap_or(true);
            if snapshot_due && persist_runtime_snapshot(&last_iface).is_ok() {
                last_snapshot_persist = Some(Instant::now());
            }

            let webui_active = webui_is_active();
            if webui_active
                && last_webui_details_persist
                    .map(|saved| saved.elapsed() >= Duration::from_secs(WEBUI_DETAILS_PERSIST))
                    .unwrap_or(true)
            {
                if let Err(error) = persist_runtime_details(&last_iface) {
                    logging::log_print(&format!("[WARN] WebUI detail snapshot failed: {error}"));
                } else {
                    last_webui_details_persist = Some(Instant::now());
                }
            }
        }

        let webui_active = webui_is_active();
        let mut sleep_secs = if mode_changed {
            SLEEP_FAST
        } else if adaptive_count > 0 {
            adaptive_count -= 1;
            SLEEP_FAST
        } else {
            SLEEP_NORMAL
        };
        if webui_active {
            sleep_secs = sleep_secs.min(WEBUI_FAST_SLEEP);
        }

        let wake = wait_for_wake(
            route_monitor.as_mut(),
            &control_server,
            Duration::from_secs(sleep_secs),
        )?;

        if wake.route_broken {
            logging::log_print(
                "[WARN] rtnetlink listener failed; reverting to timeout route checks",
            );
            route_monitor = None;
            network_dirty = true;
        }

        if wake.control && handle_control_requests(&control_server) {
            // The control request already applied the policy. Refresh the
            // cheap snapshot on the next pass without performing another
            // expensive full policy application.
            last_snapshot_persist = None;
            if let Ok(iface) = network::fast_active_iface() {
                let mode = network::iface_mode(&iface);
                if mode != IfaceMode::Unknown {
                    if iface != last_iface {
                        network::record_active_iface(&iface);
                    }
                    last_iface = iface;
                    last_mode = mode;
                    last_qdisc_check = Some(Instant::now());
                }
            }
        }

        if wake.route {
            // Netlink emits bursts for one transition. A short 180 ms settle
            // absorbs the burst while remaining dramatically faster than the
            // previous 10-second debounce.
            thread::sleep(Duration::from_millis(NETWORK_SETTLE_MS));
            network_dirty = true;
        } else if route_monitor.is_none() {
            // Safety-net polling when rtnetlink is unavailable.
            network_dirty = true;
        }
    }
}

#[derive(Debug, Default)]
struct WakeEvents {
    route: bool,
    control: bool,
    route_broken: bool,
}

fn wait_for_wake(
    mut route_monitor: Option<&mut network::RouteMonitor>,
    control_server: &ControlServer,
    timeout: Duration,
) -> io::Result<WakeEvents> {
    let route_fd = route_monitor
        .as_ref()
        .map(|monitor| monitor.raw_fd())
        .unwrap_or(-1);
    let mut poll_fds = [
        libc::pollfd {
            fd: route_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: control_server.raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    let rc = unsafe {
        libc::poll(
            poll_fds.as_mut_ptr(),
            poll_fds.len() as libc::nfds_t,
            timeout_ms,
        )
    };
    if rc < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(WakeEvents::default());
        }
        return Err(error);
    }

    let mut wake = WakeEvents::default();
    if rc == 0 {
        return Ok(wake);
    }

    let bad = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
    if route_fd >= 0 {
        if poll_fds[0].revents & bad != 0 {
            wake.route_broken = true;
        } else if poll_fds[0].revents & libc::POLLIN != 0 {
            if let Some(monitor) = route_monitor {
                monitor.drain_ready()?;
            }
            wake.route = true;
        }
    }

    if poll_fds[1].revents & bad != 0 {
        return Err(io::Error::other(format!(
            "runtime control poll failed with revents=0x{:x}",
            poll_fds[1].revents
        )));
    }
    wake.control = poll_fds[1].revents & libc::POLLIN != 0;
    Ok(wake)
}

fn handle_control_requests(server: &ControlServer) -> bool {
    let mut applied = false;
    loop {
        let (request, stream) = match server.accept() {
            Ok(Some(value)) => value,
            Ok(None) => break,
            Err(error) => {
                logging::log_print(&format!("[WARN] runtime control request failed: {error}"));
                break;
            }
        };

        match request {
            Request::Ping => {
                let response = ApplyResponse {
                    ok: true,
                    mode: "ping",
                    elapsed_ms: 0,
                    error: None,
                };
                let _ = server.reply_json(stream, &response);
            }
            Request::ApplyFast | Request::ApplyFull => {
                let full = request == Request::ApplyFull;
                let response = apply_control_request(full);
                applied |= response.ok;
                let _ = server.reply_json(stream, &response);
            }
        }
    }
    applied
}

fn apply_control_request(full: bool) -> ApplyResponse<'static> {
    let started = Instant::now();
    let mode_name = if full { "full" } else { "fast" };
    let mut errors = Vec::new();

    if full {
        if let Err(error) = profile::refresh_managed_profile() {
            errors.push(format!("profile refresh: {error}"));
        }
        errors.extend(
            sysctl::apply_base_sysctls()
                .into_iter()
                .map(|error| format!("base sysctl: {error}")),
        );
    }

    match network::fast_active_iface() {
        Ok(iface) => {
            let mode = network::iface_mode(&iface);
            if mode == IfaceMode::Unknown {
                errors.push(format!("unsupported active interface: {iface}"));
            } else {
                network::record_active_iface(&iface);
                match apply_interface_settings_inner(&iface, mode, full) {
                    Ok(failures) => errors.extend(failures),
                    Err(error) => errors.push(error.to_string()),
                }
            }
        }
        Err(error) => errors.push(format!("active interface: {error}")),
    }

    ApplyResponse {
        ok: errors.is_empty(),
        mode: mode_name,
        elapsed_ms: started.elapsed().as_millis(),
        error: (!errors.is_empty()).then(|| errors.join("; ")),
    }
}

fn persist_runtime_snapshot(iface: &str) -> io::Result<()> {
    persist_snapshot_file(iface, false, "runtime_snapshot.json")
}

fn persist_runtime_details(iface: &str) -> io::Result<()> {
    persist_snapshot_file(iface, true, "runtime_details.json")
}

fn persist_snapshot_file(iface: &str, include_details: bool, name: &str) -> io::Result<()> {
    let snapshot = crate::stats::network_snapshot(iface, true, include_details, false)?;
    let module_dir = config::module_dir();
    let path = module_dir.join(name);
    let temporary = module_dir.join(format!("{name}.tmp"));
    let payload = serde_json::to_vec(&snapshot).map_err(io::Error::other)?;
    fs::write(&temporary, payload)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn webui_is_active() -> bool {
    fs::metadata(config::module_dir().join("webui.active"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|elapsed| elapsed.as_secs() <= WEBUI_ACTIVE_WINDOW)
}

struct DaemonGuard {
    path: PathBuf,
}

impl DaemonGuard {
    fn acquire() -> io::Result<Self> {
        let path = config::module_dir().join("daemon.pid");
        match create_pid_file(&path) {
            Ok(()) => Ok(Self { path }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if daemon_pid_is_live(&path) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "TCP Optimiser daemon is already running",
                    ));
                }
                fs::remove_file(&path)?;
                create_pid_file(&path)?;
                Ok(Self { path })
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn create_pid_file(path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file, "{}", std::process::id())
}

fn daemon_pid_is_live(path: &Path) -> bool {
    let Ok(pid) = fs::read_to_string(path).and_then(|value| {
        value
            .trim()
            .parse::<u32>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }) else {
        return false;
    };
    let Ok(command_line) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    command_line
        .split(|byte| *byte == 0)
        .any(|arg| arg.ends_with(b"tcp_optimiser"))
}

pub fn is_running() -> bool {
    daemon_pid_is_live(&config::module_dir().join("daemon.pid"))
}

pub fn run_once() -> io::Result<()> {
    thread::sleep(Duration::from_secs(2));
    for error in sysctl::apply_base_sysctls() {
        logging::log_print(&format!("[WARN] sysctl apply failed: {error}"));
    }

    match network::active_iface() {
        Ok(iface) => match network::iface_mode(&iface) {
            IfaceMode::Unknown => {
                logging::log_print(&format!("[WARN] Once: unsupported interface {iface}"));
            }
            mode => apply_interface_settings(&iface, mode),
        },
        Err(error) => logging::log_print(&format!("[WARN] Once: iface detect failed: {error}")),
    }

    Ok(())
}

fn apply_interface_settings(iface: &str, mode: IfaceMode) {
    match apply_interface_settings_inner(iface, mode, true) {
        Ok(failures) => {
            for failure in failures {
                logging::log_print(&format!("[WARN] {failure}"));
            }
        }
        Err(error) => logging::log_print(&format!("[ERROR] Cannot resolve policy: {error}")),
    }
}

pub(crate) fn resolve_policy(iface: &str, mode: IfaceMode) -> io::Result<ResolvedPolicy> {
    let available = crate::kernel_module::augment_algorithms(sysctl::available_algorithms()?);
    let algorithm = select_algorithm(mode.prefix(), &available).to_string();
    let cfg = config::get_algo_config(&algorithm);
    let (base_ca, base_ss) = pacing_override().unwrap_or((cfg.pacing_ca, cfg.pacing_ss));
    let wifi_frequency_mhz = (mode == IfaceMode::WiFi)
        .then(|| network::wifi_freq(iface))
        .flatten();
    let (pacing_ca, pacing_ss) = adjusted_pacing(base_ca, base_ss, wifi_frequency_mhz);
    let requested_qdisc = qdisc_override().unwrap_or(cfg.qdisc);
    Ok(ResolvedPolicy {
        algorithm,
        qdisc: runtime_qdisc(requested_qdisc),
        pacing_ca,
        pacing_ss,
        wifi_frequency_mhz,
    })
}

pub(crate) fn repair_interface_settings(iface: &str, mode: IfaceMode) -> io::Result<Vec<String>> {
    apply_interface_settings_inner(iface, mode, false)
}

fn apply_interface_settings_inner(
    iface: &str,
    mode: IfaceMode,
    allow_connection_kill: bool,
) -> io::Result<Vec<String>> {
    let mut policy = resolve_policy(iface, mode)?;
    let requested_algorithm = policy.algorithm.clone();
    let mut failures = Vec::new();

    if !sysctl::algo_available(&policy.algorithm).unwrap_or(false) {
        match crate::kernel_module::ensure_algorithm(&policy.algorithm) {
            Ok(true) => {
                let _ = crate::kernel_module::clear_algorithm_unavailable(&policy.algorithm);
                logging::log_print(&format!(
                    "[INFO] Loaded kernel module for congestion control {}",
                    policy.algorithm
                ));
            }
            Ok(false) => {
                logging::log_print(&format!(
                    "[WARN] No bundled kernel module matched congestion control {}",
                    policy.algorithm
                ));
            }
            Err(error) => {
                logging::log_print(&format!(
                    "[WARN] Kernel module load for {} failed: {error}",
                    policy.algorithm
                ));
            }
        }
    } else {
        let _ = crate::kernel_module::clear_algorithm_unavailable(&policy.algorithm);
    }

    if !sysctl::algo_available(&policy.algorithm).unwrap_or(false) {
        let native = sysctl::available_algorithms().unwrap_or_default();
        if let Some(fallback) = runtime_fallback_algorithm(&native) {
            logging::log_print(&format!(
                "[WARN] Requested congestion control {requested_algorithm} is unavailable; falling back to {fallback}"
            ));
            policy.algorithm = fallback;
            let cfg = config::get_algo_config(&policy.algorithm);
            if qdisc_override().is_none() {
                policy.qdisc = cfg.qdisc.to_string();
            }
            if pacing_override().is_none() {
                (policy.pacing_ca, policy.pacing_ss) =
                    adjusted_pacing(cfg.pacing_ca, cfg.pacing_ss, policy.wifi_frequency_mhz);
            }
        }
    }

    let cfg = config::get_algo_config(&policy.algorithm);
    logging::log_print(&format!("Selected {}: {}", policy.algorithm, cfg.desc));
    if let Some(frequency) = policy.wifi_frequency_mhz {
        logging::log_print(&format!("Wi-Fi band detected: {frequency} MHz"));
    }

    if let Err(error) = sysctl::set_pacing(policy.pacing_ca, policy.pacing_ss) {
        failures.push(format!("Pacing apply failed: {error}"));
    }

    if !policy.qdisc.is_empty() {
        if let Err(error) = ensure_qdisc_for_policy(&policy.qdisc) {
            logging::log_print(&format!(
                "[WARN] Kernel module load for qdisc {} failed: {error}",
                policy.qdisc
            ));
        }
        if let Err(error) = sysctl::set_default_qdisc(&policy.qdisc) {
            failures.push(format!("Default qdisc {} failed: {error}", policy.qdisc));
        }
        match network::reconcile_qdisc(iface, &policy.qdisc) {
            Ok(changed) => {
                let _ = crate::kernel_module::clear_qdisc_unavailable(&policy.qdisc);
                if changed {
                    logging::log_print(&format!("Applied qdisc: {} ({iface})", policy.qdisc));
                }
            }
            Err(error) => {
                let requested_qdisc = policy.qdisc.clone();
                if crate::kernel_module::bundled_qdiscs()
                    .iter()
                    .any(|qdisc| qdisc == &requested_qdisc)
                {
                    let _ = crate::kernel_module::mark_qdisc_unavailable(&requested_qdisc);
                }
                failures.push(format!(
                    "Interface qdisc {} ({iface}) failed: {error}",
                    requested_qdisc
                ));

                if let Some(fallback) = runtime_fallback_qdisc(&requested_qdisc) {
                    if let Err(load_error) = ensure_qdisc_for_policy(&fallback) {
                        failures.push(format!(
                            "Fallback qdisc {fallback} load failed: {load_error}"
                        ));
                    } else if let Err(default_error) = sysctl::set_default_qdisc(&fallback) {
                        failures.push(format!(
                            "Fallback default qdisc {fallback} failed: {default_error}"
                        ));
                    } else {
                        match network::set_qdisc(iface, &fallback) {
                            Ok(()) => {
                                logging::log_print(&format!(
                                    "[WARN] qdisc {requested_qdisc} unavailable; using {fallback} ({iface})"
                                ));
                                policy.qdisc = fallback;
                            }
                            Err(fallback_error) => failures.push(format!(
                                "Fallback interface qdisc {fallback} ({iface}) failed: {fallback_error}"
                            )),
                        }
                    }
                }
            }
        }
    }

    let algorithm_applied = match sysctl::algo_available(&policy.algorithm) {
        Ok(true) => match sysctl::set_congestion_control(&policy.algorithm) {
            Ok(()) => {
                logging::log_print(&format!(
                    "Applied congestion control: {} ({})",
                    policy.algorithm,
                    mode.as_str()
                ));
                update_description(mode, &policy.algorithm);
                true
            }
            Err(error) => {
                failures.push(format!(
                    "Congestion control {} failed: {error}",
                    policy.algorithm
                ));
                false
            }
        },
        Ok(false) => {
            failures.push(format!(
                "Congestion control {} is unavailable",
                policy.algorithm
            ));
            false
        }
        Err(error) => {
            failures.push(format!("Algorithm capability check failed: {error}"));
            false
        }
    };

    if algorithm_applied
        && allow_connection_kill
        && config::module_dir().join("kill_connections").exists()
    {
        let proxy_state = proxy::detect_proxy_snapshot();
        let force_proxy_kill = config::module_dir().join("kill_connections_proxy").exists();
        if proxy_state.transparent && !force_proxy_kill {
            logging::log_print(&format!(
                "[INFO] Preserving existing TCP sessions because transparent proxy mode is active: {} / {}",
                proxy_state.family, proxy_state.mode
            ));
        } else {
            logging::log_print(&format!("Killing TCP connections on {iface}"));
            if let Err(error) = network::kill_connections(iface) {
                failures.push(format!("Failed to kill TCP connections: {error}"));
            }
        }
    }

    if config::module_dir().join("initcwnd_initrwnd").exists() {
        if let Err(error) = network::set_max_initcwnd_initrwnd(iface) {
            failures.push(format!("initcwnd/initrwnd apply failed: {error}"));
        }
    }

    Ok(failures)
}

fn runtime_fallback_algorithm(available: &[String]) -> Option<String> {
    available
        .iter()
        .find(|algorithm| algorithm.as_str() == "cubic")
        .or_else(|| {
            available
                .iter()
                .find(|algorithm| config::is_known_algorithm(algorithm))
        })
        .cloned()
}

fn runtime_qdisc(requested: &str) -> String {
    if !crate::kernel_module::qdisc_marked_unavailable(requested) {
        return requested.to_string();
    }
    runtime_fallback_qdisc(requested).unwrap_or_else(|| "fq_codel".to_string())
}

fn runtime_fallback_qdisc(requested: &str) -> Option<String> {
    ["fq_codel", "fq", "codel", "pfifo_fast"]
        .into_iter()
        .find(|candidate| {
            *candidate != requested && !crate::kernel_module::qdisc_marked_unavailable(candidate)
        })
        .map(str::to_string)
}

fn adjusted_pacing(base_ca: u32, base_ss: u32, wifi_frequency_mhz: Option<u32>) -> (u32, u32) {
    match wifi_frequency_mhz {
        Some(frequency) if frequency < 3000 => (base_ca * 3 / 4, base_ss * 3 / 4),
        Some(frequency) if frequency >= 6000 => (base_ca * 5 / 4, base_ss * 5 / 4),
        _ => (base_ca, base_ss),
    }
}

fn reconcile_interface_qdisc(iface: &str, mode: IfaceMode) -> io::Result<()> {
    let policy = resolve_policy(iface, mode)?;
    if policy.qdisc.is_empty() {
        return Ok(());
    }

    ensure_qdisc_for_policy(&policy.qdisc)?;
    if network::reconcile_qdisc(iface, &policy.qdisc)? {
        logging::log_print(&format!(
            "[INFO] Restored qdisc after kernel reset: {} ({iface})",
            policy.qdisc
        ));
    }
    sysctl::set_default_qdisc(&policy.qdisc)?;
    Ok(())
}

fn ensure_qdisc_for_policy(qdisc: &str) -> io::Result<()> {
    match crate::kernel_module::ensure_qdisc(qdisc) {
        Ok(true) => {
            let _ = crate::kernel_module::clear_qdisc_unavailable(qdisc);
            Ok(())
        }
        Ok(false) => Ok(()),
        Err(error) => {
            let _ = crate::kernel_module::mark_qdisc_unavailable(qdisc);
            Err(error)
        }
    }
}

fn qdisc_check_interval(mode: IfaceMode) -> Duration {
    Duration::from_secs(match mode {
        IfaceMode::WiFi => QDISC_CHECK_WIFI,
        IfaceMode::Cellular => QDISC_CHECK_CELLULAR,
        IfaceMode::Unknown => QDISC_CHECK_CELLULAR,
    })
}

fn update_description(mode: IfaceMode, algo: &str) {
    let desc = format!(
        "TCP Optimisations & dynamic congestion control | iface: {} {} | algo: {algo}",
        mode.as_str(),
        mode.icon()
    );

    let mod_prop = config::module_dir().join("module.prop");
    if let Ok(content) = fs::read_to_string(&mod_prop) {
        let updated = content
            .lines()
            .map(|line| {
                if line.starts_with("description=") {
                    format!("description={desc}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(e) = fs::write(&mod_prop, format!("{updated}\n")) {
            logging::log_print(&format!("[WARN] Failed to update description: {e}"));
        }
    }
}

fn reset_description() {
    let mod_prop = config::module_dir().join("module.prop");
    if let Ok(content) = fs::read_to_string(&mod_prop) {
        let updated = content
            .lines()
            .map(|line| {
                if line.starts_with("description=") {
                    format!("description={}", config::DEFAULT_DESC)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(e) = fs::write(&mod_prop, format!("{updated}\n")) {
            logging::log_print(&format!("[WARN] Failed to reset description: {e}"));
        }
    }
}

fn select_algorithm<'a>(prefix: &str, available: &'a [String]) -> &'a str {
    for known in config::ALL_ALGOS {
        if let Some(algo) = available.iter().find(|algo| algo.as_str() == *known) {
            if config::module_dir()
                .join(format!("{prefix}_{known}"))
                .exists()
            {
                return algo;
            }
        }
    }
    available
        .iter()
        .find(|algo| algo.as_str() == "cubic")
        .or_else(|| {
            available
                .iter()
                .find(|algo| config::is_known_algorithm(algo))
        })
        .map(String::as_str)
        .unwrap_or("cubic")
}

fn qdisc_override() -> Option<&'static str> {
    let value = fs::read_to_string(config::module_dir().join("qdisc")).ok()?;
    let value = value.trim();
    config::KNOWN_QDISCS
        .iter()
        .copied()
        .find(|known| *known == value)
}

fn pacing_override() -> Option<(u32, u32)> {
    let read = |name: &str| {
        fs::read_to_string(config::module_dir().join(name))
            .ok()?
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=1000).contains(value))
    };
    Some((read("pacing_ca")?, read("pacing_ss")?))
}

#[cfg(test)]
mod tests {
    use super::{adjusted_pacing, qdisc_check_interval};
    use crate::network::IfaceMode;
    use std::time::Duration;

    #[test]
    fn qdisc_watchdog_uses_interface_specific_intervals() {
        assert_eq!(
            qdisc_check_interval(IfaceMode::WiFi),
            Duration::from_secs(60)
        );
        assert_eq!(
            qdisc_check_interval(IfaceMode::Cellular),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn pacing_adjustment_preserves_wifi_band_policy() {
        assert_eq!(adjusted_pacing(200, 300, Some(2412)), (150, 225));
        assert_eq!(adjusted_pacing(200, 300, Some(5180)), (200, 300));
        assert_eq!(adjusted_pacing(200, 300, Some(6115)), (250, 375));
        assert_eq!(adjusted_pacing(200, 300, None), (200, 300));
    }
}
