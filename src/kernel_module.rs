use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
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
    #[serde(default)]
    kernel_release: Option<String>,
}

/// Return the full Android GKI KMI version, e.g. `6.6-android15-8`, when it
/// can be derived from the running kernel release string.
pub fn current_kmi() -> Option<String> {
    let release = kernel_release().ok()?;
    derive_kmi(&release)
}

#[derive(Debug, Clone, Serialize)]
pub struct KernelBundleStatus {
    pub kernel_release: String,
    pub kmi: Option<String>,
    pub arch: String,
    pub manifest_present: bool,
    pub matching_mode: String,
    pub matched_modules: usize,
    pub bundled_algorithms: Vec<String>,
    pub bundled_qdiscs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_load_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn bundle_status() -> KernelBundleStatus {
    let kernel_release = kernel_release().unwrap_or_else(|_| "unknown".to_string());
    let kmi = derive_kmi(&kernel_release);
    let arch = current_arch().to_string();
    let manifest_present = crate::config::module_dir()
        .join("kernel_modules")
        .join("manifest.json")
        .is_file();

    let (matching_mode, matched_modules, error) = match module_index() {
        Ok(Some(index)) if !index.entries.is_empty() => {
            let exact = index
                .entries
                .iter()
                .filter(|entry| entry.kernel_release.as_deref() == Some(kernel_release.as_str()))
                .count();
            let mode = if exact == index.entries.len() {
                "exact_release"
            } else if exact == 0 {
                "kmi"
            } else {
                "mixed"
            };
            (mode.to_string(), index.entries.len(), None)
        }
        Ok(Some(_)) | Ok(None) => ("none".to_string(), 0, None),
        Err(error) => ("invalid".to_string(), 0, Some(error.to_string())),
    };

    KernelBundleStatus {
        kernel_release,
        kmi,
        arch,
        manifest_present,
        matching_mode,
        matched_modules,
        bundled_algorithms: bundled_algorithms(),
        bundled_qdiscs: bundled_qdiscs(),
        last_load_error: fs::read_to_string(
            crate::config::module_dir().join("kernel_module_last_error"),
        )
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()),
        error,
    }
}

pub fn augment_algorithms(mut native: Vec<String>) -> Vec<String> {
    // A previous insmod failure is diagnostic state, not a permanent
    // capability verdict. Keep bundled algorithms visible so the user can
    // retry after reboot/module replacement instead of turning one transient
    // failure into a permanent UI "unsupported" state.
    for algorithm in bundled_algorithms() {
        if !native.iter().any(|item| item == &algorithm) {
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
    let blocked = unavailable_qdiscs();
    crate::config::KNOWN_QDISCS
        .iter()
        .filter(|qdisc| !blocked.contains(**qdisc))
        .filter(|qdisc| {
            qdisc_modules(qdisc)
                .is_some_and(|required| required.iter().all(|module| modules.contains(*module)))
        })
        .map(|qdisc| (*qdisc).to_string())
        .collect()
}

pub fn mark_qdisc_unavailable(qdisc: &str) -> io::Result<()> {
    if !crate::config::is_known_qdisc(qdisc) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown qdisc: {qdisc}"),
        ));
    }
    let path = crate::config::module_dir().join("unavailable_qdiscs");
    let mut blocked = unavailable_qdiscs();
    blocked.insert(qdisc.to_string());
    let mut values = blocked.into_iter().collect::<Vec<_>>();
    values.sort();
    fs::write(path, format!("{}\n", values.join(" ")))
}

pub fn qdisc_marked_unavailable(qdisc: &str) -> bool {
    unavailable_qdiscs().contains(qdisc)
}

pub fn clear_qdisc_unavailable(qdisc: &str) -> io::Result<()> {
    let path = crate::config::module_dir().join("unavailable_qdiscs");
    let mut blocked = unavailable_qdiscs();
    if !blocked.remove(qdisc) {
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

fn unavailable_qdiscs() -> HashSet<String> {
    fs::read_to_string(crate::config::module_dir().join("unavailable_qdiscs"))
        .ok()
        .map(|content| {
            content
                .split_whitespace()
                .filter(|qdisc| crate::config::is_known_qdisc(qdisc))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
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
        "bbr3" => "tcp_bbr3",
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

    let release = kernel_release().map_err(|error| error.to_string())?;
    let kmi = derive_kmi(&release);
    let arch = current_arch();
    let mut entries = Vec::new();
    let mut names = HashSet::new();

    for entry in manifest
        .modules
        .into_iter()
        .filter(|entry| entry_matches_kernel(entry, &release, kmi.as_deref(), arch))
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

fn entry_matches_kernel(
    entry: &ModuleEntry,
    release: &str,
    derived_kmi: Option<&str>,
    arch: &str,
) -> bool {
    if entry.arch != arch {
        return false;
    }

    match entry.kernel_release.as_deref() {
        // Exact-release matching is stricter than KMI matching and also works
        // for paired kernels whose uname -r has no Android ABI-generation tag.
        Some(expected) => expected == release,
        None => derived_kmi.is_some_and(|kmi| entry.kmi == kmi),
    }
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

fn parse_kallsyms_address(content: &str, symbol: &str) -> io::Result<u64> {
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(address) = fields.next() else {
            continue;
        };
        let _kind = fields.next();
        let Some(name) = fields.next() else {
            continue;
        };
        if name != symbol {
            continue;
        }
        let value = u64::from_str_radix(address, 16).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid kallsyms address for {symbol}: {error}"),
            )
        })?;
        if value == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "kallsyms hides the live address for {symbol}; root/CAP_SYSLOG access is required"
                ),
            ));
        }
        return Ok(value);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("kernel symbol not found: {symbol}"),
    ))
}

fn bbr3_runtime_params() -> io::Result<[String; 2]> {
    const KPTR_RESTRICT: &str = "/proc/sys/kernel/kptr_restrict";

    let original = fs::read_to_string(KPTR_RESTRICT)
        .unwrap_or_default()
        .trim()
        .to_string();
    let temporarily_relaxed = original == "2";

    if temporarily_relaxed {
        fs::write(KPTR_RESTRICT, "1\n").map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot temporarily relax kernel.kptr_restrict from 2 to 1 for BBR3 symbol resolution: {error}"
                ),
            )
        })?;
    }

    let resolved = (|| {
        // Open /proc/kallsyms only once. OnePlus' kernel snapshots symbol
        // visibility at open time; two independent reads can observe
        // different kptr_restrict states during a concurrent load attempt.
        let kallsyms = fs::read_to_string("/proc/kallsyms")?;
        let register = parse_kallsyms_address(&kallsyms, "tcp_register_congestion_control")?;
        let unregister = parse_kallsyms_address(&kallsyms, "tcp_unregister_congestion_control")?;
        Ok([
            format!("bbr3_register_addr=0x{register:x}"),
            format!("bbr3_unregister_addr=0x{unregister:x}"),
        ])
    })();

    if temporarily_relaxed {
        if let Err(error) = fs::write(KPTR_RESTRICT, "2\n") {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "failed to restore kernel.kptr_restrict=2 after BBR3 symbol resolution: {error}"
                ),
            ));
        }
    }

    resolved
}

fn bbr3_load_lock() -> io::Result<fs::File> {
    let path = crate::config::module_dir().join(".bbr3_load.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

fn try_load(module_name: &str) -> io::Result<bool> {
    if module_present(module_name) {
        return Ok(true);
    }

    // Serialize BBR3 loads across post-fs-data, daemon and WebUI processes.
    // Dropping the file descriptor releases flock automatically.
    let _bbr3_lock = if module_name == "tcp_bbr3" {
        Some(bbr3_load_lock()?)
    } else {
        None
    };

    // Another process may have loaded the module while this process waited.
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

    let mut command = Command::new("insmod");
    command.arg(&path);
    if module_name == "tcp_bbr3" {
        for parameter in bbr3_runtime_params()? {
            command.arg(parameter);
        }
    }
    let output = command.output()?;
    if output.status.success() || module_present(module_name) {
        let _ = fs::remove_file(crate::config::module_dir().join("kernel_module_last_error"));
        return Ok(true);
    }

    let message = format!(
        "insmod {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let _ = fs::write(
        crate::config::module_dir().join("kernel_module_last_error"),
        format!("{module_name}: {message}\n"),
    );
    Err(io::Error::other(message))
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
    // Prefer the kernel's procfs identity. On some rooted Android builds the
    // uname syscall can be virtualized independently from the module ABI,
    // while /proc/sys/kernel/osrelease continues to reflect the kernel that
    // resolves module symbols. Fall back to uname for ordinary systems.
    if let Ok(release) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        let release = release.trim();
        if !release.is_empty() {
            return Ok(release.to_string());
        }
    }

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
    fn rejects_zero_kallsyms_address_when_kptr_is_hidden() {
        let data = "0000000000000000 T tcp_register_congestion_control\n";
        let error = parse_kallsyms_address(data, "tcp_register_congestion_control").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn parses_nonzero_kallsyms_symbol_address() {
        let data = "ffffffc080123400 T tcp_register_congestion_control\n";
        assert_eq!(
            parse_kallsyms_address(data, "tcp_register_congestion_control").unwrap(),
            0xffffffc080123400
        );
    }

    #[test]
    fn rejects_hidden_kallsyms_symbol_address() {
        let data = "0000000000000000 T tcp_register_congestion_control\n";
        let error = parse_kallsyms_address(data, "tcp_register_congestion_control").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn exact_release_entry_only_matches_identical_uname_release() {
        let entry = ModuleEntry {
            name: "tcp_bbr3".to_string(),
            kmi: "6.6-android15-8".to_string(),
            arch: "aarch64".to_string(),
            file: "android15-6.6/aarch64/tcp_bbr3.ko".to_string(),
            sha256: "0".repeat(64),
            kernel_release: Some("6.6.30-android15-8-g123456789abc-ab12345678".to_string()),
        };
        let release = "6.6.139-4k-gce3170e88ddc";
        let mut paired = entry.clone();
        paired.kmi = "android15-6.6".to_string();
        paired.kernel_release = Some(release.to_string());

        assert!(entry_matches_kernel(&paired, release, None, "aarch64"));
        assert!(!entry_matches_kernel(
            &paired,
            "6.6.139-4k-gdifferent",
            None,
            "aarch64"
        ));
        assert!(!entry_matches_kernel(&paired, release, None, "x86_64"));
    }

    #[test]
    fn rejects_manifest_path_traversal() {
        assert!(safe_relative_path("../tcp_bbr3.ko").is_err());
        assert!(safe_relative_path("/data/local/tmp/tcp_bbr3.ko").is_err());
        assert!(safe_relative_path("android15-6.6/aarch64/tcp_bbr3.ko").is_ok());
    }

    #[test]
    fn complex_kernel_identity_matrix_never_weakens_exact_release_matching() {
        let release = "6.6.30-android15-8-g123456789abc-ab12345678";
        let kmi = "6.6-android15-8";

        let exact = ModuleEntry {
            name: "tcp_bbr3".to_string(),
            kmi: kmi.to_string(),
            arch: "aarch64".to_string(),
            file: "android15-6.6/aarch64/tcp_bbr3.ko".to_string(),
            sha256: "0".repeat(64),
            kernel_release: Some(release.to_string()),
        };
        assert!(entry_matches_kernel(&exact, release, Some(kmi), "aarch64"));
        assert!(!entry_matches_kernel(
            &exact,
            "6.6.31-android15-8-gdifferent-ab99999999",
            Some(kmi),
            "aarch64"
        ));
        assert!(!entry_matches_kernel(&exact, release, Some(kmi), "x86_64"));

        let kmi_only = ModuleEntry {
            kernel_release: None,
            ..exact.clone()
        };
        assert!(entry_matches_kernel(
            &kmi_only,
            release,
            Some(kmi),
            "aarch64"
        ));
        assert!(!entry_matches_kernel(
            &kmi_only,
            release,
            Some("6.6-android15-9"),
            "aarch64"
        ));
        assert!(!entry_matches_kernel(&kmi_only, release, None, "aarch64"));
    }

    #[test]
    fn kernel_module_hash_verification_rejects_tampering() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tcp-optimiser-ko-{unique}.ko"));
        fs::write(&path, b"known module bytes").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"known module bytes"));
        assert!(verify_sha256(&path, &expected).is_ok());

        fs::write(&path, b"tampered module bytes").unwrap();
        assert!(verify_sha256(&path, &expected).is_err());
        let _ = fs::remove_file(path);
    }
}
