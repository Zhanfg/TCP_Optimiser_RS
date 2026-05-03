use std::fs;
use std::io;
use std::path::Path;

use crate::config;
use crate::logging;
use crate::sysctl;

/// Run module installation / upgrade logic (replaces customize.sh)
pub fn run() -> io::Result<()> {
    logging::log_print("Starting module customization (Rust)...");

    // Detect BBR availability
    let has_bbr = sysctl::algo_available("bbr").unwrap_or(false);
    let default_algo = if has_bbr { "bbr" } else { "cubic" };

    logging::log_print(&format!(
        "TCP congestion: {} {}",
        if has_bbr { "Found BBR!" } else { "BBR not found. Going with Cubic!" },
        ""
    ));

    let mod_dir = config::module_dir();
    let mod_path = &mod_dir;

    // Detect KSU vs Magisk
    let is_ksu = std::env::var("KSU").ok().map(|v| v == "true").unwrap_or(false);

    // Create wlan_<algo> based on BBR availability
    create_file_if_needed(mod_path, "wlan", default_algo, is_ksu)?;

    // Always create rmnet_data_cubic as default
    create_file_if_needed(mod_path, "rmnet_data", "cubic", is_ksu)?;

    // Create optional feature touch files
    for name in &["kill_connections", "initcwnd_initrwnd"] {
        if check_exists_anywhere(mod_path, name, is_ksu) {
            if is_ksu {
                // Copy from existing module dir
                let src = mod_path.join(name);
                if src.exists() {
                    fs::copy(&src, mod_path.join(name))?;
                    logging::log_print(&format!("Copied {name} from module dir"));
                }
            } else {
                logging::log_print(&format!("Skipping {name}: already exists"));
            }
        }
    }

    logging::log_print("Module customization complete.");
    Ok(())
}

/// Check if a config file exists in either the module dir or the live update dir
fn check_exists_anywhere(mod_path: &Path, name: &str, _is_ksu: bool) -> bool {
    // Check for prefix_* files
    let prefix = name;
    if mod_path.read_dir().ok().map(|d| {
        d.filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with(&format!("{prefix}_")))
    }).unwrap_or(false) {
        return true;
    }
    // Check exact file
    mod_path.join(name).exists()
}

/// Create a touch file if it doesn't exist, considering KSU migration
fn create_file_if_needed(
    mod_path: &Path,
    prefix: &str,
    suffix: &str,
    is_ksu: bool,
) -> io::Result<()> {
    if check_exists_anywhere(mod_path, prefix, is_ksu) {
        if is_ksu {
            // Find and copy any file starting with prefix_ from mod_path
            if let Ok(entries) = mod_path.read_dir() {
                for entry in entries.flatten() {
                    let fname = entry.file_name();
                    let fname_str = fname.to_string_lossy();
                    if fname_str.starts_with(&format!("{prefix}_")) {
                        let dest = mod_path.join(fname_str.as_ref());
                        fs::copy(entry.path(), &dest)?;
                        logging::log_print(&format!(
                            "Copied from module dir: {}",
                            fname_str
                        ));
                        return Ok(());
                    }
                }
            }
        }
        logging::log_print(&format!("Skipping {prefix}_{suffix}: already exists"));
        return Ok(());
    }

    let target = mod_path.join(format!("{prefix}_{suffix}"));
    if !target.exists() {
        fs::write(&target, "")?;
        logging::log_print(&format!("Created: {}", target.display()));
    }

    Ok(())
}
