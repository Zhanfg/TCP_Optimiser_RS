use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::checkpoint;
use crate::config;
use crate::control::{self, ControlState, RuntimeMode};
use crate::logging;
use crate::network::{self, IfaceMode};
use crate::proxy;
use crate::sysctl;

const DEBOUNCE_TIME: u64 = 10;
const VOWIFI_CONNECT_TIME: u64 = 10;
const ADAPTIVE_FAST_CYCLES: u32 = 3;
const SLEEP_FAST: u64 = 2;
const SLEEP_NORMAL: u64 = 5;
const QDISC_CHECK_WIFI: u64 = 30;
const QDISC_CHECK_CELLULAR: u64 = 60;

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

    let mut control_state = read_control_state(None);
    if let Err(error) = control::acknowledge(&control_state) {
        let reason = format!("cannot persist runtime acknowledgement: {error}");
        logging::log_print(&format!("[ERROR] {reason}; daemon starts fail-closed"));
        control_state = ControlState::safe_fallback(reason);
    }
    let mut last_control_generation = control_state.generation;
    let mut last_control_error: Option<String> = None;
    let restore_locked = control::restore_in_progress();
    if control_state.mode.allows_writes() && !restore_locked {
        for error in sysctl::apply_base_sysctls() {
            logging::log_print(&format!("[WARN] startup sysctl apply failed: {error}"));
        }
    } else if restore_locked {
        logging::log_print(
            "[INFO] Runtime restoration is active; startup kernel writes are disabled",
        );
    } else {
        logging::log_print(&format!(
            "[INFO] Runtime starts in {} mode; kernel writes are disabled",
            control_state.mode.as_str()
        ));
    }

    let mut last_mode = IfaceMode::Unknown;
    let mut last_iface = String::new();
    let mut last_change: Option<Instant> = None;
    let mut wifi_pending_since: Option<Instant> = None;
    let mut wifi_applied = false;
    let mut adaptive_count: u32 = 0;
    let mut last_qdisc_check: Option<Instant> = None;
    let mut route_unavailable = false;
    let mut restore_barrier_logged = restore_locked;

    loop {
        let observed_control = match control::read() {
            Ok(state) => {
                if last_control_error.take().is_some() {
                    logging::log_print("[INFO] Runtime control state is readable again");
                }
                state
            }
            Err(error) => {
                let message = error.to_string();
                if last_control_error.as_deref() != Some(message.as_str()) {
                    logging::log_print(&format!(
                        "[ERROR] Runtime control state is invalid; entering read-only safe mode: {message}"
                    ));
                    last_control_error = Some(message.clone());
                }
                ControlState::safe_fallback(message)
            }
        };
        let control_changed = observed_control.generation != last_control_generation
            || observed_control.mode != control_state.mode;
        if control_changed {
            logging::log_print(&format!(
                "[INFO] Runtime control: generation={} mode={} action={:?} reason={}",
                observed_control.generation,
                observed_control.mode.as_str(),
                observed_control.requested_action,
                observed_control.reason
            ));
            if let Err(error) = control::acknowledge(&observed_control) {
                logging::log_print(&format!(
                    "[ERROR] Runtime acknowledgement failed; writes remain disabled: {error}"
                ));
                control_state =
                    ControlState::safe_fallback(format!("runtime-acknowledgement-failed: {error}"));
                thread::sleep(Duration::from_secs(SLEEP_FAST));
                continue;
            }
        }
        let control_requests_apply = control_changed
            && observed_control.mode == RuntimeMode::Active
            && observed_control.requested_action.requests_apply();
        last_control_generation = observed_control.generation;
        control_state = observed_control;

        let restore_locked = control::restore_in_progress();
        if restore_locked && !restore_barrier_logged {
            logging::log_print(
                "[INFO] Runtime restoration lock detected; daemon kernel writes are suspended",
            );
            restore_barrier_logged = true;
        } else if !restore_locked && restore_barrier_logged {
            logging::log_print("[INFO] Runtime restoration lock released");
            restore_barrier_logged = false;
        }

        if restore_locked || !control_state.mode.allows_writes() {
            last_mode = IfaceMode::Unknown;
            last_iface.clear();
            last_change = None;
            wifi_pending_since = None;
            wifi_applied = false;
            last_qdisc_check = None;
            thread::sleep(Duration::from_secs(if control_changed || restore_locked {
                SLEEP_FAST
            } else {
                SLEEP_NORMAL
            }));
            continue;
        }

        let iface = match network::active_iface() {
            Ok(i) => i,
            Err(e) => {
                if !route_unavailable {
                    logging::log_print(&format!(
                        "[WARN] Network route lost ({e}); policy will be reapplied when it returns"
                    ));
                    route_unavailable = true;
                }
                if last_mode != IfaceMode::Unknown || !last_iface.is_empty() {
                    last_mode = IfaceMode::Unknown;
                    last_iface.clear();
                    last_change = None;
                    wifi_pending_since = None;
                    wifi_applied = false;
                    last_qdisc_check = None;
                }
                thread::sleep(Duration::from_secs(SLEEP_NORMAL));
                continue;
            }
        };

        if route_unavailable {
            logging::log_print(&format!("[INFO] Network route restored on {iface}"));
            route_unavailable = false;
        }

        let new_mode = network::iface_mode(&iface);
        let force_apply =
            control_requests_apply || config::module_dir().join("force_apply").exists();
        let mut mode_changed = false;

        if new_mode != last_mode || iface != last_iface || force_apply {
            let debounce_elapsed = force_apply
                || last_change
                    .map(|changed| changed.elapsed() >= Duration::from_secs(DEBOUNCE_TIME))
                    .unwrap_or(true);
            if debounce_elapsed {
                if force_apply {
                    for error in sysctl::apply_base_sysctls() {
                        logging::log_print(&format!("[WARN] forced sysctl apply failed: {error}"));
                    }
                }
                mode_changed = true;
                match new_mode {
                    IfaceMode::Cellular => {
                        apply_interface_settings(&iface, IfaceMode::Cellular);
                        last_qdisc_check = Some(Instant::now());
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
                    }
                    IfaceMode::Unknown => last_qdisc_check = None,
                }
                last_mode = new_mode;
                last_iface.clone_from(&iface);
                last_change = Some(Instant::now());
                let _ = fs::remove_file(config::module_dir().join("force_apply"));
            }
        }

        // Unified Wi-Fi apply with VoWiFi detection
        // Only complete the VoWiFi wait for the interface transition that was
        // accepted above. Otherwise a rapid wlan0 -> wlan1 handover can reuse
        // wlan0's timer and apply settings to wlan1 before the debounce ends.
        let current_wifi_transition =
            new_mode == IfaceMode::WiFi && last_mode == IfaceMode::WiFi && iface == last_iface;
        if current_wifi_transition && !wifi_applied {
            let pending_since = wifi_pending_since.get_or_insert_with(Instant::now);
            let vowifi_active = proxy::wifi_calling_active().unwrap_or(false);
            if should_apply_wifi(wifi_applied, vowifi_active, pending_since.elapsed()) {
                logging::log_print(&format!(
                    "[INFO] Applying Wi-Fi settings (VoWiFi={vowifi_active})"
                ));
                apply_interface_settings(&iface, IfaceMode::WiFi);
                wifi_applied = true;
                last_qdisc_check = Some(Instant::now());
            }
        } else if new_mode != IfaceMode::WiFi {
            wifi_applied = false;
            wifi_pending_since = None;
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

        // Adaptive polling
        let sleep_secs = if mode_changed {
            adaptive_count = ADAPTIVE_FAST_CYCLES;
            SLEEP_FAST
        } else if adaptive_count > 0 {
            adaptive_count -= 1;
            SLEEP_FAST
        } else {
            SLEEP_NORMAL
        };

        thread::sleep(Duration::from_secs(sleep_secs));
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

fn read_daemon_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()
}

fn pid_is_live(pid: u32) -> bool {
    let Ok(command_line) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    command_line
        .split(|byte| *byte == 0)
        .any(|arg| arg.ends_with(b"tcp_optimiser"))
}

fn daemon_pid_is_live(path: &Path) -> bool {
    read_daemon_pid(path).is_some_and(pid_is_live)
}

pub fn running_pid() -> Option<u32> {
    let pid = read_daemon_pid(&config::module_dir().join("daemon.pid"))?;
    pid_is_live(pid).then_some(pid)
}

pub fn is_running() -> bool {
    running_pid().is_some()
}

pub fn run_once() -> io::Result<()> {
    thread::sleep(Duration::from_secs(2));
    if control::restore_in_progress() {
        logging::log_print("[INFO] Once skipped while a runtime restoration is active");
        return Ok(());
    }
    let control_state = read_control_state(None);
    if !control_state.mode.allows_writes() {
        logging::log_print(&format!(
            "[INFO] Once skipped in {} mode",
            control_state.mode.as_str()
        ));
        return Ok(());
    }
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
            for failure in &failures {
                logging::log_print(&format!("[WARN] {failure}"));
            }
            let verification = crate::policy::verify_policy(iface);
            if verification.summary.drifted == 0 && verification.errors.is_empty() {
                match resolve_policy(iface, mode)
                    .and_then(|policy| checkpoint::persist(iface, mode, &policy).map(|_| ()))
                {
                    Ok(()) => logging::log_print(&format!(
                        "[INFO] Last-known-good policy checkpoint updated for {iface}"
                    )),
                    Err(error) => logging::log_print(&format!(
                        "[WARN] Failed to update policy checkpoint: {error}"
                    )),
                }
            } else {
                let reason = format!(
                    "policy verification failed on {iface}: {} drifted, {} error(s)",
                    verification.summary.drifted,
                    verification.errors.len()
                );
                record_policy_failure(&reason);
            }
        }
        Err(error) => {
            logging::log_print(&format!("[ERROR] Cannot resolve policy: {error}"));
            record_policy_failure(&error.to_string());
        }
    }
}

fn record_policy_failure(reason: &str) {
    match checkpoint::record_failure(reason) {
        Ok(state) if state.consecutive_failures >= checkpoint::AUTOMATIC_SAFE_MODE_THRESHOLD => {
            let safe_reason = format!(
                "{} consecutive policy verification failures: {}",
                state.consecutive_failures, state.last_error
            );
            match control::enter_automatic_safe_mode(&safe_reason) {
                Ok(_) => logging::log_print(&format!(
                    "[ERROR] {safe_reason}; persistent safe mode enabled"
                )),
                Err(error) => logging::log_print(&format!(
                    "[ERROR] {safe_reason}; failed to persist safe mode: {error}"
                )),
            }
        }
        Ok(state) => logging::log_print(&format!(
            "[WARN] Policy failure count: {}/{}",
            state.consecutive_failures,
            checkpoint::AUTOMATIC_SAFE_MODE_THRESHOLD
        )),
        Err(error) => logging::log_print(&format!(
            "[ERROR] Policy failure state is unusable; fail-closed request result: {error}"
        )),
    }
}

fn read_control_state(previous_error: Option<&str>) -> ControlState {
    match control::read() {
        Ok(state) => state,
        Err(error) => {
            let message = error.to_string();
            if previous_error != Some(message.as_str()) {
                logging::log_print(&format!(
                    "[ERROR] Runtime control state is invalid; using safe mode: {message}"
                ));
            }
            ControlState::safe_fallback(message)
        }
    }
}

pub(crate) fn resolve_policy(iface: &str, mode: IfaceMode) -> io::Result<ResolvedPolicy> {
    let available = sysctl::available_algorithms()?;
    let algorithm = select_algorithm(mode.prefix(), &available).to_string();
    let cfg = config::get_algo_config(&algorithm);
    let (base_ca, base_ss) = pacing_override().unwrap_or((cfg.pacing_ca, cfg.pacing_ss));
    let wifi_frequency_mhz = (mode == IfaceMode::WiFi)
        .then(|| network::wifi_freq(iface))
        .flatten();
    let (pacing_ca, pacing_ss) = adjusted_pacing(base_ca, base_ss, wifi_frequency_mhz);
    Ok(ResolvedPolicy {
        algorithm,
        qdisc: qdisc_override().unwrap_or(cfg.qdisc).to_string(),
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
    let policy = resolve_policy(iface, mode)?;
    let cfg = config::get_algo_config(&policy.algorithm);
    let mut failures = Vec::new();
    logging::log_print(&format!("Selected {}: {}", policy.algorithm, cfg.desc));
    if let Some(frequency) = policy.wifi_frequency_mhz {
        logging::log_print(&format!("Wi-Fi band detected: {frequency} MHz"));
    }

    if let Err(error) = sysctl::set_pacing(policy.pacing_ca, policy.pacing_ss) {
        failures.push(format!("Pacing apply failed: {error}"));
    }

    if !policy.qdisc.is_empty() {
        if let Err(error) = sysctl::set_default_qdisc(&policy.qdisc) {
            failures.push(format!("Default qdisc {} failed: {error}", policy.qdisc));
        }
        match network::set_qdisc(iface, &policy.qdisc) {
            Ok(()) => logging::log_print(&format!("Applied qdisc: {} ({iface})", policy.qdisc)),
            Err(error) => failures.push(format!(
                "Interface qdisc {} ({iface}) failed: {error}",
                policy.qdisc
            )),
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
        logging::log_print(&format!("Killing TCP connections on {iface}"));
        if let Err(error) = network::kill_connections(iface) {
            failures.push(format!("Failed to kill TCP connections: {error}"));
        }
    }

    if config::module_dir().join("initcwnd_initrwnd").exists() {
        if let Err(error) = network::set_max_initcwnd_initrwnd(iface) {
            failures.push(format!("initcwnd/initrwnd apply failed: {error}"));
        }
    }

    Ok(failures)
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

    if network::reconcile_qdisc(iface, &policy.qdisc)? {
        logging::log_print(&format!(
            "[INFO] Restored qdisc after kernel reset: {} ({iface})",
            policy.qdisc
        ));
    }
    sysctl::set_default_qdisc(&policy.qdisc)?;
    Ok(())
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

fn should_apply_wifi(already_applied: bool, vowifi_active: bool, elapsed: Duration) -> bool {
    !already_applied && (vowifi_active || elapsed >= Duration::from_secs(VOWIFI_CONNECT_TIME))
}

#[cfg(test)]
mod tests {
    use super::{adjusted_pacing, qdisc_check_interval, should_apply_wifi};
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
    fn qdisc_watchdog_uses_interface_specific_intervals() {
        assert_eq!(
            qdisc_check_interval(IfaceMode::WiFi),
            Duration::from_secs(30)
        );
        assert_eq!(
            qdisc_check_interval(IfaceMode::Cellular),
            Duration::from_secs(60)
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
