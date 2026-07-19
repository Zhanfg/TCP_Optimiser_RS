use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::network::{self, IfaceMode};
use crate::{config, daemon, sysctl};

const LAST_REPAIR_FILE: &str = "last_repair.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Match,
    Drift,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyCheck {
    pub key: String,
    pub expected: String,
    pub actual: Option<String>,
    pub state: CheckState,
    pub repairable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VerificationSummary {
    pub matched: usize,
    pub total: usize,
    pub drifted: usize,
    pub unavailable: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationSnapshot {
    pub summary: VerificationSummary,
    pub checks: Vec<PolicyCheck>,
    pub errors: Vec<String>,
    pub last_repair: Option<RepairRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepairRecord {
    pub timestamp_epoch: u64,
    pub success: bool,
    pub reason: String,
    pub errors: Vec<String>,
}

pub fn verify_policy(iface: &str) -> VerificationSnapshot {
    let mode = network::iface_mode(iface);
    let mut checks = vec![PolicyCheck {
        key: "interface_mode".to_string(),
        expected: "Wi-Fi or Cellular".to_string(),
        actual: Some(mode.as_str().to_string()),
        state: if mode == IfaceMode::Unknown {
            CheckState::Unavailable
        } else {
            CheckState::Match
        },
        repairable: false,
    }];
    let mut errors = Vec::new();

    match daemon::resolve_policy(iface, mode) {
        Ok(expected) => {
            checks.push(check_value(
                "congestion_algorithm",
                expected.algorithm,
                sysctl::current_algorithm().ok(),
                true,
            ));
            checks.push(check_value(
                "default_qdisc",
                expected.qdisc.clone(),
                sysctl::default_qdisc().ok(),
                true,
            ));
            checks.push(check_value(
                "interface_qdisc",
                expected.qdisc,
                network::root_qdisc(iface).ok().flatten(),
                true,
            ));
            checks.push(check_value(
                "tcp_pacing_ca_ratio",
                expected.pacing_ca.to_string(),
                sysctl::read_sysctl("/proc/sys/net/ipv4/tcp_pacing_ca_ratio").ok(),
                true,
            ));
            checks.push(check_value(
                "tcp_pacing_ss_ratio",
                expected.pacing_ss.to_string(),
                sysctl::read_sysctl("/proc/sys/net/ipv4/tcp_pacing_ss_ratio").ok(),
                true,
            ));
        }
        Err(error) => {
            errors.push(format!("Cannot resolve configured policy: {error}"));
            checks.push(unavailable_check("configured_policy", true));
        }
    }

    let (advanced, advanced_errors) = sysctl::configured_advanced_overrides();
    errors.extend(advanced_errors);
    checks.extend(advanced.into_iter().map(|item| {
        check_value(
            &format!("advanced.{}", item.key),
            item.value.to_string(),
            sysctl::read_sysctl(item.path).ok(),
            true,
        )
    }));

    VerificationSnapshot {
        summary: summarize(&checks),
        checks,
        errors,
        last_repair: last_repair_record(),
    }
}

pub fn repair_policy(iface: &str) -> io::Result<RepairRecord> {
    let mode = network::iface_mode(iface);
    let mut errors = sysctl::apply_base_sysctls();

    if mode == IfaceMode::Unknown {
        errors.push(format!("Unsupported active interface: {iface}"));
    } else {
        match daemon::repair_interface_settings(iface, mode) {
            Ok(failures) => errors.extend(failures),
            Err(error) => errors.push(format!("Cannot resolve configured policy: {error}")),
        }
    }

    let verification = verify_policy(iface);
    if verification.summary.drifted > 0 {
        errors.push(format!(
            "{} configured value(s) still differ after repair",
            verification.summary.drifted
        ));
    }
    if verification.summary.unavailable > 0 {
        errors.push(format!(
            "{} verification value(s) are unavailable",
            verification.summary.unavailable
        ));
    }
    errors.extend(verification.errors);
    errors.sort();
    errors.dedup();

    let record = RepairRecord {
        timestamp_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        success: errors.is_empty(),
        reason: "manual".to_string(),
        errors,
    };
    persist_repair_record(&record)?;
    Ok(record)
}

fn check_value(
    key: &str,
    expected: String,
    actual: Option<String>,
    repairable: bool,
) -> PolicyCheck {
    let state = match actual.as_deref() {
        Some(value) if value == expected => CheckState::Match,
        Some(_) => CheckState::Drift,
        None => CheckState::Unavailable,
    };
    PolicyCheck {
        key: key.to_string(),
        expected,
        actual,
        state,
        repairable,
    }
}

fn unavailable_check(key: &str, repairable: bool) -> PolicyCheck {
    PolicyCheck {
        key: key.to_string(),
        expected: "configured".to_string(),
        actual: None,
        state: CheckState::Unavailable,
        repairable,
    }
}

fn summarize(checks: &[PolicyCheck]) -> VerificationSummary {
    VerificationSummary {
        matched: checks
            .iter()
            .filter(|check| check.state == CheckState::Match)
            .count(),
        total: checks.len(),
        drifted: checks
            .iter()
            .filter(|check| check.state == CheckState::Drift)
            .count(),
        unavailable: checks
            .iter()
            .filter(|check| check.state == CheckState::Unavailable)
            .count(),
    }
}

fn last_repair_record() -> Option<RepairRecord> {
    let content = fs::read_to_string(config::module_dir().join(LAST_REPAIR_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}

fn persist_repair_record(record: &RepairRecord) -> io::Result<()> {
    let module_dir = config::module_dir();
    fs::create_dir_all(&module_dir)?;
    let destination = module_dir.join(LAST_REPAIR_FILE);
    let temporary = module_dir.join(format!("{LAST_REPAIR_FILE}.tmp"));
    let json = serde_json::to_vec(record).map_err(io::Error::other)?;
    fs::write(&temporary, json)?;
    fs::rename(temporary, destination)
}

#[cfg(test)]
mod tests {
    use super::{check_value, summarize, CheckState, PolicyCheck};

    #[test]
    fn distinguishes_match_drift_and_unavailable() {
        assert_eq!(
            check_value("algo", "bbr".to_string(), Some("bbr".to_string()), true).state,
            CheckState::Match
        );
        assert_eq!(
            check_value("algo", "bbr".to_string(), Some("cubic".to_string()), true).state,
            CheckState::Drift
        );
        assert_eq!(
            check_value("algo", "bbr".to_string(), None, true).state,
            CheckState::Unavailable
        );
    }

    #[test]
    fn summarizes_check_states() {
        let checks = [
            PolicyCheck {
                key: "one".to_string(),
                expected: "1".to_string(),
                actual: Some("1".to_string()),
                state: CheckState::Match,
                repairable: true,
            },
            PolicyCheck {
                key: "two".to_string(),
                expected: "2".to_string(),
                actual: Some("3".to_string()),
                state: CheckState::Drift,
                repairable: true,
            },
            PolicyCheck {
                key: "three".to_string(),
                expected: "3".to_string(),
                actual: None,
                state: CheckState::Unavailable,
                repairable: false,
            },
        ];
        let summary = summarize(&checks);
        assert_eq!(summary.matched, 1);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.drifted, 1);
        assert_eq!(summary.unavailable, 1);
    }
}
