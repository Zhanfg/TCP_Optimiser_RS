use std::fs;
use std::io;
use std::process::Command;

#[derive(Debug, Default)]
pub struct TcpCounters {
    pub retrans: u64,
    pub in_segs: u64,
    pub out_segs: u64,
}

#[derive(Debug, Default)]
pub struct IfaceBytes {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Default)]
pub struct SockStat {
    pub tcp_in_use: u32,
    pub tcp_orphan: u32,
    pub tcp_tw: u32,
    pub tcp_alloc: u32,
    pub tcp_mem: u32,
}

#[derive(Debug)]
pub struct DnsServer {
    pub iface: String,
    pub ip: String,
}

#[derive(Debug, Default)]
pub struct TcpConnInfo {
    pub avg_rtt_ms: f64,
    pub max_rtt_ms: f64,
    pub avg_cwnd: u32,
    pub max_cwnd: u32,
    pub samples: usize,
}

/// Single-call stats: read /proc/net/{snmp,dev,sockstat} in one batch
#[derive(Debug, Default)]
pub struct NetworkSnapshot {
    pub tcp: TcpCounters,
    pub sock: SockStat,
    pub established: u32,
    pub dns: Vec<DnsServer>,
    pub conn_info: Option<TcpConnInfo>,
}

/// Take a full network snapshot with minimal syscalls
pub fn network_snapshot(active_iface: &str) -> io::Result<NetworkSnapshot> {
    let mut snap = NetworkSnapshot::default();

    // Batch read /proc/net/snmp + dev + sockstat in one awk call
    let cmd = format!(
        "awk 'NR==1{{for(i=1;i<=NF;i++){{if($i==\"RetransSegs\")ri=i;if($i==\"InSegs\")ii=i;if($i==\"OutSegs\")oi=i}}}}/^Tcp:/&&NR>1{{print\"retrans:\"$ri\"\\nin:\"$ii\"\\nout:\"$oi}}' /proc/net/snmp; \
         awk -v if=\"{active_iface}\" '$1==if\":\"{{print\"rx:\"$2\"\\ntx:\"$10}}' /proc/net/dev 2>/dev/null; \
         awk '/^TCP:/{{print $3,$5,$7,$9,$11}}' /proc/net/sockstat 2>/dev/null"
    );

    if let Ok(output) = Command::new("sh").arg("-c").arg(&cmd).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with("retrans:") {
                snap.tcp.retrans = line[8..].parse().unwrap_or(0);
            } else if line.starts_with("in:") {
                snap.tcp.in_segs = line[3..].parse().unwrap_or(0);
            } else if line.starts_with("out:") {
                snap.tcp.out_segs = line[4..].parse().unwrap_or(0);
            } else if line.starts_with("rx:") {
                snap.tcp.in_segs = line[3..].parse().unwrap_or(0);
            } else if line.starts_with("tx:") {
                snap.tcp.out_segs = line[3..].parse().unwrap_or(0);
            } else {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    snap.sock.tcp_in_use = parts[0].parse().unwrap_or(0);
                    snap.sock.tcp_orphan = parts[1].parse().unwrap_or(0);
                    snap.sock.tcp_tw = parts[2].parse().unwrap_or(0);
                    snap.sock.tcp_alloc = parts[3].parse().unwrap_or(0);
                    snap.sock.tcp_mem = parts[4].parse().unwrap_or(0);
                }
            }
        }
    }

    // Established connections count
    snap.established = established_conns();

    // DNS
    snap.dns = dns_servers();

    // Connection info
    snap.conn_info = tcp_conn_info();

    Ok(snap)
}

/// Get established connections count via ss
fn established_conns() -> u32 {
    Command::new("ss")
        .args(["-Htn", "state", "established"])
        .output()
        .ok()
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
            if line.contains("dns") {
                if let Some(start) = line.find('[') {
                    if let Some(end) = line.find("]: [") {
                        let val = &line[end + 4..line.len() - 1];
                        if !val.is_empty() && val != "0.0.0.0" && val != "::" {
                            servers.push(DnsServer {
                                iface: line[start + 1..end].replace("net.", "").replace(".dns", ""),
                                ip: val.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    servers
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
        cwnds.iter().sum::<u32>() / cwnds.len() as u32
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
