use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdvancedOverride {
    pub key: &'static str,
    pub path: &'static str,
    pub value: u32,
}

const ADVANCED_SYSCTLS: &[(&str, &str, u32, u32)] = &[
    (
        "tcp_keepalive_time",
        "/proc/sys/net/ipv4/tcp_keepalive_time",
        10,
        7200,
    ),
    (
        "tcp_keepalive_intvl",
        "/proc/sys/net/ipv4/tcp_keepalive_intvl",
        1,
        300,
    ),
    (
        "tcp_keepalive_probes",
        "/proc/sys/net/ipv4/tcp_keepalive_probes",
        1,
        30,
    ),
    (
        "tcp_fin_timeout",
        "/proc/sys/net/ipv4/tcp_fin_timeout",
        5,
        120,
    ),
    (
        "tcp_syn_retries",
        "/proc/sys/net/ipv4/tcp_syn_retries",
        1,
        10,
    ),
    (
        "tcp_synack_retries",
        "/proc/sys/net/ipv4/tcp_synack_retries",
        1,
        10,
    ),
    ("tcp_retries2", "/proc/sys/net/ipv4/tcp_retries2", 3, 20),
    (
        "rmem_max",
        "/proc/sys/net/core/rmem_max",
        65_536,
        134_217_728,
    ),
    (
        "wmem_max",
        "/proc/sys/net/core/wmem_max",
        65_536,
        134_217_728,
    ),
    (
        "optmem_max",
        "/proc/sys/net/core/optmem_max",
        10_240,
        4_194_304,
    ),
    (
        "tcp_notsent_lowat",
        "/proc/sys/net/ipv4/tcp_notsent_lowat",
        0,
        u32::MAX,
    ),
    ("somaxconn", "/proc/sys/net/core/somaxconn", 128, 65_535),
    (
        "netdev_max_backlog",
        "/proc/sys/net/core/netdev_max_backlog",
        256,
        65_535,
    ),
    (
        "tcp_max_syn_backlog",
        "/proc/sys/net/ipv4/tcp_max_syn_backlog",
        128,
        65_535,
    ),
    (
        "netdev_budget",
        "/proc/sys/net/core/netdev_budget",
        64,
        4_096,
    ),
    (
        "netdev_budget_usecs",
        "/proc/sys/net/core/netdev_budget_usecs",
        500,
        50_000,
    ),
    (
        "tcp_mtu_probing",
        "/proc/sys/net/ipv4/tcp_mtu_probing",
        0,
        2,
    ),
    ("tcp_sack", "/proc/sys/net/ipv4/tcp_sack", 0, 1),
    ("tcp_dsack", "/proc/sys/net/ipv4/tcp_dsack", 0, 1),
    (
        "tcp_no_metrics_save",
        "/proc/sys/net/ipv4/tcp_no_metrics_save",
        0,
        1,
    ),
    (
        "tcp_slow_start_after_idle",
        "/proc/sys/net/ipv4/tcp_slow_start_after_idle",
        0,
        1,
    ),
    ("tcp_tw_reuse", "/proc/sys/net/ipv4/tcp_tw_reuse", 0, 2),
    (
        "tcp_autocorking",
        "/proc/sys/net/ipv4/tcp_autocorking",
        0,
        1,
    ),
    (
        "tcp_early_retrans",
        "/proc/sys/net/ipv4/tcp_early_retrans",
        0,
        4,
    ),
    (
        "tcp_thin_linear_timeouts",
        "/proc/sys/net/ipv4/tcp_thin_linear_timeouts",
        0,
        1,
    ),
    (
        "tcp_thin_dupack",
        "/proc/sys/net/ipv4/tcp_thin_dupack",
        0,
        1,
    ),
    (
        "tcp_rto_max_ms",
        "/proc/sys/net/ipv4/tcp_rto_max_ms",
        1_000,
        120_000,
    ),
    (
        "tcp_plb_enabled",
        "/proc/sys/net/ipv4/tcp_plb_enabled",
        0,
        1,
    ),
    (
        "tcp_plb_idle_rehash_rounds",
        "/proc/sys/net/ipv4/tcp_plb_idle_rehash_rounds",
        0,
        31,
    ),
    (
        "tcp_plb_rehash_rounds",
        "/proc/sys/net/ipv4/tcp_plb_rehash_rounds",
        0,
        31,
    ),
    (
        "tcp_plb_suspend_rto_sec",
        "/proc/sys/net/ipv4/tcp_plb_suspend_rto_sec",
        0,
        255,
    ),
    (
        "tcp_plb_cong_thresh",
        "/proc/sys/net/ipv4/tcp_plb_cong_thresh",
        0,
        256,
    ),
    ("busy_poll", "/proc/sys/net/core/busy_poll", 0, 100_000),
    ("busy_read", "/proc/sys/net/core/busy_read", 0, 100_000),
    (
        "nf_conntrack_max",
        "/proc/sys/net/netfilter/nf_conntrack_max",
        1_024,
        1_048_576,
    ),
    (
        "nf_conntrack_tcp_timeout_established",
        "/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_established",
        60,
        432_000,
    ),
    (
        "nf_conntrack_tcp_timeout_time_wait",
        "/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_time_wait",
        1,
        600,
    ),
];

/// Read a sysctl value from /proc/sys
pub fn read_sysctl(path: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(path).map(|s| s.trim().to_string())
}

/// Write a value to a sysctl path
pub fn write_sysctl(path: impl AsRef<Path>, value: &str) -> io::Result<()> {
    fs::write(path, value)
}

/// Read available congestion control algorithms
pub fn available_algorithms() -> io::Result<Vec<String>> {
    let raw = read_sysctl("/proc/sys/net/ipv4/tcp_available_congestion_control")?;
    Ok(raw.split_whitespace().map(String::from).collect())
}

/// Read current congestion control algorithm
pub fn current_algorithm() -> io::Result<String> {
    read_sysctl("/proc/sys/net/ipv4/tcp_congestion_control")
}

/// Set congestion control algorithm
pub fn set_congestion_control(algo: &str) -> io::Result<()> {
    if !crate::config::is_known_algorithm(algo) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported congestion algorithm: {algo}"),
        ));
    }
    write_sysctl("/proc/sys/net/ipv4/tcp_congestion_control", algo)
}

/// Check if an algorithm is available
pub fn algo_available(algo: &str) -> io::Result<bool> {
    let raw = read_sysctl("/proc/sys/net/ipv4/tcp_available_congestion_control")?;
    Ok(raw.split_whitespace().any(|a| a == algo))
}

/// Read default qdisc
pub fn default_qdisc() -> io::Result<String> {
    read_sysctl("/proc/sys/net/core/default_qdisc")
}

/// Set default qdisc
pub fn set_default_qdisc(qdisc: &str) -> io::Result<()> {
    if !crate::config::is_known_qdisc(qdisc) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported qdisc: {qdisc}"),
        ));
    }
    write_sysctl("/proc/sys/net/core/default_qdisc", qdisc)
}

/// Set TCP pacing ratios
pub fn set_pacing(ca_ratio: u32, ss_ratio: u32) -> io::Result<()> {
    write_sysctl(
        "/proc/sys/net/ipv4/tcp_pacing_ca_ratio",
        &ca_ratio.to_string(),
    )?;
    write_sysctl(
        "/proc/sys/net/ipv4/tcp_pacing_ss_ratio",
        &ss_ratio.to_string(),
    )
}

/// Read TCP receive memory (min default max)
pub fn tcp_rmem() -> io::Result<(u32, u32, u32)> {
    let raw = read_sysctl("/proc/sys/net/ipv4/tcp_rmem")?;
    parse_sysctl_triplet(&raw)
}

/// Apply base TCP sysctl optimisations (replaces apply_tcp_sysctls)
pub fn apply_base_sysctls() -> Vec<String> {
    let mut failures = Vec::new();
    let tcp_ecn = read_bounded_override("tcp_ecn", 0, 2).unwrap_or(1);
    let tcp_fastopen = read_bounded_override("tcp_fastopen", 0, 3).unwrap_or(3);
    let values = [
        ("/proc/sys/net/ipv4/tcp_ecn", tcp_ecn.to_string()),
        ("/proc/sys/net/ipv4/tcp_pacing_ca_ratio", "150".to_string()),
        ("/proc/sys/net/ipv4/tcp_pacing_ss_ratio", "200".to_string()),
        ("/proc/sys/net/ipv4/tcp_window_scaling", "1".to_string()),
        ("/proc/sys/net/ipv4/tcp_max_syn_backlog", "4096".to_string()),
        ("/proc/sys/net/ipv4/tcp_mtu_probing", "1".to_string()),
        ("/proc/sys/net/ipv4/tcp_fastopen", tcp_fastopen.to_string()),
        ("/proc/sys/net/ipv4/tcp_tw_reuse", "1".to_string()),
    ];
    for (path, value) in values {
        write_optional(path, &value, &mut failures);
    }

    preserve_and_raise_triplet(
        "/proc/sys/net/ipv4/tcp_rmem",
        (4096, 87380, 16_777_216),
        &mut failures,
    );
    preserve_and_raise_triplet(
        "/proc/sys/net/ipv4/tcp_wmem",
        (4096, 65536, 16_777_216),
        &mut failures,
    );
    preserve_and_raise_scalar("/proc/sys/net/core/rmem_max", 16_777_216, &mut failures);
    preserve_and_raise_scalar("/proc/sys/net/core/wmem_max", 16_777_216, &mut failures);
    apply_advanced_overrides(&mut failures);
    failures
}

fn apply_advanced_overrides(failures: &mut Vec<String>) {
    let (overrides, parse_failures) = configured_advanced_overrides();
    failures.extend(parse_failures);
    for item in overrides {
        write_optional(item.path, &item.value.to_string(), failures);
    }
}

pub(crate) fn configured_advanced_overrides() -> (Vec<AdvancedOverride>, Vec<String>) {
    let path = crate::config::module_dir().join("advanced.conf");
    let Ok(content) = fs::read_to_string(path) else {
        return (Vec::new(), Vec::new());
    };
    parse_advanced_overrides(&content)
}

fn parse_advanced_overrides(content: &str) -> (Vec<AdvancedOverride>, Vec<String>) {
    let mut overrides = Vec::new();
    let mut failures = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((key, raw_value)) = line.split_once('=') else {
            failures.push(format!("advanced.conf: malformed entry {line:?}"));
            continue;
        };
        let Some(&(known_key, sysctl_path, min, max)) = ADVANCED_SYSCTLS
            .iter()
            .find(|(known, _, _, _)| *known == key)
        else {
            continue;
        };
        let Some(value) = parse_bounded_value(raw_value, min, max) else {
            failures.push(format!("advanced.conf: invalid value for {key}"));
            continue;
        };
        overrides.push(AdvancedOverride {
            key: known_key,
            path: sysctl_path,
            value,
        });
    }
    (overrides, failures)
}

fn read_bounded_override(name: &str, min: u32, max: u32) -> Option<u32> {
    fs::read_to_string(crate::config::module_dir().join(name))
        .ok()
        .and_then(|value| parse_bounded_value(&value, min, max))
}

fn parse_bounded_value(value: &str, min: u32, max: u32) -> Option<u32> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|value| (min..=max).contains(value))
}

fn parse_sysctl_triplet(raw: &str) -> io::Result<(u32, u32, u32)> {
    let values = raw
        .split_whitespace()
        .map(|value| {
            value.parse::<u32>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid sysctl value {value:?}: {error}"),
                )
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    match values.as_slice() {
        [min, default, max] => Ok((*min, *default, *max)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected three sysctl values, got {}", values.len()),
        )),
    }
}

fn write_optional(path: &str, value: &str, failures: &mut Vec<String>) {
    if !Path::new(path).exists() {
        return;
    }
    if let Err(error) = write_sysctl(path, value) {
        failures.push(format!("{path}: {error}"));
    }
}

fn preserve_and_raise_scalar(path: &str, floor: u32, failures: &mut Vec<String>) {
    if !Path::new(path).exists() {
        return;
    }
    let value = read_sysctl(path)
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .map_or(floor, |current| current.max(floor));
    write_optional(path, &value.to_string(), failures);
}

fn preserve_and_raise_triplet(path: &str, floor: (u32, u32, u32), failures: &mut Vec<String>) {
    if !Path::new(path).exists() {
        return;
    }
    let value = read_sysctl(path)
        .and_then(|raw| parse_sysctl_triplet(&raw))
        .map(|current| {
            (
                current.0.max(floor.0),
                current.1.max(floor.1),
                current.2.max(floor.2),
            )
        })
        .unwrap_or(floor);
    write_optional(
        path,
        &format!("{} {} {}", value.0, value.1, value.2),
        failures,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        parse_advanced_overrides, parse_bounded_value, parse_sysctl_triplet, AdvancedOverride,
        ADVANCED_SYSCTLS,
    };

    #[test]
    fn parses_exact_triplet() {
        assert_eq!(
            parse_sysctl_triplet("4096 87380 16777216").unwrap(),
            (4096, 87380, 16_777_216)
        );
    }

    #[test]
    fn rejects_partial_or_malformed_triplet() {
        assert!(parse_sysctl_triplet("4096 87380").is_err());
        assert!(parse_sysctl_triplet("4096 nope 16777216").is_err());
    }

    #[test]
    fn bounded_overrides_reject_invalid_kernel_values() {
        assert_eq!(parse_bounded_value("2\n", 0, 3), Some(2));
        assert_eq!(parse_bounded_value("4", 0, 3), None);
        assert_eq!(parse_bounded_value("-1", 0, 3), None);
        assert_eq!(parse_bounded_value("1; reboot", 0, 3), None);
    }

    #[test]
    fn advanced_sysctl_keys_and_paths_are_unique() {
        for (index, (key, path, min, max)) in ADVANCED_SYSCTLS.iter().enumerate() {
            assert!(min <= max);
            assert!(path.starts_with("/proc/sys/"));
            assert!(!ADVANCED_SYSCTLS[..index]
                .iter()
                .any(|(other, _, _, _)| other == key));
            assert!(!ADVANCED_SYSCTLS[..index]
                .iter()
                .any(|(_, other, _, _)| other == path));
        }
    }

    #[test]
    fn advanced_config_survives_boot_parse_and_rejects_bad_entries() {
        let content = "tcp_fin_timeout=30\ntcp_mtu_probing=2\ntcp_plb_idle_rehash_rounds=3\ntcp_plb_idle_retransmit_rounds=4\ntcp_sack=99\nbroken\n";
        let (overrides, failures) = parse_advanced_overrides(content);

        assert_eq!(
            overrides,
            vec![
                AdvancedOverride {
                    key: "tcp_fin_timeout",
                    path: "/proc/sys/net/ipv4/tcp_fin_timeout",
                    value: 30,
                },
                AdvancedOverride {
                    key: "tcp_mtu_probing",
                    path: "/proc/sys/net/ipv4/tcp_mtu_probing",
                    value: 2,
                },
                AdvancedOverride {
                    key: "tcp_plb_idle_rehash_rounds",
                    path: "/proc/sys/net/ipv4/tcp_plb_idle_rehash_rounds",
                    value: 3,
                },
            ]
        );
        assert_eq!(failures.len(), 2);
        assert!(failures.iter().any(|entry| entry.contains("tcp_sack")));
        assert!(failures.iter().any(|entry| entry.contains("malformed")));
    }
}
