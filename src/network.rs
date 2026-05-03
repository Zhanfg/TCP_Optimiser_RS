use std::io;
use std::process::Command;

/// Network interface mode
#[derive(Debug, Clone, PartialEq)]
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
            IfaceMode::WiFi => "\u{1f6dc}",   // 🛜 wireless icon
            IfaceMode::Cellular => "\u{1f4f6}", // 📶 signal icon
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
    let output = Command::new("ip")
        .args(["route", "get", "192.0.2.1"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let words: Vec<&str> = stdout.split_whitespace().collect();
    if let Some(pos) = words.iter().position(|&w| w == "dev") {
        if let Some(&iface) = words.get(pos + 1) {
            return Ok(iface.to_string());
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "no active interface"))
}

/// Determine the mode of an interface
pub fn iface_mode(iface: &str) -> IfaceMode {
    if iface.starts_with("wlan") || iface.starts_with("tun") {
        IfaceMode::WiFi
    } else if iface.starts_with("rmnet") || iface.starts_with("ccmni") {
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
pub fn kill_connections(iface: &str) {
    let _ = Command::new("ss")
        .args(["-K", &format!("dev {iface}")])
        .output();
}

/// Apply qdisc to an interface
pub fn set_qdisc(iface: &str, qdisc: &str) -> io::Result<()> {
    let output = Command::new("tc")
        .args(["qdisc", "replace", "dev", iface, "root", qdisc])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("tc failed: {stderr}"),
        ))
    }
}

/// Get initcwnd and initrwnd from route table
pub fn get_initcwnd_initrwnd() -> io::Result<Vec<u32>> {
    let output = Command::new("ip")
        .args(["route", "show"])
        .output()?;

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
    let Ok((_, _, max_rmem)) = crate::sysctl::tcp_rmem() else {
        return Ok(());
    };

    let Some(mtu) = iface_mtu(iface) else {
        return Ok(());
    };

    if mtu <= 40 {
        return Ok(());
    }

    let mtu_adjusted = mtu - 40;
    let max_initrwnd = max_rmem / mtu_adjusted;
    if max_initrwnd < 1 {
        return Ok(());
    }

    // Get routes for this interface
    let output = Command::new("ip")
        .args(["route", "show"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.contains(&format!("dev {iface}")) {
            // Build args: route + change + [route words] + initcwnd 10 initrwnd N
            let route_words: Vec<&str> = line.split_whitespace().collect();
            let mut args = vec!["route", "change"];
            args.extend(&route_words);
            let initcwnd_s = "initcwnd".to_string();
            let cwnd_val = "10".to_string();
            let initrwnd_s = "initrwnd".to_string();
            let rwnd_val = max_initrwnd.to_string();
            args.push(&initcwnd_s);
            args.push(&cwnd_val);
            args.push(&initrwnd_s);
            args.push(&rwnd_val);

            let _ = Command::new("ip").args(args).output();
        }
    }

    Ok(())
}
