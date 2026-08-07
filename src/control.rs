use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

const CONTROL_FILE: &str = "runtime-control-v1.json";
const ACK_FILE: &str = "runtime-control-ack-v1.json";
const RESTORE_LOCK_FILE: &str = "runtime-restore-v1.lock";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Active,
    Paused,
    SafeMode,
}

impl RuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::SafeMode => "safe_mode",
        }
    }

    pub fn allows_writes(self) -> bool {
        self == Self::Active
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAction {
    Initial,
    Reload,
    Pause,
    Resume,
    EnterSafeMode,
    LeaveSafeMode,
    AutomaticSafeMode,
}

impl RuntimeAction {
    pub fn requests_apply(self) -> bool {
        matches!(self, Self::Reload | Self::Resume | Self::LeaveSafeMode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ControlState {
    pub format_version: u32,
    pub generation: u64,
    pub mode: RuntimeMode,
    pub requested_action: RuntimeAction,
    pub updated_at_epoch: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ControlAck {
    pub format_version: u32,
    pub generation: u64,
    pub mode: RuntimeMode,
    pub daemon_pid: u32,
    pub acknowledged_at_epoch: u64,
}

pub struct RestoreGuard {
    path: PathBuf,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            generation: 0,
            mode: RuntimeMode::Active,
            requested_action: RuntimeAction::Initial,
            updated_at_epoch: now_epoch(),
            reason: "default-active".to_string(),
        }
    }
}

impl ControlState {
    pub fn safe_fallback(reason: impl Into<String>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            generation: u64::MAX,
            mode: RuntimeMode::SafeMode,
            requested_action: RuntimeAction::AutomaticSafeMode,
            updated_at_epoch: now_epoch(),
            reason: reason.into(),
        }
    }
}

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn path() -> PathBuf {
    config::module_dir().join(CONTROL_FILE)
}

fn ack_path() -> PathBuf {
    config::module_dir().join(ACK_FILE)
}

fn restore_lock_path() -> PathBuf {
    config::module_dir().join(RESTORE_LOCK_FILE)
}

pub fn read() -> io::Result<ControlState> {
    read_from(&path())
}

pub fn read_ack() -> io::Result<ControlAck> {
    let content = fs::read(ack_path())?;
    let ack: ControlAck = serde_json::from_slice(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid runtime control acknowledgement: {error}"),
        )
    })?;
    if ack.format_version != FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported runtime control acknowledgement format {}",
                ack.format_version
            ),
        ));
    }
    Ok(ack)
}

pub fn acknowledge(state: &ControlState) -> io::Result<ControlAck> {
    let ack = ControlAck {
        format_version: FORMAT_VERSION,
        generation: state.generation,
        mode: state.mode,
        daemon_pid: std::process::id(),
        acknowledged_at_epoch: now_epoch(),
    };
    write_json_atomic(&ack_path(), &ack)?;
    Ok(ack)
}

pub fn ack_matches(ack: &ControlAck, state: &ControlState, daemon_pid: u32) -> bool {
    ack.format_version == FORMAT_VERSION
        && ack.generation == state.generation
        && ack.mode == state.mode
        && ack.daemon_pid == daemon_pid
}

pub fn state_matches(left: &ControlState, right: &ControlState) -> bool {
    left.format_version == right.format_version
        && left.generation == right.generation
        && left.mode == right.mode
}

pub fn restore_in_progress() -> bool {
    restore_lock_path().exists()
}

pub fn acquire_restore_guard() -> io::Result<RestoreGuard> {
    let path = restore_lock_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match create_restore_lock(&path) {
        Ok(()) => Ok(RestoreGuard { path }),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if restore_lock_owner_is_live(&path)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "another runtime restoration is already active",
                ));
            }
            fs::remove_file(&path)?;
            create_restore_lock(&path)?;
            Ok(RestoreGuard { path })
        }
        Err(error) => Err(error),
    }
}

fn create_restore_lock(path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file, "{}", std::process::id())?;
    file.sync_all()
}

fn restore_lock_owner_is_live(path: &Path) -> io::Result<bool> {
    let pid = fs::read_to_string(path)?
        .trim()
        .parse::<u32>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let command_line = match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(command_line) => command_line,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(command_line
        .split(|byte| *byte == 0)
        .any(|argument| argument == b"restore-checkpoint"))
}

pub fn request_reload(reason: impl Into<String>) -> io::Result<ControlState> {
    update(None, RuntimeAction::Reload, reason)
}

pub fn pause(reason: impl Into<String>) -> io::Result<ControlState> {
    update(Some(RuntimeMode::Paused), RuntimeAction::Pause, reason)
}

pub fn resume(reason: impl Into<String>) -> io::Result<ControlState> {
    update(Some(RuntimeMode::Active), RuntimeAction::Resume, reason)
}

pub fn set_safe_mode(enabled: bool, reason: impl Into<String>) -> io::Result<ControlState> {
    if enabled {
        update(
            Some(RuntimeMode::SafeMode),
            RuntimeAction::EnterSafeMode,
            reason,
        )
    } else {
        update(
            Some(RuntimeMode::Active),
            RuntimeAction::LeaveSafeMode,
            reason,
        )
    }
}

pub fn enter_automatic_safe_mode(reason: impl Into<String>) -> io::Result<ControlState> {
    update(
        Some(RuntimeMode::SafeMode),
        RuntimeAction::AutomaticSafeMode,
        reason,
    )
}

fn action_allowed_while_restoring(action: RuntimeAction) -> bool {
    action == RuntimeAction::AutomaticSafeMode
}

fn update(
    mode: Option<RuntimeMode>,
    action: RuntimeAction,
    reason: impl Into<String>,
) -> io::Result<ControlState> {
    if restore_in_progress() && !action_allowed_while_restoring(action) {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "runtime restoration is active; control changes are temporarily blocked",
        ));
    }
    let mut state = match read() {
        Ok(state) => state,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => recovery_state(&error),
        Err(error) if error.kind() == io::ErrorKind::NotFound => ControlState::default(),
        Err(error) => return Err(error),
    };
    state.generation = state.generation.saturating_add(1);
    if let Some(mode) = mode {
        state.mode = mode;
    }
    state.requested_action = action;
    state.updated_at_epoch = now_epoch();
    state.reason = sanitize_reason(reason.into());
    write_json_atomic(&path(), &state)?;
    Ok(state)
}

fn recovery_state(error: &io::Error) -> ControlState {
    ControlState {
        format_version: FORMAT_VERSION,
        generation: 0,
        mode: RuntimeMode::SafeMode,
        requested_action: RuntimeAction::AutomaticSafeMode,
        updated_at_epoch: now_epoch(),
        reason: sanitize_reason(format!("recovered-invalid-control: {error}")),
    }
}

fn read_from(path: &Path) -> io::Result<ControlState> {
    let content = match fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(ControlState::default()),
        Err(error) => return Err(error),
    };
    let state: ControlState = serde_json::from_slice(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid runtime control state: {error}"),
        )
    })?;
    if state.format_version != FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported runtime control format {}",
                state.format_version
            ),
        ));
    }
    Ok(state)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let payload = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    fs::write(&temporary, payload)?;
    fs::rename(&temporary, path)
}

fn sanitize_reason(reason: String) -> String {
    let mut clean = reason
        .chars()
        .map(|character| {
            if matches!(character, '\0' | '\r' | '\n') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    clean.truncate(240);
    if clean.trim().is_empty() {
        "unspecified".to_string()
    } else {
        clean.trim().to_string()
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        ack_matches, action_allowed_while_restoring, read_from, recovery_state, state_matches,
        write_json_atomic, ControlAck, ControlState, RuntimeAction, RuntimeMode,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("tcp-optimiser-{name}-{suffix}.json"))
    }

    fn safe_state(generation: u64) -> ControlState {
        ControlState {
            format_version: 1,
            generation,
            mode: RuntimeMode::SafeMode,
            requested_action: RuntimeAction::AutomaticSafeMode,
            updated_at_epoch: 123,
            reason: "test".to_string(),
        }
    }

    #[test]
    fn missing_control_state_defaults_to_active() {
        let path = temp_path("missing");
        let state = read_from(&path).unwrap();
        assert_eq!(state.mode, RuntimeMode::Active);
        assert_eq!(state.generation, 0);
    }

    #[test]
    fn atomic_round_trip_preserves_generation_and_action() {
        let path = temp_path("roundtrip");
        let state = ControlState {
            format_version: 1,
            generation: 7,
            mode: RuntimeMode::Paused,
            requested_action: RuntimeAction::Pause,
            updated_at_epoch: 123,
            reason: "test".to_string(),
        };
        write_json_atomic(&path, &state).unwrap();
        assert_eq!(read_from(&path).unwrap(), state);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn acknowledgement_must_match_generation_mode_and_pid() {
        let state = safe_state(7);
        let mut ack = ControlAck {
            format_version: 1,
            generation: 7,
            mode: RuntimeMode::SafeMode,
            daemon_pid: 42,
            acknowledged_at_epoch: 124,
        };
        assert!(ack_matches(&ack, &state, 42));
        ack.generation = 6;
        assert!(!ack_matches(&ack, &state, 42));
    }

    #[test]
    fn transaction_state_requires_the_same_generation_and_mode() {
        let state = safe_state(7);
        assert!(state_matches(&state, &safe_state(7)));
        assert!(!state_matches(&state, &safe_state(8)));
        let mut active = safe_state(7);
        active.mode = RuntimeMode::Active;
        assert!(!state_matches(&state, &active));
    }

    #[test]
    fn only_automatic_safe_mode_may_change_control_during_restore() {
        assert!(action_allowed_while_restoring(
            RuntimeAction::AutomaticSafeMode
        ));
        assert!(!action_allowed_while_restoring(RuntimeAction::Resume));
        assert!(!action_allowed_while_restoring(RuntimeAction::Reload));
    }

    #[test]
    fn rejects_unknown_control_format() {
        let path = temp_path("version");
        fs::write(
            &path,
            br#"{"format_version":9,"generation":1,"mode":"active","requested_action":"reload","updated_at_epoch":1,"reason":"test"}"#,
        )
        .unwrap();
        assert_eq!(
            read_from(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_state_recovery_starts_safe_with_reset_generation() {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidData, "broken json");
        let state = recovery_state(&error);
        assert_eq!(state.mode, RuntimeMode::SafeMode);
        assert_eq!(state.generation, 0);
        assert!(state.reason.contains("broken json"));
    }
}
