use std::fs;
use std::io;
use std::path::Path;

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
    let parts: Vec<u32> = raw
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    Ok((*parts.first().unwrap_or(&4096), *parts.get(1).unwrap_or(&87380), *parts.get(2).unwrap_or(&16777216)))
}

/// Apply base TCP sysctl optimisations (replaces apply_tcp_sysctls)
pub fn apply_base_sysctls() {
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_ecn", "1");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_pacing_ca_ratio", "150");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_pacing_ss_ratio", "200");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_window_scaling", "1");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_rmem", "4096 87380 16777216");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_wmem", "4096 65536 16777216");
    let _ = write_sysctl("/proc/sys/net/core/rmem_max", "16777216");
    let _ = write_sysctl("/proc/sys/net/core/wmem_max", "16777216");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_max_syn_backlog", "4096");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_mtu_probing", "1");
    let _ = write_sysctl("/proc/sys/net/ipv6/tcp_ecn", "1");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_fastopen", "3");
    let _ = write_sysctl("/proc/sys/net/ipv4/tcp_tw_reuse", "1");
}
