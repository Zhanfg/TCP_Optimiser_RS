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
            "Google BBR - high throughput, low latency",
        ),
        ("bbr2", "fq", 200, 300, "BBR v2 - improved fairness"),
        ("bbr3", "fq", 220, 320, "BBR v3 - experimental"),
        (
            "cubic",
            "fq_codel",
            150,
            200,
            "Default Linux - stable and reliable",
        ),
        (
            "westwood",
            "fq_codel",
            150,
            200,
            "Bandwidth estimation - good for wireless",
        ),
        (
            "westwood_plus",
            "fq_codel",
            150,
            200,
            "Westwood+ - improved wireless variant",
        ),
        (
            "reno",
            "fq_codel",
            150,
            200,
            "Classic TCP - widely compatible",
        ),
        (
            "htcp",
            "fq_codel",
            150,
            200,
            "Hamilton TCP - high-speed long-distance",
        ),
        (
            "vegas",
            "fq_codel",
            120,
            180,
            "Delay-based - low latency, less aggressive",
        ),
        (
            "yeah",
            "fq_codel",
            120,
            180,
            "YeAH - high-speed with fairness",
        ),
        (
            "illinois",
            "fq_codel",
            120,
            180,
            "Illinois - hybrid for high BDP paths",
        ),
        (
            "dctcp",
            "fq",
            100,
            150,
            "Data Center TCP - low queuing delay",
        ),
        ("cdg", "fq", 120, 180, "CAIA Delay Gradient - delay-based"),
        (
            "bic",
            "fq_codel",
            150,
            200,
            "Binary Increase - high-speed predecessor",
        ),
        (
            "highspeed",
            "fq_codel",
            180,
            250,
            "HighSpeed - RFC 3649 for fast links",
        ),
        (
            "hybla",
            "fq_codel",
            120,
            180,
            "Hybla - satellite / high-latency links",
        ),
        ("nv", "fq_codel", 120, 180, "New Vegas - modern delay-based"),
        (
            "scalable",
            "fq_codel",
            180,
            250,
            "Scalable - simple high-speed variant",
        ),
        (
            "lp",
            "fq_codel",
            120,
            180,
            "Low Priority - background transfers",
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

/// All known algorithms in display order
pub const ALL_ALGOS: &[&str] = &[
    "bbr",
    "bbr2",
    "bbr3",
    "cubic",
    "westwood",
    "westwood_plus",
    "reno",
    "htcp",
    "vegas",
    "yeah",
    "illinois",
    "dctcp",
    "cdg",
    "bic",
    "highspeed",
    "hybla",
    "nv",
    "scalable",
    "lp",
];

/// Known qdiscs for the global selector
pub const KNOWN_QDISCS: &[&str] = &[
    "fq",
    "fq_codel",
    "cake",
    "pfifo_fast",
    "codel",
    "fq_pie",
    "pfifo",
];

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
}
