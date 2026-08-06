use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;

use crate::config;

const BASELINE_FILE: &str = "baseline-v1.json";
const BASELINE_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct BaselineDocument {
    version: u32,
    captured_at_epoch: u64,
    sysctls: BTreeMap<String, String>,
    interfaces: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct BaselineStatus {
    pub healthy: bool,
    pub version: u32,
    pub captured_at_epoch: u64,
    pub sysctl_count: usize,
    pub interface_count: usize,
    pub interface_names: Vec<String>,
    pub file_size_bytes: u64,
    pub path: String,
}

/// Read and validate baseline metadata without creating or modifying the file.
/// This is intentionally separate from `capture-baseline`: opening the WebUI
/// must never manufacture rollback evidence after tuning has already started.
pub fn read() -> io::Result<BaselineStatus> {
    let path = config::module_dir().join(BASELINE_FILE);
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("baseline path is not a regular file: {}", path.display()),
        ));
    }

    let bytes = fs::read(&path)?;
    let document: BaselineDocument = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid baseline file {}: {error}", path.display()),
        )
    })?;
    if document.version != BASELINE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported baseline version {}, expected {}",
                document.version, BASELINE_VERSION
            ),
        ));
    }
    if document.captured_at_epoch == 0 || document.sysctls.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "baseline is missing capture time or managed sysctl values",
        ));
    }

    let mut interface_names = document.interfaces.keys().cloned().collect::<Vec<_>>();
    interface_names.sort();
    Ok(BaselineStatus {
        healthy: true,
        version: document.version,
        captured_at_epoch: document.captured_at_epoch,
        sysctl_count: document.sysctls.len(),
        interface_count: interface_names.len(),
        interface_names,
        file_size_bytes: metadata.len(),
        path: path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{BaselineDocument, BASELINE_VERSION};

    #[test]
    fn baseline_status_schema_accepts_transactional_document() {
        let document: BaselineDocument = serde_json::from_str(
            r#"{
                "version": 1,
                "captured_at_epoch": 123,
                "sysctls": {"/proc/sys/net/ipv4/tcp_ecn": "2"},
                "interfaces": {"wlan0": {"root_qdisc": "fq_codel"}}
            }"#,
        )
        .unwrap();
        assert_eq!(document.version, BASELINE_VERSION);
        assert_eq!(document.sysctls.len(), 1);
        assert!(document.interfaces.contains_key("wlan0"));
    }
}
