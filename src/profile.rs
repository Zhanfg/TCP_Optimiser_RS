use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::network::{self, IfaceMode};

const PROFILE_FILE: &str = "auto_profile.json";
const AUTO_CONFIG_FILE: &str = "auto.conf";
const DISABLE_AUTO_FILE: &str = "disable_auto_tuning";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AutoRecommendations {
    pub socket_buffer_floor: u32,
    pub somaxconn: u32,
    pub netdev_max_backlog: u32,
    pub nf_conntrack_max: Option<u32>,
    pub tcp_mtu_probing: u32,
    pub tcp_sack: u32,
    pub tcp_dsack: u32,
    pub tcp_no_metrics_save: u32,
    pub tcp_autocorking: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetworkProfile {
    pub schema: u32,
    pub generated_epoch: u64,
    pub auto_tuning_enabled: bool,
    pub kernel_release: String,
    pub kmi: Option<String>,
    pub arch: String,
    pub memory_kib: Option<u64>,
    pub active_iface: Option<String>,
    pub iface_mode: String,
    pub iface_mtu: Option<u32>,
    pub proxy: crate::proxy::ProxySnapshot,
    pub available_algorithms: Vec<String>,
    pub bundled_algorithms: Vec<String>,
    pub bundled_qdiscs: Vec<String>,
    pub wifi_algorithm: Option<String>,
    pub cellular_algorithm: Option<String>,
    pub qdisc_policy: String,
    pub recommendations: AutoRecommendations,
}

pub fn auto_tuning_enabled() -> bool {
    !crate::config::module_dir().join(DISABLE_AUTO_FILE).exists()
}

pub fn set_auto_tuning(enabled: bool) -> io::Result<NetworkProfile> {
    let module_dir = crate::config::module_dir();
    fs::create_dir_all(&module_dir)?;
    let marker = module_dir.join(DISABLE_AUTO_FILE);
    if enabled {
        if marker.exists() {
            fs::remove_file(marker)?;
        }
    } else if !marker.exists() {
        fs::write(marker, b"disabled\n")?;
    }
    let (profile, _) = refresh_managed_profile()?;
    Ok(profile)
}

pub fn load_or_refresh() -> io::Result<NetworkProfile> {
    let path = crate::config::module_dir().join(PROFILE_FILE);
    if let Ok(content) = fs::read(&path) {
        if let Ok(profile) = serde_json::from_slice::<NetworkProfile>(&content) {
            return Ok(profile);
        }
    }
    refresh_managed_profile().map(|(profile, _)| profile)
}

pub fn refresh_managed_profile() -> io::Result<(NetworkProfile, bool)> {
    let module_dir = crate::config::module_dir();
    fs::create_dir_all(&module_dir)?;

    let profile_path = module_dir.join(PROFILE_FILE);
    let mut profile = collect_profile();
    if let Ok(content) = fs::read(&profile_path) {
        if let Ok(mut previous) = serde_json::from_slice::<NetworkProfile>(&content) {
            let previous_epoch = previous.generated_epoch;
            previous.generated_epoch = 0;
            let mut current = profile.clone();
            current.generated_epoch = 0;
            if previous == current {
                profile.generated_epoch = previous_epoch;
            }
        }
    }

    let auto_config = render_managed_config(&profile);
    let managed_changed =
        write_if_changed(&module_dir.join(AUTO_CONFIG_FILE), auto_config.as_bytes())?;

    let json = serde_json::to_vec_pretty(&profile).map_err(io::Error::other)?;
    write_if_changed(&profile_path, &json)?;

    Ok((profile, managed_changed))
}

pub fn recommended_socket_buffer_floor() -> u32 {
    socket_buffer_floor_for_memory(memory_kib())
}

fn collect_profile() -> NetworkProfile {
    let memory = memory_kib();
    let proxy = crate::proxy::detect_proxy_snapshot();
    let active_iface = network::active_iface().ok();
    let mode = active_iface
        .as_deref()
        .map(network::iface_mode)
        .unwrap_or(IfaceMode::Unknown);
    let iface_mtu = active_iface.as_deref().and_then(network::iface_mtu);
    let available_algorithms = crate::kernel_module::augment_algorithms(
        crate::sysctl::available_algorithms().unwrap_or_default(),
    );
    let recommendations = recommendations(memory, &proxy);

    NetworkProfile {
        schema: 1,
        generated_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        auto_tuning_enabled: auto_tuning_enabled(),
        kernel_release: kernel_release().unwrap_or_else(|| "unknown".to_string()),
        kmi: crate::kernel_module::current_kmi(),
        arch: std::env::consts::ARCH.to_string(),
        memory_kib: memory,
        active_iface,
        iface_mode: mode.as_str().to_string(),
        iface_mtu,
        proxy,
        available_algorithms,
        bundled_algorithms: crate::kernel_module::bundled_algorithms(),
        bundled_qdiscs: crate::kernel_module::bundled_qdiscs(),
        wifi_algorithm: selected_algorithm("wlan"),
        cellular_algorithm: selected_algorithm("rmnet_data"),
        qdisc_policy: if crate::config::module_dir().join("qdisc").is_file() {
            "manual".to_string()
        } else {
            "per_algorithm".to_string()
        },
        recommendations,
    }
}

fn recommendations(
    memory: Option<u64>,
    proxy: &crate::proxy::ProxySnapshot,
) -> AutoRecommendations {
    let memory_gib = memory.unwrap_or(0) / 1_048_576;
    let socket_buffer_floor = socket_buffer_floor_for_memory(memory);
    let somaxconn = match memory_gib {
        0..=4 => 1024,
        5..=8 => 2048,
        _ => 4096,
    };
    let netdev_max_backlog = match memory_gib {
        0..=4 => 2048,
        5..=8 => 4096,
        _ => 8192,
    };
    let nf_conntrack_max = proxy.transparent.then(|| {
        let floor = if memory_gib >= 8 { 262_144 } else { 131_072 };
        read_u32("/proc/sys/net/netfilter/nf_conntrack_max")
            .unwrap_or(floor)
            .max(floor)
    });

    AutoRecommendations {
        socket_buffer_floor,
        somaxconn,
        netdev_max_backlog,
        nf_conntrack_max,
        tcp_mtu_probing: 1,
        tcp_sack: 1,
        tcp_dsack: 1,
        tcp_no_metrics_save: 0,
        tcp_autocorking: 1,
    }
}

fn render_managed_config(profile: &NetworkProfile) -> String {
    let r = &profile.recommendations;
    let mut values = BTreeMap::from([
        ("netdev_max_backlog", r.netdev_max_backlog),
        ("somaxconn", r.somaxconn),
        ("tcp_autocorking", r.tcp_autocorking),
        ("tcp_dsack", r.tcp_dsack),
        ("tcp_mtu_probing", r.tcp_mtu_probing),
        ("tcp_no_metrics_save", r.tcp_no_metrics_save),
        ("tcp_sack", r.tcp_sack),
    ]);
    if let Some(value) = r.nf_conntrack_max {
        values.insert("nf_conntrack_max", value);
    }

    let mut output = String::new();
    for (key, value) in values {
        output.push_str(key);
        output.push('=');
        output.push_str(&value.to_string());
        output.push('\n');
    }
    output
}

fn socket_buffer_floor_for_memory(memory: Option<u64>) -> u32 {
    match memory.unwrap_or(0) / 1_048_576 {
        0..=7 => 16_777_216,
        8..=11 => 25_165_824,
        _ => 33_554_432,
    }
}

fn memory_kib() -> Option<u64> {
    fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("MemTotal:")?;
            rest.split_whitespace().next()?.parse().ok()
        })
}

fn selected_algorithm(prefix: &str) -> Option<String> {
    let needle = format!("{prefix}_");
    let mut selected = fs::read_dir(crate::config::module_dir())
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let algorithm = name.strip_prefix(&needle)?;
            (entry.path().is_file() && crate::config::is_known_algorithm(algorithm))
                .then(|| algorithm.to_string())
        })
        .collect::<Vec<_>>();
    selected.sort();
    selected.into_iter().next()
}

fn kernel_release() -> Option<String> {
    if let Ok(release) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        let release = release.trim();
        if !release.is_empty() {
            return Some(release.to_string());
        }
    }
    let output = Command::new("uname").arg("-r").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_u32(path: &str) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn write_if_changed(path: &Path, content: &[u8]) -> io::Result<bool> {
    if fs::read(path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, content)?;
    fs::rename(temporary, path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        render_managed_config, socket_buffer_floor_for_memory, AutoRecommendations, NetworkProfile,
    };
    use crate::proxy::ProxySnapshot;

    #[test]
    fn buffer_floor_scales_without_reducing_legacy_floor() {
        assert_eq!(
            socket_buffer_floor_for_memory(Some(4 * 1_048_576)),
            16_777_216
        );
        assert_eq!(
            socket_buffer_floor_for_memory(Some(8 * 1_048_576)),
            25_165_824
        );
        assert_eq!(
            socket_buffer_floor_for_memory(Some(12 * 1_048_576)),
            33_554_432
        );
    }

    #[test]
    fn managed_config_is_stable_and_proxy_aware() {
        let profile = NetworkProfile {
            schema: 1,
            generated_epoch: 0,
            auto_tuning_enabled: true,
            kernel_release: "test".to_string(),
            kmi: None,
            arch: "aarch64".to_string(),
            memory_kib: Some(8 * 1_048_576),
            active_iface: Some("wlan0".to_string()),
            iface_mode: "Wi-Fi".to_string(),
            iface_mtu: Some(1500),
            proxy: ProxySnapshot {
                family: "mihomo".to_string(),
                label: "Mihomo".to_string(),
                mode: "tproxy".to_string(),
                transparent: true,
                tproxy: true,
                virtual_iface: None,
            },
            available_algorithms: vec!["cubic".to_string()],
            bundled_algorithms: Vec::new(),
            bundled_qdiscs: Vec::new(),
            wifi_algorithm: Some("cubic".to_string()),
            cellular_algorithm: Some("cubic".to_string()),
            qdisc_policy: "per_algorithm".to_string(),
            recommendations: AutoRecommendations {
                socket_buffer_floor: 25_165_824,
                somaxconn: 2048,
                netdev_max_backlog: 4096,
                nf_conntrack_max: Some(262_144),
                tcp_mtu_probing: 1,
                tcp_sack: 1,
                tcp_dsack: 1,
                tcp_no_metrics_save: 0,
                tcp_autocorking: 1,
            },
        };
        let rendered = render_managed_config(&profile);
        assert!(rendered.contains("nf_conntrack_max=262144\n"));
        assert!(rendered.contains("tcp_mtu_probing=1\n"));
    }
}
