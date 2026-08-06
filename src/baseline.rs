use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{config, network, sysctl};

const BASELINE_FILE: &str = "baseline-v1.json";
const BASELINE_VERSION: u32 = 1;
const LOCK_FILE: &str = ".baseline.lock";
const LOCK_RETRIES: usize = 80;
const LOCK_RETRY_DELAY_MS: u64 = 25;
const STALE_LOCK_SECONDS: u64 = 30;
const CONGESTION_CONTROL_PATH: &str = "/proc/sys/net/ipv4/tcp_congestion_control";
const DEFAULT_QDISC_PATH: &str = "/proc/sys/net/core/default_qdisc";

/// Complete allowlist of kernel nodes the module can modify. Restoration only
/// accepts paths from this list, so a damaged baseline cannot redirect writes
/// to arbitrary files even though uninstall runs with root privileges.
const MANAGED_SYSCTL_PATHS: &[&str] = &[
    CONGESTION_CONTROL_PATH,
    DEFAULT_QDISC_PATH,
    "/proc/sys/net/ipv4/tcp_pacing_ca_ratio",
    "/proc/sys/net/ipv4/tcp_pacing_ss_ratio",
    "/proc/sys/net/ipv4/tcp_ecn",
    "/proc/sys/net/ipv4/tcp_window_scaling",
    "/proc/sys/net/ipv4/tcp_max_syn_backlog",
    "/proc/sys/net/ipv4/tcp_mtu_probing",
    "/proc/sys/net/ipv4/tcp_fastopen",
    "/proc/sys/net/ipv4/tcp_tw_reuse",
    "/proc/sys/net/ipv4/tcp_rmem",
    "/proc/sys/net/ipv4/tcp_wmem",
    "/proc/sys/net/core/rmem_max",
    "/proc/sys/net/core/wmem_max",
    "/proc/sys/net/ipv4/tcp_keepalive_time",
    "/proc/sys/net/ipv4/tcp_keepalive_intvl",
    "/proc/sys/net/ipv4/tcp_keepalive_probes",
    "/proc/sys/net/ipv4/tcp_fin_timeout",
    "/proc/sys/net/ipv4/tcp_syn_retries",
    "/proc/sys/net/ipv4/tcp_synack_retries",
    "/proc/sys/net/ipv4/tcp_retries2",
    "/proc/sys/net/core/optmem_max",
    "/proc/sys/net/ipv4/tcp_notsent_lowat",
    "/proc/sys/net/core/somaxconn",
    "/proc/sys/net/core/netdev_max_backlog",
    "/proc/sys/net/core/netdev_budget",
    "/proc/sys/net/core/netdev_budget_usecs",
    "/proc/sys/net/ipv4/tcp_sack",
    "/proc/sys/net/ipv4/tcp_dsack",
    "/proc/sys/net/ipv4/tcp_no_metrics_save",
    "/proc/sys/net/ipv4/tcp_slow_start_after_idle",
    "/proc/sys/net/ipv4/tcp_autocorking",
    "/proc/sys/net/ipv4/tcp_early_retrans",
    "/proc/sys/net/ipv4/tcp_thin_linear_timeouts",
    "/proc/sys/net/ipv4/tcp_thin_dupack",
    "/proc/sys/net/ipv4/tcp_rto_max_ms",
    "/proc/sys/net/ipv4/tcp_plb_enabled",
    "/proc/sys/net/ipv4/tcp_plb_idle_rehash_rounds",
    "/proc/sys/net/ipv4/tcp_plb_rehash_rounds",
    "/proc/sys/net/ipv4/tcp_plb_suspend_rto_sec",
    "/proc/sys/net/ipv4/tcp_plb_cong_thresh",
    "/proc/sys/net/core/busy_poll",
    "/proc/sys/net/core/busy_read",
    "/proc/sys/net/netfilter/nf_conntrack_max",
    "/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_established",
    "/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_time_wait",
];

#[derive(Debug, Clone, Deserialize, Serialize)]
struct BaselineSnapshot {
    version: u32,
    captured_at_epoch: u64,
    sysctls: BTreeMap<String, String>,
    interfaces: BTreeMap<String, InterfaceBaseline>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct InterfaceBaseline {
    root_qdisc: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaselineSummary {
    pub path: String,
    pub captured_at_epoch: u64,
    pub sysctl_count: usize,
    pub interface_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub success: bool,
    pub restored_sysctls: usize,
    pub restored_interfaces: usize,
    pub errors: Vec<String>,
}

/// Persist the original kernel state exactly once before any tuning is applied.
/// Existing baselines are validated and reused so module upgrades never replace
/// the device's pre-module values with values written by an older module build.
pub fn ensure_global_baseline() -> io::Result<BaselineSummary> {
    let module_dir = config::module_dir();
    let _lock = BaselineLock::acquire(&module_dir)?;
    let path = module_dir.join(BASELINE_FILE);
    let snapshot = if path.exists() {
        load_snapshot(&path)?
    } else {
        let snapshot = capture_snapshot()?;
        write_snapshot(&path, &snapshot)?;
        snapshot
    };
    Ok(summary(&path, &snapshot))
}

/// Journal an interface's original root qdisc before the first replacement.
/// The same entry is reused across network handovers and future module upgrades.
pub fn ensure_interface_baseline(iface: &str) -> io::Result<()> {
    validate_interface_name(iface)?;
    let module_dir = config::module_dir();
    let _lock = BaselineLock::acquire(&module_dir)?;
    let path = module_dir.join(BASELINE_FILE);
    let mut snapshot = if path.exists() {
        load_snapshot(&path)?
    } else {
        capture_snapshot()?
    };

    if snapshot.interfaces.contains_key(iface) {
        if !path.exists() {
            write_snapshot(&path, &snapshot)?;
        }
        return Ok(());
    }

    let root_qdisc = network::root_qdisc(iface)?;
    snapshot
        .interfaces
        .insert(iface.to_string(), InterfaceBaseline { root_qdisc });
    write_snapshot(&path, &snapshot)
}

/// Restore all managed sysctls and every journaled interface qdisc.
/// Restoration is best-effort and returns a structured report containing every
/// failure instead of stopping after the first unsupported or vanished node.
pub fn restore() -> io::Result<RestoreReport> {
    let module_dir = config::module_dir();
    let _lock = BaselineLock::acquire(&module_dir)?;
    let path = module_dir.join(BASELINE_FILE);
    let snapshot = load_snapshot(&path)?;
    let managed = MANAGED_SYSCTL_PATHS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut report = RestoreReport {
        success: false,
        restored_sysctls: 0,
        restored_interfaces: 0,
        errors: Vec::new(),
    };

    for (path, value) in snapshot
        .sysctls
        .iter()
        .filter(|(path, _)| path.as_str() != DEFAULT_QDISC_PATH)
        .filter(|(path, _)| path.as_str() != CONGESTION_CONTROL_PATH)
    {
        restore_sysctl(path, value, &managed, &mut report);
    }
    for special_path in [DEFAULT_QDISC_PATH, CONGESTION_CONTROL_PATH] {
        if let Some(value) = snapshot.sysctls.get(special_path) {
            restore_sysctl(special_path, value, &managed, &mut report);
        }
    }

    for (iface, baseline) in &snapshot.interfaces {
        match restore_interface_qdisc(iface, baseline.root_qdisc.as_deref()) {
            Ok(()) => report.restored_interfaces += 1,
            Err(error) => report
                .errors
                .push(format!("interface {iface}: {error}")),
        }
    }

    report.errors.sort();
    report.errors.dedup();
    report.success = report.errors.is_empty();
    Ok(report)
}

fn capture_snapshot() -> io::Result<BaselineSnapshot> {
    let mut sysctls = BTreeMap::new();
    for path in MANAGED_SYSCTL_PATHS.iter().copied() {
        if !Path::new(path).exists() {
            continue;
        }
        let value = sysctl::read_sysctl(path)?;
        sysctls.insert(path.to_string(), value);
    }
    if sysctls.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no managed kernel sysctl nodes are readable",
        ));
    }

    let mut interfaces = BTreeMap::new();
    if let Ok(iface) = network::active_iface() {
        if validate_interface_name(&iface).is_ok() {
            if let Ok(root_qdisc) = network::root_qdisc(&iface) {
                interfaces.insert(iface, InterfaceBaseline { root_qdisc });
            }
        }
    }

    Ok(BaselineSnapshot {
        version: BASELINE_VERSION,
        captured_at_epoch: now_epoch(),
        sysctls,
        interfaces,
    })
}

fn restore_sysctl(
    path: &str,
    value: &str,
    managed: &BTreeSet<&'static str>,
    report: &mut RestoreReport,
) {
    if !managed.contains(path) {
        report
            .errors
            .push(format!("refused unmanaged sysctl path: {path}"));
        return;
    }
    if !Path::new(path).exists() {
        report.errors.push(format!("sysctl disappeared: {path}"));
        return;
    }
    match sysctl::write_sysctl(path, value) {
        Ok(()) => match sysctl::read_sysctl(path) {
            Ok(actual) if actual == value => report.restored_sysctls += 1,
            Ok(actual) => report.errors.push(format!(
                "sysctl readback mismatch: {path}, expected {value:?}, got {actual:?}"
            )),
            Err(error) => report
                .errors
                .push(format!("sysctl readback failed: {path}: {error}")),
        },
        Err(error) => report
            .errors
            .push(format!("sysctl restore failed: {path}: {error}")),
    }
}

fn restore_interface_qdisc(iface: &str, expected: Option<&str>) -> io::Result<()> {
    validate_interface_name(iface)?;
    if let Some(qdisc) = expected {
        validate_kernel_token(qdisc)?;
    }

    let current = network::root_qdisc(iface)?;
    if qdisc_equivalent(expected, current.as_deref()) {
        return Ok(());
    }

    let output = match expected {
        Some(qdisc) if !matches!(qdisc, "noqueue" | "mq") => Command::new("tc")
            .args(["qdisc", "replace", "dev", iface, "root", qdisc])
            .output()?,
        _ => Command::new("tc")
            .args(["qdisc", "del", "dev", iface, "root"])
            .output()?,
    };
    let actual = network::root_qdisc(iface)?;
    if qdisc_equivalent(expected, actual.as_deref()) {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "qdisc restore mismatch, expected {}, got {}; tc stderr: {}",
        expected.unwrap_or("none"),
        actual.as_deref().unwrap_or("none"),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn qdisc_equivalent(expected: Option<&str>, actual: Option<&str>) -> bool {
    match expected {
        None | Some("noqueue") => matches!(actual, None | Some("noqueue")),
        Some(value) => actual == Some(value),
    }
}

fn validate_interface_name(iface: &str) -> io::Result<()> {
    if iface.is_empty()
        || iface.len() > 64
        || !iface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid interface name: {iface:?}"),
        ));
    }
    Ok(())
}

fn validate_kernel_token(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid qdisc token in baseline: {value:?}"),
        ));
    }
    Ok(())
}

fn load_snapshot(path: &Path) -> io::Result<BaselineSnapshot> {
    let bytes = fs::read(path)?;
    let snapshot: BaselineSnapshot = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid baseline file {}: {error}", path.display()),
        )
    })?;
    if snapshot.version != BASELINE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported baseline version {}, expected {}",
                snapshot.version, BASELINE_VERSION
            ),
        ));
    }
    Ok(snapshot)
}

fn write_snapshot(path: &Path, snapshot: &BaselineSnapshot) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(io::Error::other)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

fn summary(path: &Path, snapshot: &BaselineSnapshot) -> BaselineSummary {
    BaselineSummary {
        path: path.display().to_string(),
        captured_at_epoch: snapshot.captured_at_epoch,
        sysctl_count: snapshot.sysctls.len(),
        interface_count: snapshot.interfaces.len(),
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct BaselineLock {
    path: PathBuf,
}

impl BaselineLock {
    fn acquire(module_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(module_dir)?;
        let path = module_dir.join(LOCK_FILE);
        for _ in 0..LOCK_RETRIES {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    writeln!(file, "{}", std::process::id())?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    thread::sleep(Duration::from_millis(LOCK_RETRY_DELAY_MS));
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "timed out waiting for baseline lock",
        ))
    }
}

impl Drop for BaselineLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed >= Duration::from_secs(STALE_LOCK_SECONDS))
}

#[cfg(test)]
mod tests {
    use super::{
        qdisc_equivalent, validate_interface_name, validate_kernel_token, BaselineSnapshot,
        InterfaceBaseline, BASELINE_VERSION,
    };
    use std::collections::BTreeMap;

    #[test]
    fn qdisc_noqueue_and_absent_are_equivalent() {
        assert!(qdisc_equivalent(None, None));
        assert!(qdisc_equivalent(None, Some("noqueue")));
        assert!(qdisc_equivalent(Some("noqueue"), None));
        assert!(qdisc_equivalent(Some("fq_codel"), Some("fq_codel")));
        assert!(!qdisc_equivalent(Some("fq"), Some("fq_codel")));
    }

    #[test]
    fn rejects_unsafe_kernel_identifiers() {
        assert!(validate_interface_name("rmnet_data0").is_ok());
        assert!(validate_interface_name("wlan0;reboot").is_err());
        assert!(validate_kernel_token("fq_codel").is_ok());
        assert!(validate_kernel_token("fq codel").is_err());
    }

    #[test]
    fn baseline_json_round_trip_preserves_values() {
        let snapshot = BaselineSnapshot {
            version: BASELINE_VERSION,
            captured_at_epoch: 123,
            sysctls: BTreeMap::from([(
                "/proc/sys/net/ipv4/tcp_rmem".to_string(),
                "4096 87380 6291456".to_string(),
            )]),
            interfaces: BTreeMap::from([(
                "wlan0".to_string(),
                InterfaceBaseline {
                    root_qdisc: Some("fq_codel".to_string()),
                },
            )]),
        };
        let json = serde_json::to_vec(&snapshot).unwrap();
        let decoded: BaselineSnapshot = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.version, BASELINE_VERSION);
        assert_eq!(decoded.sysctls, snapshot.sysctls);
        assert_eq!(decoded.interfaces.len(), 1);
    }
}
