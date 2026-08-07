use serde::Serialize;
use std::io;

use crate::control::{self, RuntimeMode};
use crate::network::{self, IfaceMode};
use crate::{daemon, sysctl};

#[derive(Debug, Clone, Serialize)]
pub struct PlanValue {
    pub current: Option<String>,
    pub planned: Option<String>,
    pub changed: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyPlan {
    pub interface: String,
    pub interface_mode: String,
    pub interface_mtu: Option<u32>,
    pub wifi_frequency_mhz: Option<u32>,
    pub runtime_mode: RuntimeMode,
    pub write_allowed: bool,
    pub algorithm: PlanValue,
    pub default_qdisc: PlanValue,
    pub interface_qdisc: PlanValue,
    pub pacing_ca: PlanValue,
    pub pacing_ss: PlanValue,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyDiff {
    pub interface: String,
    pub interface_mode: String,
    pub runtime_mode: RuntimeMode,
    pub write_allowed: bool,
    pub changes: Vec<PlanChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanChange {
    pub key: String,
    pub current: Option<String>,
    pub planned: Option<String>,
    pub available: bool,
}

pub fn build(iface: Option<String>) -> io::Result<PolicyPlan> {
    let interface = iface.map(Ok).unwrap_or_else(network::active_iface)?;
    let mode = network::iface_mode(&interface);
    let mut warnings = Vec::new();
    let control_state = match control::read() {
        Ok(state) => state,
        Err(error) => {
            warnings.push(format!(
                "Runtime control state is invalid; writes are treated as disabled: {error}"
            ));
            control::ControlState::safe_fallback("invalid-control-state")
        }
    };

    if mode == IfaceMode::Unknown {
        warnings.push(format!(
            "Interface {interface} is not recognized as Wi-Fi or cellular"
        ));
    }
    if !control_state.mode.allows_writes() {
        warnings.push(format!(
            "Runtime mode {} prevents policy writes",
            control_state.mode.as_str()
        ));
    }

    let planned = match daemon::resolve_policy(&interface, mode) {
        Ok(policy) => Some(policy),
        Err(error) => {
            warnings.push(format!("Configured policy cannot be resolved: {error}"));
            None
        }
    };
    let policy_resolved = planned.is_some();

    let current_algorithm = sysctl::current_algorithm().ok();
    let current_default_qdisc = sysctl::default_qdisc().ok();
    let current_interface_qdisc = network::root_qdisc(&interface).ok().flatten();
    let current_pacing_ca = sysctl::read_sysctl("/proc/sys/net/ipv4/tcp_pacing_ca_ratio").ok();
    let current_pacing_ss = sysctl::read_sysctl("/proc/sys/net/ipv4/tcp_pacing_ss_ratio").ok();

    let algorithm = value(
        current_algorithm,
        planned.as_ref().map(|policy| policy.algorithm.clone()),
    );
    let default_qdisc = value(
        current_default_qdisc,
        planned.as_ref().map(|policy| policy.qdisc.clone()),
    );
    let interface_qdisc = value(
        current_interface_qdisc,
        planned.as_ref().map(|policy| policy.qdisc.clone()),
    );
    let pacing_ca = value(
        current_pacing_ca,
        planned.as_ref().map(|policy| policy.pacing_ca.to_string()),
    );
    let pacing_ss = value(
        current_pacing_ss,
        planned.as_ref().map(|policy| policy.pacing_ss.to_string()),
    );

    let mut changes = Vec::new();
    push_change(&mut changes, "algorithm", &algorithm);
    push_change(&mut changes, "default_qdisc", &default_qdisc);
    push_change(&mut changes, "interface_qdisc", &interface_qdisc);
    push_change(&mut changes, "pacing_ca", &pacing_ca);
    push_change(&mut changes, "pacing_ss", &pacing_ss);

    Ok(PolicyPlan {
        interface: interface.clone(),
        interface_mode: mode.as_str().to_string(),
        interface_mtu: network::iface_mtu(&interface),
        wifi_frequency_mhz: planned
            .as_ref()
            .and_then(|policy| policy.wifi_frequency_mhz),
        runtime_mode: control_state.mode,
        write_allowed: control_state.mode.allows_writes()
            && mode != IfaceMode::Unknown
            && policy_resolved,
        algorithm,
        default_qdisc,
        interface_qdisc,
        pacing_ca,
        pacing_ss,
        changes,
        warnings,
    })
}

pub fn diff(iface: Option<String>) -> io::Result<PolicyDiff> {
    let plan = build(iface)?;
    let mut changes = Vec::new();
    collect_change(&mut changes, "algorithm", &plan.algorithm);
    collect_change(&mut changes, "default_qdisc", &plan.default_qdisc);
    collect_change(&mut changes, "interface_qdisc", &plan.interface_qdisc);
    collect_change(&mut changes, "pacing_ca", &plan.pacing_ca);
    collect_change(&mut changes, "pacing_ss", &plan.pacing_ss);
    Ok(PolicyDiff {
        interface: plan.interface,
        interface_mode: plan.interface_mode,
        runtime_mode: plan.runtime_mode,
        write_allowed: plan.write_allowed,
        changes,
        warnings: plan.warnings,
    })
}

fn value(current: Option<String>, planned: Option<String>) -> PlanValue {
    let available = current.is_some();
    let changed = match (&current, &planned) {
        (Some(current), Some(planned)) => current != planned,
        (None, Some(_)) => true,
        _ => false,
    };
    PlanValue {
        current,
        planned,
        changed,
        available,
    }
}

fn push_change(target: &mut Vec<String>, key: &str, value: &PlanValue) {
    if value.changed {
        target.push(key.to_string());
    }
}

fn collect_change(target: &mut Vec<PlanChange>, key: &str, value: &PlanValue) {
    if !value.changed {
        return;
    }
    target.push(PlanChange {
        key: key.to_string(),
        current: value.current.clone(),
        planned: value.planned.clone(),
        available: value.available,
    });
}

#[cfg(test)]
mod tests {
    use super::value;

    #[test]
    fn plan_value_marks_only_real_differences() {
        assert!(!value(Some("bbr".into()), Some("bbr".into())).changed);
        assert!(value(Some("cubic".into()), Some("bbr".into())).changed);
        assert!(value(None, Some("bbr".into())).changed);
        assert!(!value(Some("bbr".into()), None).changed);
    }
}
