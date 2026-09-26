use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Configuration for a single TCP congestion control algorithm
#[derive(Debug, Clone)]
pub struct AlgoConfig {
    pub qdisc: &'static str,
    pub pacing_ca: u32,
    pub pacing_ss: u32,
    pub desc: &'static str,
}

/// Get the configuration for a specific algorithm.
/// Returns default CUBIC config for unknown algorithms.
pub fn get_algo_config(algo: &str) -> AlgoConfig {
    algos()
        .get(algo)
        .cloned()
        .unwrap_or_else(|| algos()["cubic"].clone())
}

fn algos() -> &'static HashMap<&'static str, AlgoConfig> {
    static ALGOS: OnceLock<HashMap<&'static str, AlgoConfig>> = OnceLock::new();
    ALGOS.get_or_init(build_algo_map)
}

fn build_algo_map() -> HashMap<&'static str, AlgoConfig> {
    let entries = [
        (
            "bbr",
            "fq",
            200,
            300,
            "BBRv1 - kernel native model-based congestion control",
        ),
        (
            "bbr3",
            "fq",
            220,
            320,
            "BBRv3 - bundled PJZ110 kernel module",
        ),
        (
            "cubic",
            "fq_codel",
            150,
            200,
            "CUBIC - kernel native default congestion control",
        ),
        (
            "reno",
            "fq_codel",
            150,
            200,
            "Reno - kernel native classic congestion control",
        ),
    ];

    let mut map = HashMap::new();
    for (name, qdisc, ca, ss, desc) in entries {
        map.insert(
            name,
            AlgoConfig {
                qdisc,
                pacing_ca: ca,
                pacing_ss: ss,
                desc,
            },
        );
    }
    map
}

/// Algorithms intentionally exposed by the PJZ110 build.
/// The WebUI still renders only the subset reported by the running kernel
/// plus a bundled algorithm that can actually be loaded.
pub const ALL_ALGOS: &[&str] = &["bbr", "bbr3", "cubic", "reno"];

/// Queue disciplines intentionally exposed by the PJZ110 build.
/// Each is either native on the target kernel or provided by this package.
pub const KNOWN_QDISCS: &[&str] = &["fq", "fq_codel", "codel", "cake", "pie", "fq_pie"];

/// Default description for module.prop
pub const DEFAULT_DESC: &str = "TCP Optimisations & update tcp_cong_algo based on interface";

/// Module id
pub const MODULE_ID: &str = "tcp_optimiser";

/// Module path under /data/adb/modules/
pub fn module_dir() -> PathBuf {
    for key in ["TCP_OPTIMISER_MODULE_DIR", "MODPATH"] {
        if let Some(path) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            return PathBuf::from(path);
        }
    }
    PathBuf::from("/data/adb/modules").join(MODULE_ID)
}

pub fn live_module_dir() -> PathBuf {
    PathBuf::from("/data/adb/modules").join(MODULE_ID)
}

pub fn is_known_algorithm(value: &str) -> bool {
    ALL_ALGOS.contains(&value)
}

pub fn is_known_qdisc(value: &str) -> bool {
    KNOWN_QDISCS.contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_algorithm_uses_cubic_defaults() {
        let unknown = get_algo_config("not-an-algorithm");
        let cubic = get_algo_config("cubic");
        assert_eq!(unknown.qdisc, cubic.qdisc);
        assert_eq!(unknown.pacing_ca, cubic.pacing_ca);
        assert_eq!(unknown.pacing_ss, cubic.pacing_ss);
    }

    #[test]
    fn kernel_names_are_allowlisted() {
        assert!(is_known_algorithm("bbr"));
        assert!(!is_known_algorithm("bbr\nreno"));
        assert!(is_known_qdisc("fq_codel"));
        assert!(!is_known_qdisc("fq;reboot"));
    }

    #[test]
    fn every_algorithm_resolves_to_a_supported_policy_shape() {
        for algorithm in ALL_ALGOS {
            let config = get_algo_config(algorithm);
            assert!(
                is_known_qdisc(config.qdisc),
                "{algorithm} maps to unknown qdisc {}",
                config.qdisc
            );
            assert!(config.pacing_ca > 0, "{algorithm} has zero CA pacing");
            assert!(
                config.pacing_ss > 0,
                "{algorithm} has zero slow-start pacing"
            );
            assert!(
                !config.desc.trim().is_empty(),
                "{algorithm} has no description"
            );
        }
    }
}
