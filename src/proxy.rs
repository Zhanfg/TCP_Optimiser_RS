use std::fs;
use std::io;
use std::process::Command;

/// Proxy family detected
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyType {
    None,
    Mihomo,
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
            ProxyType::Mihomo => "Mihomo",
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
    let mut process_text = String::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str() else { continue };
            if !pid.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            let process_name = fs::read_link(entry.path().join("exe"))
                .ok()
                .and_then(|path| path.file_name().map(|name| name.to_owned()))
                .or_else(|| {
                    fs::read(entry.path().join("cmdline"))
                        .ok()
                        .and_then(|bytes| {
                            bytes
                                .split(|byte| *byte == 0)
                                .next()
                                .and_then(|argv0| {
                                    std::path::Path::new(std::str::from_utf8(argv0).ok()?)
                                        .file_name()
                                })
                                .map(|name| name.to_owned())
                        })
                });
            if let Some(name) = process_name {
                process_text.push_str(&name.to_string_lossy());
                process_text.push('\n');
            }
        }
    }
    if process_text.is_empty() {
        process_text = match Command::new("ps").arg("-A").output() {
            Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).into(),
            _ => return ProxyType::Unknown,
        };
    }
    detect_proxy_from_text(&process_text)
}

fn detect_proxy_from_text(process_text: &str) -> ProxyType {
    let output = process_text.to_lowercase();

    let families: &[(ProxyType, &[&str])] = &[
        (ProxyType::Mihomo, &["mihomo"]),
        (ProxyType::Clash, &["clash"]),
        (ProxyType::Surfing, &["surfing"]),
        (ProxyType::V2Ray, &["v2ray", "xray"]),
        (ProxyType::SingBox, &["sing-box"]),
        (
            ProxyType::Shadowsocks,
            &["ss-local", "ss-redir", "shadowsocks"],
        ),
        (
            ProxyType::Other,
            &[
                "nekobox", "nekoray", "hiddify", "trojan", "naive", "hysteria", "tuic",
            ],
        ),
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
        if output.status.success() {
            let procs = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if procs.contains("birdhost") {
                return HostsStatus::BirdHost;
            }
            if procs.contains("adaway") {
                return HostsStatus::AdAway;
            }
            let blockers = [
                "blokada",
                "dns66",
                "netguard",
                "block this",
                "rethink",
                "personaldns",
            ];
            if blockers.iter().any(|b| procs.contains(b)) {
                return HostsStatus::Blocker;
            }
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

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let registrations = stdout.lines().filter_map(parse_ims_registration);
        let mut found = false;
        for registered in registrations {
            found = true;
            if registered {
                return Ok(true);
            }
        }
        if found {
            return Ok(false);
        }
    }

    // Fallback: check SystemUIService for vowifi
    let output = Command::new("dumpsys")
        .args(["activity", "service", "SystemUIService"])
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "dumpsys SystemUIService failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(systemui_has_vowifi(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_ims_registration(line: &str) -> Option<bool> {
    let marker = "mImsRegistered";
    let value = line.split_once(marker)?.1.trim_start();
    let value = value
        .strip_prefix('=')
        .or_else(|| value.strip_prefix(':'))?
        .trim_start();
    match value
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()?
    {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn systemui_has_vowifi(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.contains("slot='vowifi'") && line.contains("visible user="))
}

#[cfg(test)]
mod tests {
    use super::{detect_proxy_from_text, parse_ims_registration, systemui_has_vowifi, ProxyType};

    #[test]
    fn distinguishes_mihomo_and_reads_full_command_lines() {
        assert_eq!(detect_proxy_from_text("mihomo\n"), ProxyType::Mihomo);
        assert_eq!(detect_proxy_from_text("clash\n"), ProxyType::Clash);
    }

    #[test]
    fn recognizes_visible_vowifi_slot() {
        assert!(systemui_has_vowifi("slot='vowifi' visible user=0"));
        assert!(!systemui_has_vowifi("slot='wifi' visible user=0"));
    }

    #[test]
    fn parses_only_the_ims_registration_field() {
        assert_eq!(parse_ims_registration("mImsRegistered=true"), Some(true));
        assert_eq!(
            parse_ims_registration("mImsRegistered=false other=true"),
            Some(false)
        );
        assert_eq!(parse_ims_registration("mImsRegisteredState=true"), None);
    }
}
