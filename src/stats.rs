use std::fs;
use std::io;
use std::net::IpAddr;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct TcpCounters {
    pub retrans: u64,
    pub in_segs: u64,
    pub out_segs: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct IfaceBytes {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct SockStat {
    pub tcp_in_use: u32,
    pub tcp_orphan: u32,
    pub tcp_tw: u32,
    pub tcp_alloc: u32,
    pub tcp_mem: u32,
}

#[derive(Debug, Serialize)]
pub struct DnsServer {
    pub iface: String,
    pub ip: String,
}

#[derive(Debug, Default, Serialize)]
pub struct TcpConnInfo {
    pub avg_rtt_ms: f64,
    pub max_rtt_ms: f64,
    pub avg_cwnd: u32,
    pub max_cwnd: u32,
    pub samples: usize,
}

/// Single-call stats: read /proc/net/{snmp,dev,sockstat} in one batch
#[derive(Debug, Serialize)]
pub struct NetworkSnapshot {
    pub build: crate::build_info::BuildInfo,
    pub active_iface: String,
    pub module_active: bool,
    pub algorithm: String,
    pub default_qdisc: String,
    pub available_algorithms: Vec<String>,
    pub proxy: String,
    pub hosts: String,
    pub init_windows: Vec<u32>,
    pub tcp: Option<TcpCounters>,
    pub iface: Option<IfaceBytes>,
    pub sock: Option<SockStat>,
    pub established: Option<u32>,
    pub dns: Option<Vec<DnsServer>>,
    pub conn_info: Option<TcpConnInfo>,
    pub verification: crate::policy::VerificationSnapshot,
}

/// Take a full network snapshot with minimal syscalls
pub fn network_snapshot(active_iface: &str, include_stats: bool) -> io::Result<NetworkSnapshot> {
    Ok(NetworkSnapshot {
        build: crate::build_info::current(),
        active_iface: active_iface.to_string(),
        module_active: crate::daemon::is_running(),
        algorithm: crate::sysctl::current_algorithm().unwrap_or_else(|_| "unknown".to_string()),
        default_qdisc: crate::sysctl::default_qdisc().unwrap_or_else(|_| "unknown".to_string()),
        available_algorithms: crate::sysctl::available_algorithms().unwrap_or_default(),
        proxy: crate::proxy::detect_proxy().label().to_string(),
        hosts: crate::proxy::detect_hosts().key(),
        init_windows: crate::network::get_initcwnd_initrwnd().unwrap_or_default(),
        tcp: include_stats
            .then(|| {
                fs::read_to_string("/proc/net/snmp")
                    .and_then(|content| parse_tcp_snmp(&content))
                    .ok()
            })
            .flatten(),
        iface: include_stats
            .then(|| {
                fs::read_to_string("/proc/net/dev")
                    .and_then(|content| parse_iface_bytes(&content, active_iface))
                    .ok()
            })
            .flatten(),
        sock: include_stats
            .then(|| {
                fs::read_to_string("/proc/net/sockstat")
                    .and_then(|content| parse_sockstat(&content))
                    .ok()
            })
            .flatten(),
        established: include_stats.then(established_conns),
        dns: include_stats.then(dns_servers),
        conn_info: include_stats.then(tcp_conn_info).flatten(),
        verification: crate::policy::verify_policy(active_iface),
    })
}

fn parse_tcp_snmp(content: &str) -> io::Result<TcpCounters> {
    let mut tcp_lines = content.lines().filter(|line| line.starts_with("Tcp:"));
    let headers: Vec<&str> = tcp_lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing TCP SNMP header"))?
        .split_whitespace()
        .skip(1)
        .collect();
    let values: Vec<&str> = tcp_lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing TCP SNMP values"))?
        .split_whitespace()
        .skip(1)
        .collect();
    if headers.len() != values.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TCP SNMP header/value length mismatch",
        ));
    }
    let get = |name: &str| -> io::Result<u64> {
        let index = headers
            .iter()
            .position(|header| *header == name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing TCP counter {name}"),
                )
            })?;
        values[index].parse::<u64>().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid TCP counter {name}: {error}"),
            )
        })
    };
    Ok(TcpCounters {
        retrans: get("RetransSegs")?,
        in_segs: get("InSegs")?,
        out_segs: get("OutSegs")?,
    })
}

fn parse_iface_bytes(content: &str, active_iface: &str) -> io::Result<IfaceBytes> {
    for line in content.lines() {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != active_iface {
            continue;
        }
        let fields: Vec<&str> = values.split_whitespace().collect();
        if fields.len() < 9 {
            break;
        }
        return Ok(IfaceBytes {
            rx_bytes: parse_u64(fields[0], "interface rx_bytes")?,
            tx_bytes: parse_u64(fields[8], "interface tx_bytes")?,
        });
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("interface {active_iface} missing from /proc/net/dev"),
    ))
}

fn parse_sockstat(content: &str) -> io::Result<SockStat> {
    let line = content
        .lines()
        .find(|line| line.starts_with("TCP:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing TCP sockstat line"))?;
    let fields: Vec<&str> = line.split_whitespace().skip(1).collect();
    let get = |name: &str| -> io::Result<u32> {
        let index = fields
            .iter()
            .position(|field| *field == name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing sockstat field {name}"),
                )
            })?;
        fields
            .get(index + 1)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing value for {name}"),
                )
            })?
            .parse::<u32>()
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid {name}: {error}"),
                )
            })
    };
    Ok(SockStat {
        tcp_in_use: get("inuse")?,
        tcp_orphan: get("orphan")?,
        tcp_tw: get("tw")?,
        tcp_alloc: get("alloc")?,
        tcp_mem: get("mem")?,
    })
}

fn parse_u64(value: &str, name: &str) -> io::Result<u64> {
    value.parse::<u64>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {name}: {error}"),
        )
    })
}

/// Get established connections count via ss
fn established_conns() -> u32 {
    Command::new("ss")
        .args(["-Htn", "state", "established"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.lines().count() as u32
        })
        .unwrap_or(0)
}

/// Get DNS servers from system properties
fn dns_servers() -> Vec<DnsServer> {
    let mut servers = Vec::new();
    if let Ok(output) = Command::new("getprop").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(server) = parse_dns_property(line) {
                servers.push(server);
            }
        }
    }
    servers
}

fn parse_dns_property(line: &str) -> Option<DnsServer> {
    let (raw_key, raw_value) = line.split_once("]: [")?;
    let key = raw_key.strip_prefix('[')?;
    let property = key.strip_prefix("net.")?;
    let (iface, dns_key) = property
        .rsplit_once('.')
        .map_or(("system", property), |(iface, key)| (iface, key));
    let slot = dns_key.strip_prefix("dns")?;
    if slot.is_empty() || !slot.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let value = raw_value.strip_suffix(']')?.trim();
    let address = value.parse::<IpAddr>().ok()?;
    if address.is_unspecified() {
        return None;
    }
    Some(DnsServer {
        iface: iface.to_string(),
        ip: address.to_string(),
    })
}

/// Get per-connection TCP info from ss
fn tcp_conn_info() -> Option<TcpConnInfo> {
    let output = Command::new("ss").args(["-tino"]).output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rtts = Vec::new();
    let mut cwnds = Vec::new();

    for line in stdout.lines() {
        if line.contains("LISTEN") || line.contains("CLOSE-WAIT") || line.contains("TIME-WAIT") {
            continue;
        }
        if let Some(cwnd) = extract_ss_val(line, "cwnd:") {
            cwnds.push(cwnd);
        }
        if let Some(rtt) = extract_ss_val_f64(line, "rtt:") {
            rtts.push(rtt);
        }
    }

    if rtts.is_empty() {
        return None;
    }

    let avg_rtt = rtts.iter().sum::<f64>() / rtts.len() as f64;
    let max_rtt = rtts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let avg_cwnd = if !cwnds.is_empty() {
        (cwnds.iter().map(|value| u64::from(*value)).sum::<u64>() / cwnds.len() as u64) as u32
    } else {
        0
    };
    let max_cwnd = cwnds.iter().cloned().max().unwrap_or(0);

    Some(TcpConnInfo {
        avg_rtt_ms: (avg_rtt * 100.0).round() / 100.0,
        max_rtt_ms: (max_rtt * 100.0).round() / 100.0,
        avg_cwnd,
        max_cwnd,
        samples: rtts.len(),
    })
}

fn extract_ss_val(line: &str, prefix: &str) -> Option<u32> {
    let start = line.find(prefix)? + prefix.len();
    let end = line[start..]
        .find(|c: char| c.is_whitespace())
        .map(|i| start + i)
        .unwrap_or(line.len());
    line[start..end].parse().ok()
}

fn extract_ss_val_f64(line: &str, prefix: &str) -> Option<f64> {
    let start = line.find(prefix)? + prefix.len();
    let end = line[start..]
        .find(|c: char| c == '/' || c.is_whitespace())
        .map(|i| start + i)
        .unwrap_or(line.len());
    line[start..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNMP: &str = "Tcp: RtoAlgorithm InSegs OutSegs RetransSegs\nTcp: 1 120 90 3\n";
    const DEV: &str =
        "Inter-| Receive | Transmit\n wlan0: 1000 1 2 3 4 5 6 7 2000 9 10 11 12 13 14 15\n";
    const SOCK: &str = "sockets: used 100\nTCP: inuse 8 orphan 2 tw 3 alloc 10 mem 4\n";

    #[test]
    fn keeps_tcp_segments_separate_from_interface_bytes() {
        let tcp = parse_tcp_snmp(SNMP).unwrap();
        let iface = parse_iface_bytes(DEV, "wlan0").unwrap();
        assert_eq!((tcp.in_segs, tcp.out_segs, tcp.retrans), (120, 90, 3));
        assert_eq!((iface.rx_bytes, iface.tx_bytes), (1000, 2000));
    }

    #[test]
    fn parses_named_sockstat_fields() {
        let sock = parse_sockstat(SOCK).unwrap();
        assert_eq!(
            (
                sock.tcp_in_use,
                sock.tcp_orphan,
                sock.tcp_tw,
                sock.tcp_alloc,
                sock.tcp_mem
            ),
            (8, 2, 3, 10, 4)
        );
    }

    #[test]
    fn dns_parser_rejects_malformed_properties_without_panicking() {
        let system = parse_dns_property("[net.dns1]: [8.8.8.8]").unwrap();
        assert_eq!(
            (system.iface.as_str(), system.ip.as_str()),
            ("system", "8.8.8.8")
        );
        let wifi = parse_dns_property("[net.wlan0.dns2]: [2001:4860:4860::8888]").unwrap();
        assert_eq!(wifi.iface, "wlan0");
        assert!(parse_dns_property("[net.dns1]: [").is_none());
        assert!(parse_dns_property("[persist.sys.private_dns_mode]: [opportunistic]").is_none());
        assert!(parse_dns_property("[init.svc.dnsmasq]: [running]").is_none());
        assert!(parse_dns_property("[net.dns1]: [0.0.0.0]").is_none());
        assert!(parse_dns_property("garbage").is_none());
    }
}
