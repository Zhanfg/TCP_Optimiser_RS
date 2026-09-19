use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct BundleManifest {
    schema: u32,
    modules: Vec<ModuleEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModuleEntry {
    name: String,
    kmi: String,
    arch: String,
    file: String,
    sha256: String,
}

/// Return the full Android GKI KMI version, e.g. `6.6-android15-8`, when it
/// can be derived from the running kernel release string.
pub fn current_kmi() -> Option<String> {
    let release = kernel_release().ok()?;
    derive_kmi(&release)
}

pub fn augment_algorithms(mut native: Vec<String>) -> Vec<String> {
    let blocked = unavailable_algorithms();
    for algorithm in bundled_algorithms() {
        if !blocked.contains(&algorithm) && !native.iter().any(|item| item == &algorithm) {
            native.push(algorithm);
        }
    }
    native
}

pub fn mark_algorithm_unavailable(algorithm: &str) -> io::Result<()> {
    if !crate::config::is_known_algorithm(algorithm) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown congestion algorithm: {algorithm}"),
        ));
    }
    let path = crate::config::module_dir().join("unavailable_algos");
    let mut blocked = unavailable_algorithms();
    blocked.insert(algorithm.to_string());
    let mut values = blocked.into_iter().collect::<Vec<_>>();
    values.sort();
    fs::write(path, format!("{}\n", values.join(" ")))
}

pub fn clear_algorithm_unavailable(algorithm: &str) -> io::Result<()> {
    let path = crate::config::module_dir().join("unavailable_algos");
    let mut blocked = unavailable_algorithms();
    if !blocked.remove(algorithm) {
        return Ok(());
    }
    if blocked.is_empty() {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    let mut values = blocked.into_iter().collect::<Vec<_>>();
    values.sort();
    fs::write(path, format!("{}\n", values.join(" ")))
}

fn unavailable_algorithms() -> HashSet<String> {
    fs::read_to_string(crate::config::module_dir().join("unavailable_algos"))
        .ok()
        .map(|content| {
            content
                .split_whitespace()
                .filter(|algorithm| crate::config::is_known_algorithm(algorithm))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn bundled_algorithms() -> Vec<String> {
    let modules = matching_module_names().unwrap_or_default();
    crate::config::ALL_ALGOS
        .iter()
        .filter_map(|algorithm| {
            algorithm_module(algorithm)
                .filter(|module| modules.contains(*module))
                .map(|_| (*algorithm).to_string())
        })
        .collect()
}

pub fn bundled_qdiscs() -> Vec<String> {
    let modules = matching_module_names().unwrap_or_default();
    crate::config::KNOWN_QDISCS
        .iter()
        .filter(|qdisc| {
            qdisc_modules(qdisc)
                .is_some_and(|required| required.iter().all(|module| modules.contains(*module)))
        })
        .map(|qdisc| (*qdisc).to_string())
        .collect()
}

pub fn ensure_algorithm(algorithm: &str) -> io::Result<bool> {
    let Some(module) = algorithm_module(algorithm) else {
        return Ok(false);
    };
    try_load(module)
}

fn algorithm_module(algorithm: &str) -> Option<&'static str> {
    Some(match algorithm {
        "bbr" => "tcp_bbr",
        "bbr1" => "tcp_bbr1",
        "bbr2" => "tcp_bbr2",
        "bbr3" => "tcp_bbr3",
        "bic" => "tcp_bic",
        "cdg" => "tcp_cdg",
        "dctcp" => "tcp_dctcp",
        "highspeed" => "tcp_highspeed",
        "htcp" => "tcp_htcp",
        "hybla" => "tcp_hybla",
        "illinois" => "tcp_illinois",
        "lp" => "tcp_lp",
        "nv" => "tcp_nv",
        "scalable" => "tcp_scalable",
        "vegas" => "tcp_vegas",
        "westwood" => "tcp_westwood",
        "yeah" => "tcp_yeah",
        _ => return None,
    })
}

pub fn ensure_qdisc(qdisc: &str) -> io::Result<bool> {
    let Some(modules) = qdisc_modules(qdisc) else {
        return Ok(false);
    };

    let mut loaded_any = false;
    for module in modules {
        loaded_any |= try_load(module)?;
    }
    Ok(loaded_any)
}

fn qdisc_modules(qdisc: &str) -> Option<&'static [&'static str]> {
    Some(match qdisc {
        "fq" => &["sch_fq"],
        "fq_codel" => &["sch_codel", "sch_fq_codel"],
        "codel" => &["sch_codel"],
        "cake" => &["sch_cake"],
        "pie" => &["sch_pie"],
        "fq_pie" => &["sch_pie", "sch_fq_pie"],
        _ => return None,
    })
}

#[derive(Debug)]
struct ModuleIndex {
    root: PathBuf,
    entries: Vec<ModuleEntry>,
    names: HashSet<String>,
}

static MODULE_INDEX: OnceLock<Result<Option<ModuleIndex>, String>> = OnceLock::new();

fn build_module_index() -> Result<Option<ModuleIndex>, String> {
    let root = crate::config::module_dir().join("kernel_modules");
    let manifest_path = root.join("manifest.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest: BundleManifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("invalid kernel module manifest: {error}"))?;
    if manifest.schema != 1 {
        return Err(format!(
            "unsupported kernel module manifest schema {}",
            manifest.schema
        ));
    }

    let Some(kmi) = current_kmi() else {
        return Ok(None);
    };
    let arch = current_arch();
    let mut entries = Vec::new();
    let mut names = HashSet::new();

    for entry in manifest
        .modules
        .into_iter()
        .filter(|entry| entry.kmi == kmi && entry.arch == arch)
    {
        let relative = safe_relative_path(&entry.file).map_err(|error| error.to_string())?;
        if root.join(relative).is_file() {
            names.insert(entry.name.clone());
            entries.push(entry);
        }
    }

    Ok(Some(ModuleIndex {
        root,
        entries,
        names,
    }))
}

fn module_index() -> io::Result<Option<&'static ModuleIndex>> {
    match MODULE_INDEX.get_or_init(build_module_index) {
        Ok(Some(index)) => Ok(Some(index)),
        Ok(None) => Ok(None),
        Err(error) => Err(invalid_data(error.clone())),
    }
}

fn matching_module_names() -> io::Result<HashSet<String>> {
    Ok(module_index()?
        .map(|index| index.names.clone())
        .unwrap_or_default())
}

fn try_load(module_name: &str) -> io::Result<bool> {
    if module_present(module_name) {
        return Ok(true);
    }

    let Some(index) = module_index()? else {
        return Ok(false);
    };
    let Some(entry) = index.entries.iter().find(|entry| entry.name == module_name) else {
        return Ok(false);
    };

    let relative = safe_relative_path(&entry.file)?;
    let path = index.root.join(relative);
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("bundled kernel module is missing: {}", path.display()),
        ));
    }
    verify_sha256(&path, &entry.sha256)?;

    let output = Command::new("insmod").arg(&path).output()?;
    if output.status.success() || module_present(module_name) {
        return Ok(true);
    }

    Err(io::Error::other(format!(
        "insmod {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn module_present(module_name: &str) -> bool {
    Path::new("/sys/module").join(module_name).exists()
        || fs::read_to_string("/proc/modules")
            .ok()
            .is_some_and(|content| {
                content
                    .lines()
                    .any(|line| line.split_whitespace().next() == Some(module_name))
            })
}

fn kernel_release() -> io::Result<String> {
    let output = Command::new("uname").arg("-r").output()?;
    if !output.status.success() {
        return Err(io::Error::other("uname -r failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn current_arch() -> &'static str {
    std::env::consts::ARCH
}

fn derive_kmi(release: &str) -> Option<String> {
    // GKI kernel release:
    //   w.x.y-androidN-k-suffix
    // KMI version:
    //   w.x-androidN-k
    // The KMI generation (k) is ABI-significant and must not be discarded.
    let mut fields = release.split('-');
    let version = fields.next()?;
    let android = fields.next()?;
    let kmi_generation = fields.next()?;

    let mut version_parts = version.split('.');
    let major = version_parts.next()?;
    let minor = version_parts.next()?;
    let _sublevel = version_parts.next()?;

    if !major.chars().all(|c| c.is_ascii_digit())
        || !minor.chars().all(|c| c.is_ascii_digit())
        || !android
            .strip_prefix("android")
            .is_some_and(|value| !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()))
        || kmi_generation.is_empty()
        || !kmi_generation.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }

    Some(format!("{major}.{minor}-{android}-{kmi_generation}"))
}

fn safe_relative_path(value: &str) -> io::Result<PathBuf> {
    if value.is_empty() || value.contains('\\') || value.contains('\0') {
        return Err(invalid_data("unsafe kernel module path"));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_data("unsafe kernel module path"));
    }
    Ok(path.to_path_buf())
}

fn verify_sha256(path: &Path, expected: &str) -> io::Result<()> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_data("invalid kernel module SHA-256"));
    }

    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(invalid_data(format!(
            "kernel module hash mismatch: {}",
            path.display()
        )))
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_android_gki_family() {
        assert_eq!(
            derive_kmi("6.6.30-android15-8-g123456789abc-ab12345678"),
            Some("6.6-android15-8".to_string())
        );
        assert_eq!(
            derive_kmi("5.15.153-android13-8-00001-gdeadbeef"),
            Some("5.15-android13-8".to_string())
        );
        assert_eq!(derive_kmi("6.6.30-custom"), None);
    }

    #[test]
    fn rejects_manifest_path_traversal() {
        assert!(safe_relative_path("../tcp_bbr3.ko").is_err());
        assert!(safe_relative_path("/data/local/tmp/tcp_bbr3.ko").is_err());
        assert!(safe_relative_path("android15-6.6/aarch64/tcp_bbr3.ko").is_ok());
    }
}
