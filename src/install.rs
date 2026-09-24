use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config;
use crate::logging;
use crate::sysctl;

/// Run module installation / upgrade logic (replaces customize.sh).
/// `MODPATH` or `TCP_OPTIMISER_MODULE_DIR` identifies the staging directory.
pub fn run() -> io::Result<()> {
    let staging_dir = config::module_dir();
    crate::integrity::verify_module(&staging_dir)?;
    fs::create_dir_all(&staging_dir)?;
    logging::log_print("Starting module customization (Rust)...");
    let live_dir = config::live_module_dir();

    let native = sysctl::available_algorithms().unwrap_or_else(|error| {
        logging::log_print(&format!(
            "[WARN] Cannot read congestion algorithms: {error}"
        ));
        vec!["cubic".to_string()]
    });
    for algorithm in crate::kernel_module::bundled_algorithms() {
        let _ = validate_algorithm_loadability(&algorithm);
    }
    preflight_bundled_qdiscs();

    let available = crate::kernel_module::augment_algorithms(
        sysctl::available_algorithms().unwrap_or_else(|_| native.clone()),
    );

    let safe_fallback = safe_fallback_algorithm(&native);
    let default_algo =
        if available.iter().any(|algo| algo == "bbr") && validate_algorithm_loadability("bbr") {
            "bbr"
        } else {
            safe_fallback
        };
    ensure_prefixed_config(&staging_dir, &live_dir, "wlan", default_algo, &available)?;
    ensure_prefixed_config(
        &staging_dir,
        &live_dir,
        "rmnet_data",
        default_algo,
        &available,
    )?;
    validate_prefixed_config(&staging_dir, "wlan", safe_fallback)?;
    validate_prefixed_config(&staging_dir, "rmnet_data", safe_fallback)?;

    // Rebuild the cache after real module-load preflight so WebUI fallback
    // data never advertises a bundled algorithm that this device rejected.
    let available = crate::kernel_module::augment_algorithms(
        sysctl::available_algorithms().unwrap_or_else(|_| native.clone()),
    );
    fs::write(staging_dir.join("available_algos"), available.join(" "))?;

    for name in [
        "kill_connections",
        "initcwnd_initrwnd",
        "qdisc",
        "pacing_ca",
        "pacing_ss",
        "tcp_ecn",
        "tcp_fastopen",
        "advanced.conf",
        "debug_mode",
        "disable_auto_tuning",
        "kill_connections_proxy",
    ] {
        preserve_exact_config(&staging_dir, &live_dir, name)?;
    }
    match crate::profile::refresh_managed_profile() {
        Ok((profile, _)) => logging::log_print(&format!(
            "Auto profile: iface={} mtu={} proxy={}/{} buffers>={}MiB",
            profile.active_iface.as_deref().unwrap_or("unavailable"),
            profile
                .iface_mtu
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            profile.proxy.family,
            profile.proxy.mode,
            profile.recommendations.socket_buffer_floor / 1_048_576,
        )),
        Err(error) => logging::log_print(&format!("[WARN] Auto profile failed: {error}")),
    }

    logging::log_print(&format!(
        "Module customization complete (default={default_algo}, algorithms={}).",
        available.join(" ")
    ));
    Ok(())
}

fn safe_fallback_algorithm(available: &[String]) -> &str {
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

fn validate_algorithm_loadability(algorithm: &str) -> bool {
    if sysctl::algo_available(algorithm).unwrap_or(false) {
        let _ = crate::kernel_module::clear_algorithm_unavailable(algorithm);
        return true;
    }

    match crate::kernel_module::ensure_algorithm(algorithm) {
        Ok(_) if sysctl::algo_available(algorithm).unwrap_or(false) => {
            let _ = crate::kernel_module::clear_algorithm_unavailable(algorithm);
            true
        }
        Ok(_) => {
            logging::log_print(&format!(
                "[WARN] {algorithm} is bundled but did not become available; excluding it"
            ));
            let _ = crate::kernel_module::mark_algorithm_unavailable(algorithm);
            false
        }
        Err(error) => {
            logging::log_print(&format!(
                "[WARN] {algorithm} preflight load failed: {error}; using a safe fallback"
            ));
            let _ = crate::kernel_module::mark_algorithm_unavailable(algorithm);
            false
        }
    }
}

fn preflight_bundled_qdiscs() {
    for qdisc in crate::kernel_module::bundled_qdiscs() {
        match crate::kernel_module::ensure_qdisc(&qdisc) {
            Ok(true) => {
                let _ = crate::kernel_module::clear_qdisc_unavailable(&qdisc);
                logging::log_print(&format!("[INFO] Qdisc preflight passed: {qdisc}"));
            }
            Ok(false) => {
                let _ = crate::kernel_module::mark_qdisc_unavailable(&qdisc);
                logging::log_print(&format!(
                    "[WARN] Qdisc {qdisc} is bundled but did not become loadable"
                ));
            }
            Err(error) => {
                let _ = crate::kernel_module::mark_qdisc_unavailable(&qdisc);
                logging::log_print(&format!(
                    "[WARN] Qdisc {qdisc} preflight load failed: {error}"
                ));
            }
        }
    }
}

fn validate_prefixed_config(dir: &Path, prefix: &str, fallback: &str) -> io::Result<()> {
    let Some(selected) = prefixed_files(dir, prefix).into_iter().next() else {
        return Ok(());
    };
    let Some(algorithm) = selected
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(&format!("{prefix}_")))
    else {
        return Ok(());
    };
    if validate_algorithm_loadability(algorithm) {
        return Ok(());
    }

    for stale in prefixed_files(dir, prefix) {
        fs::remove_file(stale)?;
    }
    fs::write(dir.join(format!("{prefix}_{fallback}")), "")?;
    logging::log_print(&format!(
        "[WARN] Replaced unavailable {prefix} algorithm {algorithm} with {fallback}"
    ));
    Ok(())
}

fn prefixed_files(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let needle = format!("{prefix}_");
    let mut files = dir
        .read_dir()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            (entry.path().is_file() && name.starts_with(&needle)).then(|| entry.path())
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn ensure_prefixed_config(
    staging_dir: &Path,
    live_dir: &Path,
    prefix: &str,
    fallback: &str,
    available: &[String],
) -> io::Result<()> {
    let staged = prefixed_files(staging_dir, prefix);
    let selected = staged.iter().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(&format!("{prefix}_")))
            .is_some_and(|algorithm| available.iter().any(|item| item == algorithm))
    });
    if let Some(selected) = selected {
        for stale in staged.iter().filter(|path| *path != selected) {
            fs::remove_file(stale)?;
        }
        return Ok(());
    }
    for stale in staged {
        fs::remove_file(stale)?;
    }

    if staging_dir != live_dir {
        if let Some(source) = prefixed_files(live_dir, prefix).into_iter().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(&format!("{prefix}_")))
                .is_some_and(|algorithm| available.iter().any(|item| item == algorithm))
        }) {
            let destination = staging_dir.join(source.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "configuration filename missing")
            })?);
            fs::copy(&source, &destination)?;
            logging::log_print(&format!("Preserved: {}", destination.display()));
            return Ok(());
        }
    }

    let target = staging_dir.join(format!("{prefix}_{fallback}"));
    fs::write(&target, "")?;
    logging::log_print(&format!("Created: {}", target.display()));
    Ok(())
}

fn preserve_exact_config(staging_dir: &Path, live_dir: &Path, name: &str) -> io::Result<()> {
    let destination = staging_dir.join(name);
    if destination.exists() || staging_dir == live_dir {
        return Ok(());
    }
    let source = live_dir.join(name);
    if source.is_file() {
        fs::copy(&source, &destination)?;
        logging::log_print(&format!("Preserved: {name}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{preserve_exact_config, safe_fallback_algorithm};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn installer_fallback_is_available_on_non_cubic_kernels() {
        let available = vec!["reno".to_string()];
        assert_eq!(safe_fallback_algorithm(&available), "reno");
    }

    #[test]
    fn upgrade_preserves_advanced_config() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tcp-optimiser-install-{unique}"));
        let live = root.join("live");
        let staging = root.join("staging");
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(live.join("advanced.conf"), "tcp_fin_timeout=30\n").unwrap();

        preserve_exact_config(&staging, &live, "advanced.conf").unwrap();

        assert_eq!(
            fs::read_to_string(staging.join("advanced.conf")).unwrap(),
            "tcp_fin_timeout=30\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
