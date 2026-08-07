use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::config;

const BASELINE_FILE: &str = "baseline-v1.json";
const BASELINE_VERSION: u32 = 1;
const BASELINE_PROVENANCE_FILE: &str = "baseline-provenance-v1.json";
const BASELINE_PROVENANCE_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct BaselineDocument {
    version: u32,
    captured_at_epoch: u64,
    sysctls: BTreeMap<String, String>,
    interfaces: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct BaselineProvenanceDocument {
    format_version: u32,
    provenance: String,
    exact_pre_module: bool,
    note: String,
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
    pub provenance: String,
    pub exact_pre_module: bool,
    pub provenance_note: String,
}

/// Read and validate baseline metadata without creating or modifying the file.
/// This is intentionally separate from `capture-baseline`: opening the WebUI
/// must never manufacture rollback evidence after tuning has already started.
pub fn read() -> io::Result<BaselineStatus> {
    let module_dir = config::module_dir();
    let path = module_dir.join(BASELINE_FILE);
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

    let provenance = read_provenance(&module_dir.join(BASELINE_PROVENANCE_FILE))?;
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
        provenance: provenance.provenance,
        exact_pre_module: provenance.exact_pre_module,
        provenance_note: provenance.note,
    })
}

fn read_provenance(path: &Path) -> io::Result<BaselineProvenanceDocument> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(BaselineProvenanceDocument {
                format_version: BASELINE_PROVENANCE_VERSION,
                provenance: "exact_pre_module".to_string(),
                exact_pre_module: true,
                note: "Legacy transactional baseline without a provenance sidecar; treated as exact pre-module evidence.".to_string(),
            });
        }
        Err(error) => return Err(error),
    };
    decode_provenance(&bytes)
}

fn decode_provenance(bytes: &[u8]) -> io::Result<BaselineProvenanceDocument> {
    let document: BaselineProvenanceDocument = serde_json::from_slice(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid baseline provenance: {error}"),
        )
    })?;
    if document.format_version != BASELINE_PROVENANCE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported baseline provenance format {}",
                document.format_version
            ),
        ));
    }
    if !matches!(
        document.provenance.as_str(),
        "exact_pre_module" | "legacy_upgrade_snapshot"
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown baseline provenance {}", document.provenance),
        ));
    }
    if document.exact_pre_module != (document.provenance == "exact_pre_module") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "baseline provenance flag is inconsistent",
        ));
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::{decode_provenance, BaselineDocument, BASELINE_VERSION};

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

    #[test]
    fn provenance_schema_distinguishes_legacy_upgrade_snapshot() {
        let document = decode_provenance(
            br#"{
                "format_version": 1,
                "provenance": "legacy_upgrade_snapshot",
                "exact_pre_module": false,
                "note": "compatibility"
            }"#,
        )
        .unwrap();
        assert_eq!(document.provenance, "legacy_upgrade_snapshot");
        assert!(!document.exact_pre_module);
    }

    #[test]
    fn provenance_schema_rejects_inconsistent_exact_flag() {
        assert!(decode_provenance(
            br#"{
                "format_version": 1,
                "provenance": "legacy_upgrade_snapshot",
                "exact_pre_module": true,
                "note": "invalid"
            }"#,
        )
        .is_err());
    }
}
