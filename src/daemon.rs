use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::adaptive;
use crate::config;
use crate::logging;
use crate::network::{self, IfaceMode};
use crate::profile;
use crate::proxy;
use crate::sysctl;

const DEBOUNCE_TIME: u64 = 10;
const VOWIFI_CONNECT_TIME: u64 = 10;
const VOWIFI_PROBE_INTERVAL: u64 = 5;
const ADAPTIVE_FAST_CYCLES: u32 = 3;
const SLEEP_FAST: u64 = 2;
const SLEEP_NORMAL: u64 = 30;
const QDISC_CHECK_WIFI: u64 = 60;
const QDISC_CHECK_CELLULAR: u64 = 120;
const ADAPTIVE_STATE_PERSIST: u64 = 30;

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

    let mut last_mode = IfaceMode::Unknown;
    let mut last_iface = String::new();
    let mut last_change: Option<Instant> = None;
    let mut wifi_pending_since: Option<Instant> = None;
    let mut wifi_applied = false;
    let mut last_vowifi_probe: Option<Instant> = None;
    let mut last_vowifi_active = false;
    let mut adaptive_count: u32 = 0;
    let mut last_qdisc_check: Option<Instant> = None;
    let mut adaptive_observer = adaptive::RuntimeObserver::default();
    let mut last_adaptive_persist: Option<Instant> = None;
    let mut last_adaptive_state = adaptive::PathState::Unknown;
    let mut route_unavailable = false;
    let mut route_monitor = match network::RouteMonitor::new() {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            logging::log_print(&format!(
                "[WARN] rtnetlink monitor unavailable; using timeout polling: {error}"
            ));
            None
        }
    };

    loop {
        let iface = match network::active_iface() {
            Ok(i) => i,
            Err(e) => {
                if !route_unavailable {
                    logging::log_print(&format!(
                        "[WARN] Network route lost ({e}); policy will be reapplied when it returns"
                    ));
                    route_unavailable = true;
                    network::clear_cached_active_iface();
                }
                if last_mode != IfaceMode::Unknown || !last_iface.is_empty() {
                    last_mode = IfaceMode::Unknown;
                    last_iface.clear();
                    last_change = None;
                    wifi_pending_since = None;
                    wifi_applied = false;
                    last_vowifi_probe = None;
                    last_vowifi_active = false;
                    last_qdisc_check = None;
                    adaptive_observer.clear();
                    adaptive::clear_runtime_state();
                    last_adaptive_persist = None;
                    last_adaptive_state = adaptive::PathState::Unknown;
                }

                // Keep the low-frequency timeout as a safety net, but let a
                // route/link event wake us immediately when connectivity returns.
                let wait = Duration::from_secs(SLEEP_NORMAL);
                if let Some(monitor) = route_monitor.as_mut() {
                    if let Err(error) = monitor.wait(wait) {
                        logging::log_print(&format!(
                            "[WARN] rtnetlink monitor failed while offline; reverting to timeout polling: {error}"
                        ));
                        route_monitor = None;
                        thread::sleep(wait);
                    }
                } else {
                    thread::sleep(wait);
                }
                continue;
            }
        };

        if iface != last_iface {
            network::record_active_iface(&iface);
        }

        if route_unavailable {
            logging::log_print(&format!("[INFO] Network route restored on {iface}"));
            route_unavailable = false;
        }

        let new_mode = network::iface_mode(&iface);
        let force_apply = config::module_dir().join("force_apply").exists();
        let mut mode_changed = false;

        if new_mode != last_mode || iface != last_iface || force_apply {
            let debounce_elapsed = force_apply
                || last_change
                    .map(|changed| changed.elapsed() >= Duration::from_secs(DEBOUNCE_TIME))
                    .unwrap_or(true);
            if debounce_elapsed {
                if force_apply {
                    if let Err(error) = profile::refresh_managed_profile() {
                        logging::log_print(&format!(
                            "[WARN] Forced auto profile refresh failed: {error}"
                        ));
                    }
                    for error in sysctl::apply_base_sysctls() {
                        logging::log_print(&format!("[WARN] forced sysctl apply failed: {error}"));
                    }
                }
                mode_changed = true;
                match new_mode {
                    IfaceMode::Cellular => {
                        apply_interface_settings(&iface, IfaceMode::Cellular);
                        last_qdisc_check = Some(Instant::now());
                        last_vowifi_probe = None;
                        last_vowifi_active = false;
                    }
                    IfaceMode::WiFi => {
                        if force_apply {
                            apply_interface_settings(&iface, IfaceMode::WiFi);
                            wifi_applied = true;
                            wifi_pending_since = None;
                            last_qdisc_check = Some(Instant::now());
                        } else {
                            wifi_applied = false;
                            wifi_pending_since = Some(Instant::now());
                            last_qdisc_check = None;
                        }
                        last_vowifi_probe = None;
                        last_vowifi_active = false;
                    }
                    IfaceMode::Unknown => {
                        last_qdisc_check = None;
                        last_vowifi_probe = None;
                        last_vowifi_active = false;
                    }
                }
                last_mode = new_mode;
                last_iface.clone_from(&iface);
                last_change = Some(Instant::now());
                let _ = fs::remove_file(config::module_dir().join("force_apply"));
            }
        }

        // Unified Wi-Fi apply with VoWiFi detection. dumpsys is relatively
        // expensive on Android, so probe it at a bounded cadence while keeping
        // the original 10-second wait semantics.
        let current_wifi_transition =
            new_mode == IfaceMode::WiFi && last_mode == IfaceMode::WiFi && iface == last_iface;
        if current_wifi_transition && !wifi_applied {
            let pending_since = wifi_pending_since.get_or_insert_with(Instant::now);
            if should_probe_vowifi(last_vowifi_probe.map(|probe| probe.elapsed())) {
                last_vowifi_active = proxy::wifi_calling_active().unwrap_or(false);
                last_vowifi_probe = Some(Instant::now());
            }

            if should_apply_wifi(wifi_applied, last_vowifi_active, pending_since.elapsed()) {
                logging::log_print(&format!(
                    "[INFO] Applying Wi-Fi settings (VoWiFi={last_vowifi_active})"
                ));
                apply_interface_settings(&iface, IfaceMode::WiFi);
                wifi_applied = true;
                last_qdisc_check = Some(Instant::now());
            }
        } else if new_mode != IfaceMode::WiFi {
            wifi_applied = false;
            wifi_pending_since = None;
            last_vowifi_probe = None;
            last_vowifi_active = false;
        }

        let policy_active = match new_mode {
            IfaceMode::WiFi => wifi_applied,
            IfaceMode::Cellular => true,
            IfaceMode::Unknown => false,
        };
        if policy_active
            && last_qdisc_check
                .map(|checked| checked.elapsed() >= qdisc_check_interval(new_mode))
                .unwrap_or(false)
        {
            if let Err(error) = reconcile_interface_qdisc(&iface, new_mode) {
                logging::log_print(&format!("[WARN] qdisc reconciliation failed: {error}"));
            }
            last_qdisc_check = Some(Instant::now());
        }

        // Observe path health without changing network policy. The observer
        // reuses daemon-loop intervals, so it never sleeps inside this loop.
        if new_mode != IfaceMode::Unknown {
            if let Some(state) = adaptive_observer.tick(&iface) {
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
        }

        // Adaptive polling
        let wifi_waiting = current_wifi_transition && !wifi_applied;
        let sleep_secs = if wifi_waiting {
            SLEEP_FAST
        } else if mode_changed {
            adaptive_count = ADAPTIVE_FAST_CYCLES;
            SLEEP_FAST
        } else if adaptive_count > 0 {
            adaptive_count -= 1;
            SLEEP_FAST
        } else {
            SLEEP_NORMAL
        };

        let wait = Duration::from_secs(sleep_secs);
        if let Some(monitor) = route_monitor.as_mut() {
            if let Err(error) = monitor.wait(wait) {
                logging::log_print(&format!(
                    "[WARN] rtnetlink monitor failed; reverting to timeout polling: {error}"
                ));
                route_monitor = None;
                thread::sleep(wait);
            }
        } else {
            thread::sleep(wait);
        }
    }
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
                let _ = crate::kernel_module::mark_algorithm_unavailable(&policy.algorithm);
            }
            Err(error) => {
                let _ = crate::kernel_module::mark_algorithm_unavailable(&policy.algorithm);
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
        match network::set_qdisc(iface, &policy.qdisc) {
            Ok(()) => {
                let _ = crate::kernel_module::clear_qdisc_unavailable(&policy.qdisc);
                logging::log_print(&format!("Applied qdisc: {} ({iface})", policy.qdisc));
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

fn should_probe_vowifi(last_probe_elapsed: Option<Duration>) -> bool {
    last_probe_elapsed
        .map(|elapsed| elapsed >= Duration::from_secs(VOWIFI_PROBE_INTERVAL))
        .unwrap_or(true)
}

fn should_apply_wifi(already_applied: bool, vowifi_active: bool, elapsed: Duration) -> bool {
    !already_applied && (vowifi_active || elapsed >= Duration::from_secs(VOWIFI_CONNECT_TIME))
}

#[cfg(test)]
mod tests {
    use super::{adjusted_pacing, qdisc_check_interval, should_apply_wifi, should_probe_vowifi};
    use crate::network::IfaceMode;
    use std::time::Duration;

    #[test]
    fn wifi_applies_on_registration_or_timeout_only_once() {
        assert!(should_apply_wifi(false, true, Duration::ZERO));
        assert!(!should_apply_wifi(false, false, Duration::from_secs(9)));
        assert!(should_apply_wifi(false, false, Duration::from_secs(10)));
        assert!(!should_apply_wifi(true, true, Duration::from_secs(20)));
    }

    #[test]
    fn vowifi_probe_is_rate_limited() {
        assert!(should_probe_vowifi(None));
        assert!(!should_probe_vowifi(Some(Duration::from_secs(4))));
        assert!(should_probe_vowifi(Some(Duration::from_secs(5))));
    }

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
