use std::fs;
use std::io;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

pub fn run() -> io::Result<()> {
    logging::ensure_flag();
    reset_description();

    let mut last_mode = IfaceMode::Unknown;
    let mut change_time: u64 = 0;
    let mut adaptive_count: u32 = 0;
    let mut sleep_secs = SLEEP_NORMAL;

    loop {
        let now = epoch_secs();

        let iface = match network::active_iface() {
            Ok(i) => i,
            Err(e) => {
                logging::log_print(&format!("[WARN] iface detect failed: {e}"));
                thread::sleep(Duration::from_secs(SLEEP_NORMAL));
                continue;
            }
        };

        let new_mode = network::iface_mode(&iface);
        let force_apply = config::module_dir().join("force_apply").exists();
        let mut mode_changed = false;

        if new_mode != last_mode || force_apply {
            if now.saturating_sub(change_time) >= DEBOUNCE_TIME {
                mode_changed = true;
                match new_mode {
                    IfaceMode::Cellular => {
                        apply_interface_settings(&iface, &IfaceMode::Cellular);
                    }
                    IfaceMode::WiFi => { /* handled below */ }
                    IfaceMode::Unknown => {}
                }
                last_mode = new_mode;
                change_time = now;
                let _ = fs::remove_file(config::module_dir().join("force_apply"));
            }
        }

        // Unified Wi-Fi apply with VoWiFi detection
        if new_mode == IfaceMode::WiFi {
            let vowifi = proxy::wifi_calling_active().unwrap_or(true);
            if mode_changed || (now.saturating_sub(change_time) >= VOWIFI_CONNECT_TIME || !vowifi) {
                logging::log_print(&format!("[INFO] Applying Wi-Fi settings (VoWiFi={})", !vowifi));
                apply_interface_settings(&iface, &IfaceMode::WiFi);
            }
        }

        // Adaptive polling
        if mode_changed {
            adaptive_count = ADAPTIVE_FAST_CYCLES;
            sleep_secs = SLEEP_FAST;
        } else if adaptive_count > 0 {
            adaptive_count -= 1;
            sleep_secs = SLEEP_FAST;
        } else {
            sleep_secs = SLEEP_NORMAL;
        }

        thread::sleep(Duration::from_secs(sleep_secs));
    }
}

pub fn run_once() -> io::Result<()> {
    thread::sleep(Duration::from_secs(2));

    let algo = if sysctl::algo_available("bbr").unwrap_or(false) {
        "bbr"
    } else {
        "cubic"
    };

    if let Err(e) = sysctl::set_congestion_control(algo) {
        logging::log_print(&format!("[ERROR] Failed to set initial algo: {e}"));
    }
    sysctl::apply_base_sysctls();
    logging::log_print(&format!("[INFO] Once: congestion_control={algo}"));

    Ok(())
}

fn apply_interface_settings(iface: &str, mode: &IfaceMode) {
    let prefix = mode.prefix();
    let available = match sysctl::available_algorithms() {
        Ok(a) => a,
        Err(e) => {
            logging::log_print(&format!("[ERROR] Cannot read algorithms: {e}"));
            return;
        }
    };

    let algo = available
        .iter()
        .find(|a| config::module_dir().join(format!("{prefix}_{a}")).exists())
        .map(|s| s.as_str())
        .unwrap_or("cubic");

    let cfg = config::get_algo_config(algo);

    let (ca, ss) = if *mode == IfaceMode::WiFi {
        match network::wifi_freq(iface) {
            Some(freq) => {
                logging::log_print(&format!("Wi-Fi band detected: {freq} MHz"));
                if freq < 3000 {
                    (cfg.pacing_ca * 3 / 4, cfg.pacing_ss * 3 / 4)
                } else if freq < 6000 {
                    (cfg.pacing_ca, cfg.pacing_ss)
                } else {
                    (cfg.pacing_ca * 5 / 4, cfg.pacing_ss * 5 / 4)
                }
            }
            None => (cfg.pacing_ca, cfg.pacing_ss),
        }
    } else {
        (cfg.pacing_ca, cfg.pacing_ss)
    };

    if let Err(e) = sysctl::set_pacing(ca, ss) {
        logging::log_print(&format!("[WARN] Pacing failed: {e}"));
    }

    if !cfg.qdisc.is_empty() {
        match network::set_qdisc(iface, cfg.qdisc) {
            Ok(()) => logging::log_print(&format!("Applied qdisc: {} ({iface})", cfg.qdisc)),
            Err(e) => logging::log_print(&format!("Failed to apply qdisc: {} ({iface}): {e}", cfg.qdisc)),
        }
    }

    match sysctl::algo_available(algo) {
        Ok(true) => {
            if let Err(e) = sysctl::set_congestion_control(algo) {
                logging::log_print(&format!("[ERROR] Failed to set {algo}: {e}"));
                return;
            }
            logging::log_print(&format!("Applied congestion control: {algo} ({})", mode.as_str()));

            if config::module_dir().join("kill_connections").exists() {
                logging::log_print(&format!("Killing TCP connections on {iface}"));
                network::kill_connections(iface);
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

fn update_description(mode: &IfaceMode, algo: &str) {
    let desc = format!(
        "TCP Optimisations \\& update tcp_cong_algo based on interface \\| iface\\: {} {} \\| algo\\: {algo}",
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
        if let Err(e) = fs::write(&mod_prop, updated) {
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
        if let Err(e) = fs::write(&mod_prop, updated) {
            logging::log_print(&format!("[WARN] Failed to reset description: {e}"));
        }
    }
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
