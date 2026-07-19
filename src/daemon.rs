use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::config;
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

pub fn run() -> io::Result<()> {
    let _daemon_guard = DaemonGuard::acquire()?;
    logging::ensure_flag();
    reset_description();
    for error in sysctl::apply_base_sysctls() {
        logging::log_print(&format!("[WARN] startup sysctl apply failed: {error}"));
    }

    let mut last_mode = IfaceMode::Unknown;
    let mut last_iface = String::new();
    let mut last_change: Option<Instant> = None;
    let mut wifi_pending_since: Option<Instant> = None;
    let mut wifi_applied = false;
    let mut adaptive_count: u32 = 0;
    let mut last_qdisc_check: Option<Instant> = None;
    let mut route_unavailable = false;

    loop {
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
        let force_apply = config::module_dir().join("force_apply").exists();
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
    let prefix = mode.prefix();
    let available = match sysctl::available_algorithms() {
        Ok(a) => a,
        Err(e) => {
            logging::log_print(&format!("[ERROR] Cannot read algorithms: {e}"));
            return;
        }
    };

    let algo = select_algorithm(prefix, &available);

    let cfg = config::get_algo_config(algo);
    logging::log_print(&format!("Selected {algo}: {}", cfg.desc));

    let (base_ca, base_ss) = pacing_override().unwrap_or((cfg.pacing_ca, cfg.pacing_ss));
    let (ca, ss) = if mode == IfaceMode::WiFi {
        match network::wifi_freq(iface) {
            Some(freq) => {
                logging::log_print(&format!("Wi-Fi band detected: {freq} MHz"));
                if freq < 3000 {
                    (base_ca * 3 / 4, base_ss * 3 / 4)
                } else if freq < 6000 {
                    (base_ca, base_ss)
                } else {
                    (base_ca * 5 / 4, base_ss * 5 / 4)
                }
            }
            None => (base_ca, base_ss),
        }
    } else {
        (base_ca, base_ss)
    };

    if let Err(e) = sysctl::set_pacing(ca, ss) {
        logging::log_print(&format!("[WARN] Pacing failed: {e}"));
    }

    let qdisc = qdisc_override().unwrap_or(cfg.qdisc);
    if !qdisc.is_empty() {
        if let Err(error) = sysctl::set_default_qdisc(qdisc) {
            logging::log_print(&format!(
                "[WARN] Failed to set default qdisc {qdisc}: {error}"
            ));
        }
        match network::set_qdisc(iface, qdisc) {
            Ok(()) => logging::log_print(&format!("Applied qdisc: {qdisc} ({iface})")),
            Err(e) => logging::log_print(&format!("Failed to apply qdisc: {qdisc} ({iface}): {e}")),
        }
    }

    match sysctl::algo_available(algo) {
        Ok(true) => {
            if let Err(e) = sysctl::set_congestion_control(algo) {
                logging::log_print(&format!("[ERROR] Failed to set {algo}: {e}"));
                return;
            }
            logging::log_print(&format!(
                "Applied congestion control: {algo} ({})",
                mode.as_str()
            ));

            if config::module_dir().join("kill_connections").exists() {
                logging::log_print(&format!("Killing TCP connections on {iface}"));
                if let Err(error) = network::kill_connections(iface) {
                    logging::log_print(&format!("[WARN] Failed to kill TCP connections: {error}"));
                }
            }
            update_description(mode, algo);
        }
        Ok(false) => logging::log_print(&format!("Unavailable algorithm: {algo}")),
        Err(e) => logging::log_print(&format!("[ERROR] algo_available check: {e}")),
    }

    if config::module_dir().join("initcwnd_initrwnd").exists() {
        if let Err(e) = network::set_max_initcwnd_initrwnd(iface) {
            logging::log_print(&format!("[WARN] initcwnd/initrwnd failed: {e}"));
        }
    }
}

fn reconcile_interface_qdisc(iface: &str, mode: IfaceMode) -> io::Result<()> {
    let available = sysctl::available_algorithms()?;
    let algo = select_algorithm(mode.prefix(), &available);
    let qdisc = qdisc_override().unwrap_or(config::get_algo_config(algo).qdisc);
    if qdisc.is_empty() {
        return Ok(());
    }

    if network::reconcile_qdisc(iface, qdisc)? {
        logging::log_print(&format!(
            "[INFO] Restored qdisc after kernel reset: {qdisc} ({iface})"
        ));
    }
    sysctl::set_default_qdisc(qdisc)?;
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
    use super::{qdisc_check_interval, should_apply_wifi};
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
}
