use std::io;
use std::process::Command;

/// Proxy family detected
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyType {
    None,
    Clash,
    Surfing,
    V2Ray,
    SingBox,
    Shadowsocks,
    Other,
    Multiple,
    Unknown,
}

impl ProxyType {
    pub fn label(&self) -> &'static str {
        match self {
            ProxyType::None => "\u{2014}",
            ProxyType::Clash => "Clash",
            ProxyType::Surfing => "Surfing",
            ProxyType::V2Ray => "V2Ray / Xray",
            ProxyType::SingBox => "sing-box",
            ProxyType::Shadowsocks => "Shadowsocks",
            ProxyType::Other => "Detected",
            ProxyType::Multiple => "Multiple",
            ProxyType::Unknown => "...",
        }
    }
}

/// Detect running proxy applications
pub fn detect_proxy() -> ProxyType {
    let output = match Command::new("ps").args(["-A", "-o", "comm="]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_lowercase(),
        Err(_) => return ProxyType::Unknown,
    };

    let families: &[(ProxyType, &[&str])] = &[
        (ProxyType::Clash, &["clash", "mihomo"]),
        (ProxyType::Surfing, &["surfing"]),
        (ProxyType::V2Ray, &["v2ray", "xray"]),
        (ProxyType::SingBox, &["sing-box"]),
        (ProxyType::Shadowsocks, &["ss-local", "ss-redir", "shadowsocks"]),
        (ProxyType::Other, &["nekobox", "nekoray", "hiddify", "trojan", "naive", "hysteria", "tuic"]),
    ];

    let hits: Vec<&ProxyType> = families
        .iter()
        .filter(|(_, patterns)| patterns.iter().any(|p| output.contains(p)))
        .map(|(t, _)| t)
        .collect();

    if hits.is_empty() {
        ProxyType::None
    } else if hits.len() > 1 {
        ProxyType::Multiple
    } else {
        hits[0].clone()
    }
}

/// Hosts modification status
#[derive(Debug, Clone, PartialEq)]
pub enum HostsStatus {
    None,
    Systemless,
    BirdHost,
    AdAway,
    Blocker,
    Blocked(u32),
    Modified,
    Unknown,
}

impl HostsStatus {
    pub fn label(&self) -> String {
        match self {
            HostsStatus::None => "\u{2014}".to_string(),
            HostsStatus::Systemless => "Systemless".to_string(),
            HostsStatus::BirdHost => "BirdHost".to_string(),
            HostsStatus::AdAway => "AdAway".to_string(),
            HostsStatus::Blocker => "Blocker App".to_string(),
            HostsStatus::Blocked(n) => format!("{n} blocked"),
            HostsStatus::Modified => "Modified".to_string(),
            HostsStatus::Unknown => "\u{2014}".to_string(),
        }
    }
}

/// Detect hosts file modifications
pub fn detect_hosts() -> HostsStatus {
    // Check running apps first
    if let Ok(output) = Command::new("ps").args(["-A", "-o", "comm="]).output() {
        let procs = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if procs.contains("birdhost") {
            return HostsStatus::BirdHost;
        }
        if procs.contains("adaway") {
            return HostsStatus::AdAway;
        }
        let blockers = ["blokada", "dns66", "netguard", "block this", "rethink", "personaldns"];
        if blockers.iter().any(|b| procs.contains(b)) {
            return HostsStatus::Blocker;
        }
    }

    // Check systemless hosts (Magisk)
    if std::path::Path::new("/data/adb/modules/hosts").exists()
        || std::path::Path::new("/data/adb/modules_update/hosts").exists()
    {
        return HostsStatus::Systemless;
    }

    // Check /etc/hosts size and blocked entries
    let hosts_path = "/etc/hosts";
    let Ok(content) = std::fs::read_to_string(hosts_path) else {
        return HostsStatus::Unknown;
    };

    let size = content.len();
    let blocked = content
        .lines()
        .filter(|line| {
            let t = line.trim();
            t.starts_with("0.0.0.0 ") || t.starts_with("127.0.0.1 ")
        })
        .count() as u32;

    if size < 200 && blocked <= 3 {
        HostsStatus::None
    } else if blocked > 5 {
        HostsStatus::Blocked(blocked)
    } else {
        HostsStatus::Modified
    }
}

/// Check VoWiFi state via dumpsys
pub fn wifi_calling_active() -> io::Result<bool> {
    // Try telephony.registry first
    let output = Command::new("dumpsys")
        .args(["telephony.registry"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().find(|l| l.contains("mImsRegistered")) {
        return Ok(line.contains("true"));
    }

    // Fallback: check SystemUIService for vowifi
    let output = Command::new("dumpsys")
        .args(["activity", "service", "SystemUIService"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().any(|l| {
        l.contains("slot='vowifi'") && l.contains("visible user=")
    }))
}
