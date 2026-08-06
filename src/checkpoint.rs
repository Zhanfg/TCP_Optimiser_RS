use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::control::{self, ControlState};
use crate::network::{self, IfaceMode};
use crate::{config, daemon, sysctl};

const CHECKPOINT_FILE: &str = "last-good-policy-v1.json";
const FAILURE_FILE: &str = "policy-failures-v1.json";
const FORMAT_VERSION: u32 = 1;
pub const AUTOMATIC_SAFE_MODE_THRESHOLD: u32 = 3;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreCheckpointReport {
    pub success: bool,
    pub control: ControlState,
    pub checkpoint: LastGoodPolicy,
    pub errors: Vec<String>,
}

pub fn status() -> io::Result<CheckpointStatus> {
    let checkpoint = read_checkpoint()?;
    let failures = read_failures().unwrap_or_else(|_| FailureState::default());
    Ok(CheckpointStatus {
        available: checkpoint.is_some(),
        checkpoint,
        consecutive_failures: failures.consecutive_failures,
        automatic_safe_mode_threshold: AUTOMATIC_SAFE_MODE_THRESHOLD,
    })
}

pub fn persist(
    iface: &str,
    mode: IfaceMode,
    policy: &daemon::ResolvedPolicy,
) -> io::Result<LastGoodPolicy> {
    validate_iface_name(iface)?;
    let checkpoint = LastGoodPolicy {
        format_version: FORMAT_VERSION,
        captured_at_epoch: now_epoch(),
        interface: iface.to_string(),
        interface_mode: mode.as_str().to_string(),
        algorithm: policy.algorithm.clone(),
        qdisc: policy.qdisc.clone(),
        pacing_ca: policy.pacing_ca,
        pacing_ss: policy.pacing_ss,
    };
    write_json_atomic(&checkpoint_path(), &checkpoint)?;
    clear_failures()?;
    Ok(checkpoint)
}

pub fn record_failure(message: impl Into<String>) -> io::Result<FailureState> {
    let mut state = read_failures().unwrap_or_else(|_| FailureState::default());
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
    let control = control::enter_automatic_safe_mode("restoring-last-known-good-policy")?;
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
    match sysctl::algo_available(&checkpoint.algorithm) {
        Ok(true) => {
            if let Err(error) = sysctl::set_congestion_control(&checkpoint.algorithm) {
                errors.push(format!("algorithm restore failed: {error}"));
            }
        }
        Ok(false) => errors.push(format!(
            "checkpoint algorithm {} is unavailable",
            checkpoint.algorithm
        )),
        Err(error) => errors.push(format!("algorithm capability check failed: {error}")),
    }

    Ok(RestoreCheckpointReport {
        success: errors.is_empty(),
        control,
        checkpoint,
        errors,
    })
}

fn checkpoint_path() -> PathBuf {
    config::module_dir().join(CHECKPOINT_FILE)
}

fn failure_path() -> PathBuf {
    config::module_dir().join(FAILURE_FILE)
}

fn read_checkpoint() -> io::Result<Option<LastGoodPolicy>> {
    read_optional_json(&checkpoint_path())
}

fn read_failures() -> io::Result<FailureState> {
    Ok(read_optional_json(&failure_path())?.unwrap_or_default())
}

fn validate_checkpoint(checkpoint: &LastGoodPolicy) -> io::Result<()> {
    if checkpoint.format_version != FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported checkpoint format {}",
                checkpoint.format_version
            ),
        ));
    }
    validate_iface_name(&checkpoint.interface)?;
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
    if !(1..=1000).contains(&checkpoint.pacing_ca)
        || !(1..=1000).contains(&checkpoint.pacing_ss)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "checkpoint pacing values are out of range",
        ));
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
    let mut clean = message.replace(['\0', '\r', '\n'], " ");
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
            format_version: FORMAT_VERSION,
            consecutive_failures: 0,
            updated_at_epoch: 0,
            last_error: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_checkpoint, validate_iface_name, LastGoodPolicy};

    #[test]
    fn interface_name_validation_rejects_paths_and_shell_text() {
        assert!(validate_iface_name("wlan0").is_ok());
        assert!(validate_iface_name("rmnet_data0").is_ok());
        assert!(validate_iface_name("../../proc").is_err());
        assert!(validate_iface_name("wlan0;reboot").is_err());
    }

    #[test]
    fn checkpoint_validation_rejects_unknown_kernel_tokens() {
        let mut checkpoint = LastGoodPolicy {
            format_version: 1,
            captured_at_epoch: 1,
            interface: "wlan0".to_string(),
            interface_mode: "Wi-Fi".to_string(),
            algorithm: "bbr".to_string(),
            qdisc: "fq".to_string(),
            pacing_ca: 200,
            pacing_ss: 300,
        };
        assert!(validate_checkpoint(&checkpoint).is_ok());
        checkpoint.algorithm = "bbr;reboot".to_string();
        assert!(validate_checkpoint(&checkpoint).is_err());
    }
}
