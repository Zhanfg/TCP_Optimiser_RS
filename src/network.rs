use std::io;
use std::process::Command;

/// Network interface mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfaceMode {
    WiFi,
    Cellular,
    Unknown,
}

impl IfaceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            IfaceMode::WiFi => "Wi-Fi",
            IfaceMode::Cellular => "Cellular",
            IfaceMode::Unknown => "Unknown",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            IfaceMode::WiFi => "\u{1f6dc}",           // 🛜 wireless icon
            IfaceMode::Cellular => "\u{1f4f6}",       // 📶 signal icon
            IfaceMode::Unknown => "\u{2049}\u{fe0f}", // ⁉️
        }
    }

    pub fn prefix(&self) -> &'static str {
        match self {
            IfaceMode::WiFi => "wlan",
            IfaceMode::Cellular => "rmnet_data",
            IfaceMode::Unknown => "unknown",
        }
    }
}

/// Get the active network interface name
pub fn active_iface() -> io::Result<String> {
    match route_get_iface(&["-4", "route", "get", "1.1.1.1"]) {
        Ok(iface) if !is_virtual_iface(&iface) => Ok(iface),
        Ok(iface) => default_physical_iface().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "active route uses virtual interface {iface}, no physical default route found"
                ),
            )
        }),
        Err(route_error) => default_physical_iface().ok_or(route_error),
    }
}

fn route_get_iface(args: &[&str]) -> io::Result<String> {
    let output = Command::new("ip").args(args).output()?;
    if !output.status.success() {
        return Err(command_error("ip route get", &output.stderr));
    }
    parse_iface_after_dev(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no interface in route output"))
}

fn default_physical_iface() -> Option<String> {
    let output = Command::new("ip")
        .args(["route", "show", "table", "main"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.split_whitespace().next() == Some("default"))
        .filter_map(parse_iface_after_dev)
        .find(|iface| !is_virtual_iface(iface))
        .map(str::to_owned)
}

fn parse_iface_after_dev(output: &str) -> Option<&str> {
    let words: Vec<&str> = output.split_whitespace().collect();
    let pos = words.iter().position(|word| *word == "dev")?;
    words.get(pos + 1).copied()
}

fn is_virtual_iface(iface: &str) -> bool {
    ["tun", "tap", "wg"]
        .iter()
        .any(|prefix| iface.starts_with(prefix))
}

/// Determine the mode of an interface
pub fn iface_mode(iface: &str) -> IfaceMode {
    if ["wlan", "swlan", "wifi"]
        .iter()
        .any(|prefix| iface.starts_with(prefix))
    {
        IfaceMode::WiFi
    } else if ["rmnet", "ccmni", "ccemni", "wwan", "pdp", "v4-rmnet"]
        .iter()
        .any(|prefix| iface.starts_with(prefix))
    {
        IfaceMode::Cellular
    } else {
        IfaceMode::Unknown
    }
}

/// Get Wi-Fi frequency in MHz (returns None if not Wi-Fi or iw unavailable)
pub fn wifi_freq(iface: &str) -> Option<u32> {
    let output = Command::new("iw")
        .args(["dev", iface, "link"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("freq:") {
            // Format: "freq: 5180" or "    freq: 5180"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(pos) = parts.iter().position(|&w| w == "freq:") {
                if let Some(&freq_str) = parts.get(pos + 1) {
                    return freq_str.parse().ok();
                }
            }
        }
    }
    None
}

/// Get interface MTU
pub fn iface_mtu(iface: &str) -> Option<u32> {
    let output = Command::new("ip")
        .args(["link", "show", iface])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for word in stdout.split_whitespace() {
        if word.starts_with("mtu") {
            // Format: "mtu1500" or "mtu 1500"
            let mtu_str = word.trim_start_matches("mtu");
            if mtu_str.is_empty() {
                // Space-separated: find next word
                let words: Vec<&str> = stdout.split_whitespace().collect();
                if let Some(pos) = words.iter().position(|&w| w == "mtu") {
                    if let Some(&val) = words.get(pos + 1) {
                        return val.parse().ok();
                    }
                }
            } else {
                return mtu_str.parse().ok();
            }
        }
    }
    None
}

/// Kill TCP connections on an interface using ss -K
pub fn kill_connections(iface: &str) -> io::Result<()> {
    let output = Command::new("ss").args(["-K", "dev", iface]).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("ss -K", &output.stderr))
    }
}

/// Apply qdisc to an interface
pub fn set_qdisc(iface: &str, qdisc: &str) -> io::Result<()> {
    if !crate::config::is_known_qdisc(qdisc) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported qdisc: {qdisc}"),
        ));
    }
    let output = Command::new("tc")
        .args(["qdisc", "replace", "dev", iface, "root", qdisc])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(format!("tc failed: {stderr}")))
    }
}

/// Get initcwnd and initrwnd from route table
pub fn get_initcwnd_initrwnd() -> io::Result<Vec<u32>> {
    let output = Command::new("ip").args(["route", "show"]).output()?;
    if !output.status.success() {
        return Err(command_error("ip route show", &output.stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut values = Vec::new();
    let words: Vec<&str> = stdout.split_whitespace().collect();

    let mut i = 0;
    while i < words.len() {
        if words[i] == "initcwnd" {
            if let Some(&val) = words.get(i + 1) {
                if let Ok(n) = val.parse::<u32>() {
                    values.push(n);
                }
            }
        }
        if words[i] == "initrwnd" {
            if let Some(&val) = words.get(i + 1) {
                if let Ok(n) = val.parse::<u32>() {
                    values.push(n);
                }
            }
        }
        i += 1;
    }
    Ok(values)
}

/// Set initcwnd and initrwnd on all routes for a given interface
pub fn set_max_initcwnd_initrwnd(iface: &str) -> io::Result<()> {
    let (_, default_rmem, _) = crate::sysctl::tcp_rmem()?;

    let mtu = iface_mtu(iface).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("MTU unavailable for {iface}"),
        )
    })?;

    if mtu <= 40 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid MTU {mtu}"),
        ));
    }

    let maximum_segment_size = mtu - 40;
    let initrwnd = (default_rmem / maximum_segment_size).clamp(1, 1023);

    // Get routes for this interface
    let output = Command::new("ip").args(["route", "show"]).output()?;
    if !output.status.success() {
        return Err(command_error("ip route show", &output.stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut matched = 0usize;
    let mut failures = Vec::new();

    for line in stdout.lines() {
        let Some(args) = route_change_args(line, iface, initrwnd) else {
            continue;
        };
        matched += 1;
        match Command::new("ip").args(&args).output() {
            Ok(result) if result.status.success() => {}
            Ok(result) => failures.push(String::from_utf8_lossy(&result.stderr).trim().to_string()),
            Err(error) => failures.push(error.to_string()),
        }
    }

    if matched == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no routes found for {iface}"),
        ));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "failed to update {}/{} routes: {}",
            failures.len(),
            matched,
            failures.join("; ")
        )))
    }
}

fn route_change_args(line: &str, iface: &str, initrwnd: u32) -> Option<Vec<String>> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let dev_pos = words.iter().position(|word| *word == "dev")?;
    if words.get(dev_pos + 1).copied() != Some(iface) {
        return None;
    }

    let mut args = vec!["route".to_string(), "change".to_string()];
    let mut index = 0;
    while index < words.len() {
        if matches!(words[index], "initcwnd" | "initrwnd") {
            index += 2;
            continue;
        }
        args.push(words[index].to_string());
        index += 1;
    }
    args.extend([
        "initcwnd".to_string(),
        "10".to_string(),
        "initrwnd".to_string(),
        initrwnd.to_string(),
    ]);
    Some(args)
}

fn command_error(command: &str, stderr: &[u8]) -> io::Error {
    let detail = String::from_utf8_lossy(stderr);
    io::Error::other(format!("{command} failed: {}", detail.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_android_interfaces() {
        assert_eq!(iface_mode("wlan0"), IfaceMode::WiFi);
        assert_eq!(iface_mode("swlan0"), IfaceMode::WiFi);
        assert_eq!(iface_mode("rmnet_data0"), IfaceMode::Cellular);
        assert_eq!(iface_mode("ccemni0"), IfaceMode::Cellular);
        assert_eq!(iface_mode("tun0"), IfaceMode::Unknown);
    }

    #[test]
    fn rewrites_route_metrics_without_duplicates() {
        let args = route_change_args(
            "default via 192.168.1.1 dev wlan0 proto dhcp initcwnd 4 initrwnd 8",
            "wlan0",
            59,
        )
        .unwrap();
        assert_eq!(
            args.iter()
                .filter(|value| value.as_str() == "initcwnd")
                .count(),
            1
        );
        assert_eq!(
            args.iter()
                .filter(|value| value.as_str() == "initrwnd")
                .count(),
            1
        );
        assert_eq!(
            &args[args.len() - 4..],
            ["initcwnd", "10", "initrwnd", "59"]
        );
    }

    #[test]
    fn route_matching_uses_exact_interface_token() {
        assert!(route_change_args("default dev wlan01", "wlan0", 10).is_none());
        assert!(route_change_args("default dev wlan0", "wlan0", 10).is_some());
    }
}
