use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::control::{self, ControlState};
use crate::network::{self, IfaceMode};
use crate::{config, daemon, sysctl};

const CHECKPOINT_FILE: &str = "last-good-policy-v2.json";
const FAILURE_FILE: &str = "policy-failures-v1.json";
const CHECKPOINT_FORMAT_VERSION: u32 = 2;
const FAILURE_FORMAT_VERSION: u32 = 1;
const DAEMON_QUIESCE_SECONDS: u64 = 6;
pub const AUTOMATIC_SAFE_MODE_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CheckpointSysctl {
    pub key: String,
    pub path: String,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LastGoodPolicy {
    pub format_version: u32,
    pub captured_at_epoch: u64,
    pub interface: String,
    pub interface_mode: String,
    pub algorithm: String,
    pub qdisc: String,
    pub pacing_ca: u32,
    pub pacing_ss: u32,
    pub advanced_sysctls: Vec<CheckpointSysctl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FailureState {
    pub format_version: u32,
    pub consecutive_failures: u32,
    pub updated_at_epoch: u64,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckpointStatus {
    pub available: bool,
    pub checkpoint: Option<LastGoodPolicy>,
    pub consecutive_failures: u32,
    pub automatic_safe_mode_threshold: u32,
    pub failure_state_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreCheckpointReport {
    pub success: bool,
    pub rollback_attempted: bool,
    pub rollback_succeeded: bool,
    pub control: ControlState,
    pub checkpoint: LastGoodPolicy,
    pub errors: Vec<String>,
    pub rollback_errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeSysctlValue {
    key: String,
    path: String,
    value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeKernelState {
    algorithm: String,
    default_qdisc: String,
    interface_qdisc: String,
    pacing_ca: u32,
    pacing_ss: u32,
    advanced_sysctls: Vec<RuntimeSysctlValue>,
}

pub fn status() -> io::Result<CheckpointStatus> {
    let checkpoint = read_checkpoint()?;
    let (consecutive_failures, failure_state_error) = match read_failures() {
        Ok(state) => (state.consecutive_failures, None),
        Err(error) => (0, Some(error.to_string())),
    };
    Ok(CheckpointStatus {
        available: checkpoint.is_some(),
        checkpoint,
        consecutive_failures,
        automatic_safe_mode_threshold: AUTOMATIC_SAFE_MODE_THRESHOLD,
        failure_state_error,
    })
}

pub fn persist(
    iface: &str,
    mode: IfaceMode,
    policy: &daemon::ResolvedPolicy,
) -> io::Result<LastGoodPolicy> {
    validate_iface_name(iface)?;
    let (advanced, parse_errors) = sysctl::configured_advanced_overrides();
    if !parse_errors.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cannot checkpoint invalid advanced.conf: {}",
                parse_errors.join("; ")
            ),
        ));
    }
    let checkpoint = LastGoodPolicy {
        format_version: CHECKPOINT_FORMAT_VERSION,
        captured_at_epoch: now_epoch(),
        interface: iface.to_string(),
        interface_mode: mode.as_str().to_string(),
        algorithm: policy.algorithm.clone(),
        qdisc: policy.qdisc.clone(),
        pacing_ca: policy.pacing_ca,
        pacing_ss: policy.pacing_ss,
        advanced_sysctls: advanced
            .into_iter()
            .map(|item| CheckpointSysctl {
                key: item.key.to_string(),
                path: item.path.to_string(),
                value: item.value,
            })
            .collect(),
    };
    validate_checkpoint(&checkpoint)?;
    write_json_atomic(&checkpoint_path(), &checkpoint)?;
    clear_failures()?;
    Ok(checkpoint)
}

pub fn record_failure(message: impl Into<String>) -> io::Result<FailureState> {
    let mut state = read_failures()?;
    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
    state.updated_at_epoch = now_epoch();
    state.last_error = sanitize_message(message.into());
    write_json_atomic(&failure_path(), &state)?;
    Ok(state)
}

pub fn clear_failures() -> io::Result<()> {
    match fs::remove_file(failure_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn restore() -> io::Result<RestoreCheckpointReport> {
    let checkpoint = read_checkpoint()?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "last-known-good policy checkpoint is unavailable",
        )
    })?;
    validate_checkpoint(&checkpoint)?;
    preflight_checkpoint(&checkpoint)?;

    let control = control::enter_automatic_safe_mode("restoring-last-known-good-policy")?;
    if daemon::is_running() {
        thread::sleep(Duration::from_secs(DAEMON_QUIESCE_SECONDS));
    }

    let previous = capture_runtime_state(&checkpoint)?;
    let mut errors = apply_checkpoint(&checkpoint);
    if errors.is_empty() {
        errors.extend(verify_checkpoint_applied(&checkpoint));
    }

    let rollback_attempted = !errors.is_empty();
    let mut rollback_errors = Vec::new();
    if rollback_attempted {
        rollback_errors.extend(restore_runtime_state(&checkpoint.interface, &previous));
        if rollback_errors.is_empty() {
            rollback_errors.extend(verify_runtime_state(&checkpoint.interface, &previous));
        }
    }
    let rollback_succeeded = rollback_attempted && rollback_errors.is_empty();

    Ok(RestoreCheckpointReport {
        success: errors.is_empty(),
        rollback_attempted,
        rollback_succeeded,
        control,
        checkpoint,
        errors,
        rollback_errors,
    })
}

fn preflight_checkpoint(checkpoint: &LastGoodPolicy) -> io::Result<()> {
    if !sysctl::algo_available(&checkpoint.algorithm)? {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "checkpoint algorithm {} is unavailable",
                checkpoint.algorithm
            ),
        ));
    }
    let current_mode = network::iface_mode(&checkpoint.interface);
    if current_mode.as_str() != checkpoint.interface_mode {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "checkpoint interface mode changed: expected {}, found {}",
                checkpoint.interface_mode,
                current_mode.as_str()
            ),
        ));
    }
    let current_qdisc = network::root_qdisc(&checkpoint.interface)?;
    if current_qdisc.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "interface {} has no readable root qdisc; transactional rollback cannot be guaranteed",
                checkpoint.interface
            ),
        ));
    }
    for item in &checkpoint.advanced_sysctls {
        read_u32(&item.path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("checkpoint sysctl {} is unavailable: {error}", item.key),
            )
        })?;
    }
    Ok(())
}

fn capture_runtime_state(checkpoint: &LastGoodPolicy) -> io::Result<RuntimeKernelState> {
    let interface_qdisc = network::root_qdisc(&checkpoint.interface)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "interface {} root qdisc is unavailable",
                checkpoint.interface
            ),
        )
    })?;
    let mut advanced_sysctls = Vec::with_capacity(checkpoint.advanced_sysctls.len());
    for item in &checkpoint.advanced_sysctls {
        advanced_sysctls.push(RuntimeSysctlValue {
            key: item.key.clone(),
            path: item.path.clone(),
            value: read_u32(&item.path)?,
        });
    }
    Ok(RuntimeKernelState {
        algorithm: sysctl::current_algorithm()?,
        default_qdisc: sysctl::default_qdisc()?,
        interface_qdisc,
        pacing_ca: read_pacing("/proc/sys/net/ipv4/tcp_pacing_ca_ratio")?,
        pacing_ss: read_pacing("/proc/sys/net/ipv4/tcp_pacing_ss_ratio")?,
        advanced_sysctls,
    })
}

fn apply_checkpoint(checkpoint: &LastGoodPolicy) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(error) = sysctl::set_pacing(checkpoint.pacing_ca, checkpoint.pacing_ss) {
        errors.push(format!("pacing restore failed: {error}"));
    }
    if let Err(error) = sysctl::set_default_qdisc(&checkpoint.qdisc) {
        errors.push(format!("default qdisc restore failed: {error}"));
    }
    if let Err(error) = network::set_qdisc(&checkpoint.interface, &checkpoint.qdisc) {
        errors.push(format!("interface qdisc restore failed: {error}"));
    }
    if let Err(error) = sysctl::set_congestion_control(&checkpoint.algorithm) {
        errors.push(format!("algorithm restore failed: {error}"));
    }
    for item in &checkpoint.advanced_sysctls {
        if let Err(error) = sysctl::write_sysctl(&item.path, &item.value.to_string()) {
            errors.push(format!("advanced sysctl {} restore failed: {error}", item.key));
        }
    }
    errors
}

fn verify_checkpoint_applied(checkpoint: &LastGoodPolicy) -> Vec<String> {
    let expected = RuntimeKernelState {
        algorithm: checkpoint.algorithm.clone(),
        default_qdisc: checkpoint.qdisc.clone(),
        interface_qdisc: checkpoint.qdisc.clone(),
        pacing_ca: checkpoint.pacing_ca,
        pacing_ss: checkpoint.pacing_ss,
        advanced_sysctls: checkpoint
            .advanced_sysctls
            .iter()
            .map(|item| RuntimeSysctlValue {
                key: item.key.clone(),
                path: item.path.clone(),
                value: item.value,
            })
            .collect(),
    };
    verify_runtime_state(&checkpoint.interface, &expected)
        .into_iter()
        .map(|error| format!("checkpoint {error}"))
        .collect()
}

fn restore_runtime_state(iface: &str, previous: &RuntimeKernelState) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(error) = sysctl::set_pacing(previous.pacing_ca, previous.pacing_ss) {
        errors.push(format!("rollback pacing failed: {error}"));
    }
    if let Err(error) = sysctl::set_default_qdisc(&previous.default_qdisc) {
        errors.push(format!("rollback default qdisc failed: {error}"));
    }
    if let Err(error) = network::set_qdisc(iface, &previous.interface_qdisc) {
        errors.push(format!("rollback interface qdisc failed: {error}"));
    }
    if let Err(error) = sysctl::set_congestion_control(&previous.algorithm) {
        errors.push(format!("rollback algorithm failed: {error}"));
    }
    for item in &previous.advanced_sysctls {
        if let Err(error) = sysctl::write_sysctl(&item.path, &item.value.to_string()) {
            errors.push(format!("rollback advanced sysctl {} failed: {error}", item.key));
        }
    }
    errors
}

fn verify_runtime_state(iface: &str, expected: &RuntimeKernelState) -> Vec<String> {
    let mut errors = Vec::new();
    compare_value(
        &mut errors,
        "algorithm",
        sysctl::current_algorithm().ok(),
        expected.algorithm.clone(),
    );
    compare_value(
        &mut errors,
        "default qdisc",
        sysctl::default_qdisc().ok(),
        expected.default_qdisc.clone(),
    );
    compare_value(
        &mut errors,
        "interface qdisc",
        network::root_qdisc(iface).ok().flatten(),
        expected.interface_qdisc.clone(),
    );
    compare_value(
        &mut errors,
        "pacing CA",
        read_pacing("/proc/sys/net/ipv4/tcp_pacing_ca_ratio")
            .ok()
            .map(|value| value.to_string()),
        expected.pacing_ca.to_string(),
    );
    compare_value(
        &mut errors,
        "pacing SS",
        read_pacing("/proc/sys/net/ipv4/tcp_pacing_ss_ratio")
            .ok()
            .map(|value| value.to_string()),
        expected.pacing_ss.to_string(),
    );
    for item in &expected.advanced_sysctls {
        compare_value(
            &mut errors,
            &format!("advanced sysctl {}", item.key),
            read_u32(&item.path).ok().map(|value| value.to_string()),
            item.value.to_string(),
        );
    }
    errors
}

fn compare_value(errors: &mut Vec<String>, name: &str, actual: Option<String>, expected: String) {
    match actual {
        Some(value) if value == expected => {}
        Some(value) => errors.push(format!(
            "{name} verification failed: expected {expected}, found {value}"
        )),
        None => errors.push(format!("{name} verification is unavailable")),
    }
}

fn read_pacing(path: &str) -> io::Result<u32> {
    read_u32(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("invalid pacing value at {path}: {error}"),
        )
    })
}

fn read_u32(path: &str) -> io::Result<u32> {
    let value = sysctl::read_sysctl(path)?;
    value.parse::<u32>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid numeric sysctl value at {path}: {error}"),
        )
    })
}

fn checkpoint_path() -> PathBuf {
    config::module_dir().join(CHECKPOINT_FILE)
}

fn failure_path() -> PathBuf {
    config::module_dir().join(FAILURE_FILE)
}

fn read_checkpoint() -> io::Result<Option<LastGoodPolicy>> {
    let checkpoint = read_optional_json(&checkpoint_path())?;
    if let Some(value) = checkpoint.as_ref() {
        validate_checkpoint(value)?;
    }
    Ok(checkpoint)
}

fn read_failures() -> io::Result<FailureState> {
    let bytes = match fs::read(failure_path()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FailureState::default()),
        Err(error) => return Err(error),
    };
    decode_failure_state(&bytes)
}

fn decode_failure_state(bytes: &[u8]) -> io::Result<FailureState> {
    let state: FailureState = serde_json::from_slice(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid policy failure state: {error}"),
        )
    })?;
    if state.format_version != FAILURE_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported policy failure format {}", state.format_version),
        ));
    }
    Ok(state)
}

fn validate_checkpoint(checkpoint: &LastGoodPolicy) -> io::Result<()> {
    if checkpoint.format_version != CHECKPOINT_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported checkpoint format {}",
                checkpoint.format_version
            ),
        ));
    }
    validate_iface_name(&checkpoint.interface)?;
    if !matches!(checkpoint.interface_mode.as_str(), "Wi-Fi" | "Cellular") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid checkpoint interface mode {}", checkpoint.interface_mode),
        ));
    }
    if !config::is_known_algorithm(&checkpoint.algorithm) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown checkpoint algorithm {}", checkpoint.algorithm),
        ));
    }
    if !config::is_known_qdisc(&checkpoint.qdisc) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown checkpoint qdisc {}", checkpoint.qdisc),
        ));
    }
    if !(1..=1000).contains(&checkpoint.pacing_ca) || !(1..=1000).contains(&checkpoint.pacing_ss) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "checkpoint pacing values are out of range",
        ));
    }

    let (configured, parse_errors) = sysctl::configured_advanced_overrides();
    if !parse_errors.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("advanced.conf is invalid: {}", parse_errors.join("; ")),
        ));
    }
    if checkpoint.advanced_sysctls.len() != configured.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "checkpoint advanced sysctl set does not match the configured policy",
        ));
    }
    let mut seen = HashSet::new();
    for item in &checkpoint.advanced_sysctls {
        if !seen.insert(item.key.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate checkpoint sysctl {}", item.key),
            ));
        }
        let Some(configured_item) = configured.iter().find(|candidate| candidate.key == item.key)
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown checkpoint sysctl {}", item.key),
            ));
        };
        if configured_item.path != item.path || configured_item.value != item.value {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("checkpoint sysctl {} does not match advanced.conf", item.key),
            ));
        }
    }
    Ok(())
}

fn validate_iface_name(iface: &str) -> io::Result<()> {
    if iface.is_empty()
        || iface.len() > 32
        || !iface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid interface name {iface:?}"),
        ));
    }
    Ok(())
}

fn read_optional_json<T>(path: &Path) -> io::Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {}: {error}", path.display()),
        )
    })
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

fn sanitize_message(message: String) -> String {
    let mut clean = message
        .chars()
        .map(|character| {
            if matches!(character, '\0' | '\r' | '\n') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    clean.truncate(320);
    clean.trim().to_string()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Default for FailureState {
    fn default() -> Self {
        Self {
            format_version: FAILURE_FORMAT_VERSION,
            consecutive_failures: 0,
            updated_at_epoch: 0,
            last_error: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compare_value, decode_failure_state, validate_checkpoint, validate_iface_name,
        LastGoodPolicy,
    };

    fn checkpoint() -> LastGoodPolicy {
        LastGoodPolicy {
            format_version: 2,
            captured_at_epoch: 1,
            interface: "wlan0".to_string(),
            interface_mode: "Wi-Fi".to_string(),
            algorithm: "bbr".to_string(),
            qdisc: "fq".to_string(),
            pacing_ca: 200,
            pacing_ss: 300,
            advanced_sysctls: Vec::new(),
        }
    }

    #[test]
    fn interface_name_validation_rejects_paths_and_shell_text() {
        assert!(validate_iface_name("wlan0").is_ok());
        assert!(validate_iface_name("rmnet_data0").is_ok());
        assert!(validate_iface_name("../../proc").is_err());
        assert!(validate_iface_name("wlan0;reboot").is_err());
    }

    #[test]
    fn checkpoint_validation_rejects_unknown_kernel_tokens() {
        let mut checkpoint = checkpoint();
        assert!(validate_checkpoint(&checkpoint).is_ok());
        checkpoint.algorithm = "bbr;reboot".to_string();
        assert!(validate_checkpoint(&checkpoint).is_err());
    }

    #[test]
    fn corrupted_failure_state_is_not_reset() {
        assert!(decode_failure_state(br#"{"format_version":1,"consecutive_failures":"bad"}"#)
            .is_err());
        assert!(decode_failure_state(br#"{"format_version":9,"consecutive_failures":1,"updated_at_epoch":1,"last_error":"x"}"#)
            .is_err());
    }

    #[test]
    fn readback_comparison_reports_drift_and_unavailable_values() {
        let mut errors = Vec::new();
        compare_value(&mut errors, "algo", Some("cubic".into()), "bbr".into());
        compare_value(&mut errors, "qdisc", None, "fq".into());
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("expected bbr"));
        assert!(errors[1].contains("unavailable"));
    }
}
