use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::baseline;
use crate::config;
use crate::logging;
use crate::sysctl;

const BASELINE_FILE: &str = "baseline-v1.json";
const BASELINE_PROVENANCE_FILE: &str = "baseline-provenance-v1.json";
const BASELINE_PROVENANCE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallBaselineMode {
    ExactPreModule,
    PreserveExisting,
    LegacyUpgradeSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct BaselineProvenance {
    format_version: u32,
    provenance: String,
    exact_pre_module: bool,
    note: String,
}

/// Run module installation / upgrade logic (replaces customize.sh).
/// `MODPATH` or `TCP_OPTIMISER_MODULE_DIR` identifies the staging directory.
pub fn run() -> io::Result<()> {
    let staging_dir = config::module_dir();
    crate::integrity::verify_module(&staging_dir)?;
    fs::create_dir_all(&staging_dir)?;
    logging::log_print("Starting module customization (Rust)...");
    let live_dir = config::live_module_dir();
    let baseline_mode = classify_baseline_mode(&staging_dir, &live_dir);

    preserve_exact_config(&staging_dir, &live_dir, BASELINE_FILE)?;
    preserve_exact_config(&staging_dir, &live_dir, BASELINE_PROVENANCE_FILE)?;
    ensure_baseline_provenance(&staging_dir, baseline_mode)?;

    let baseline = baseline::ensure_global_baseline()?;
    logging::log_print(&format!(
        "Kernel baseline ready (sysctls={}, interfaces={}, captured_at={}, provenance={}).",
        baseline.sysctl_count,
        baseline.interface_count,
        baseline.captured_at_epoch,
        baseline_mode_name(baseline_mode)
    ));
    if baseline_mode == InstallBaselineMode::LegacyUpgradeSnapshot {
        logging::log_print(
            "[WARN] Direct legacy upgrade uses a pre-upgrade compatibility snapshot, not a verified vendor-default baseline.",
        );
    }

    let available = sysctl::available_algorithms().unwrap_or_else(|error| {
        logging::log_print(&format!(
            "[WARN] Cannot read congestion algorithms: {error}"
        ));
        vec!["cubic".to_string()]
    });
    fs::write(staging_dir.join("available_algos"), available.join(" "))?;

    let safe_fallback = safe_fallback_algorithm(&available);
    let default_algo = if available.iter().any(|algo| algo == "bbr") {
        "bbr"
    } else {
        safe_fallback
    };
    ensure_prefixed_config(&staging_dir, &live_dir, "wlan", default_algo, &available)?;
    ensure_prefixed_config(
        &staging_dir,
        &live_dir,
        "rmnet_data",
        safe_fallback,
        &available,
    )?;

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
        "runtime-control-v1.json",
        "last-good-policy-v2.json",
        "policy-failures-v1.json",
    ] {
        preserve_exact_config(&staging_dir, &live_dir, name)?;
    }
    logging::log_print(&format!(
        "Module customization complete (default={default_algo}, algorithms={}).",
        available.join(" ")
    ));
    Ok(())
}

fn classify_baseline_mode(staging_dir: &Path, live_dir: &Path) -> InstallBaselineMode {
    if staging_dir == live_dir || !live_dir.join("module.prop").is_file() {
        return InstallBaselineMode::ExactPreModule;
    }
    if live_dir.join(BASELINE_FILE).is_file() {
        InstallBaselineMode::PreserveExisting
    } else {
        InstallBaselineMode::LegacyUpgradeSnapshot
    }
}

fn baseline_mode_name(mode: InstallBaselineMode) -> &'static str {
    match mode {
        InstallBaselineMode::ExactPreModule => "exact_pre_module",
        InstallBaselineMode::PreserveExisting => "preserved_existing",
        InstallBaselineMode::LegacyUpgradeSnapshot => "legacy_upgrade_snapshot",
    }
}

fn ensure_baseline_provenance(staging_dir: &Path, mode: InstallBaselineMode) -> io::Result<()> {
    let path = staging_dir.join(BASELINE_PROVENANCE_FILE);
    if path.is_file() {
        validate_baseline_provenance(&path)?;
        return Ok(());
    }

    let (provenance, exact_pre_module, note) = match mode {
        InstallBaselineMode::ExactPreModule => (
            "exact_pre_module",
            true,
            "Captured before this module first applied managed kernel changes.",
        ),
        InstallBaselineMode::PreserveExisting => (
            "exact_pre_module",
            true,
            "Preserved from an earlier transactional TCP Optimiser installation.",
        ),
        InstallBaselineMode::LegacyUpgradeSnapshot => (
            "legacy_upgrade_snapshot",
            false,
            "Captured immediately before replacing a legacy TCP Optimiser installation; restores the pre-upgrade state but is not guaranteed to be the vendor default.",
        ),
    };
    let document = BaselineProvenance {
        format_version: BASELINE_PROVENANCE_VERSION,
        provenance: provenance.to_string(),
        exact_pre_module,
        note: note.to_string(),
    };
    write_json_atomic(&path, &document)
}

fn validate_baseline_provenance(path: &Path) -> io::Result<BaselineProvenance> {
    let bytes = fs::read(path)?;
    let document: BaselineProvenance = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid baseline provenance {}: {error}", path.display()),
        )
    })?;
    if document.format_version != BASELINE_PROVENANCE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported baseline provenance format {}",
                document.format_version
            ),
        ));
    }
    if !matches!(
        document.provenance.as_str(),
        "exact_pre_module" | "legacy_upgrade_snapshot"
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown baseline provenance {}", document.provenance),
        ));
    }
    if document.exact_pre_module != (document.provenance == "exact_pre_module") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "baseline provenance exact_pre_module flag is inconsistent",
        ));
    }
    Ok(document)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
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
    use super::{
        classify_baseline_mode, ensure_baseline_provenance, preserve_exact_config,
        safe_fallback_algorithm, validate_baseline_provenance, InstallBaselineMode, BASELINE_FILE,
        BASELINE_PROVENANCE_FILE,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_dirs(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tcp-optimiser-{label}-{unique}"));
        let live = root.join("live");
        let staging = root.join("staging");
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&staging).unwrap();
        (root, live, staging)
    }

    #[test]
    fn installer_fallback_is_available_on_non_cubic_kernels() {
        let available = vec!["reno".to_string()];
        assert_eq!(safe_fallback_algorithm(&available), "reno");
    }

    #[test]
    fn upgrade_preserves_advanced_config() {
        let (root, live, staging) = temporary_dirs("install");
        fs::write(live.join("advanced.conf"), "tcp_fin_timeout=30\n").unwrap();

        preserve_exact_config(&staging, &live, "advanced.conf").unwrap();

        assert_eq!(
            fs::read_to_string(staging.join("advanced.conf")).unwrap(),
            "tcp_fin_timeout=30\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upgrade_preserves_original_kernel_baseline() {
        let (root, live, staging) = temporary_dirs("baseline");
        fs::write(live.join(BASELINE_FILE), "{\"version\":1}\n").unwrap();

        preserve_exact_config(&staging, &live, BASELINE_FILE).unwrap();

        assert_eq!(
            fs::read_to_string(staging.join(BASELINE_FILE)).unwrap(),
            "{\"version\":1}\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_upgrade_without_baseline_is_allowed_with_compatibility_provenance() {
        let (root, live, staging) = temporary_dirs("legacy");
        fs::write(live.join("module.prop"), "id=tcp_optimiser\n").unwrap();

        let mode = classify_baseline_mode(&staging, &live);
        assert_eq!(mode, InstallBaselineMode::LegacyUpgradeSnapshot);
        ensure_baseline_provenance(&staging, mode).unwrap();
        let provenance =
            validate_baseline_provenance(&staging.join(BASELINE_PROVENANCE_FILE)).unwrap();
        assert_eq!(provenance.provenance, "legacy_upgrade_snapshot");
        assert!(!provenance.exact_pre_module);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transactional_upgrade_preserves_existing_baseline_mode() {
        let (root, live, staging) = temporary_dirs("transactional");
        fs::write(live.join("module.prop"), "id=tcp_optimiser\n").unwrap();
        fs::write(live.join(BASELINE_FILE), "{\"version\":1}\n").unwrap();

        assert_eq!(
            classify_baseline_mode(&staging, &live),
            InstallBaselineMode::PreserveExisting
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_install_provenance_is_consistent() {
        let (root, _live, staging) = temporary_dirs("fresh");
        ensure_baseline_provenance(&staging, InstallBaselineMode::ExactPreModule).unwrap();
        let provenance =
            validate_baseline_provenance(&staging.join(BASELINE_PROVENANCE_FILE)).unwrap();
        assert_eq!(provenance.provenance, "exact_pre_module");
        assert!(provenance.exact_pre_module);
        fs::remove_dir_all(root).unwrap();
    }
}
