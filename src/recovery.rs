use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::{config, sysctl};

const BASELINE_FILE: &str = ".sysctl-baseline.json";
const BASELINE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct Baseline {
    version: u32,
    values: BTreeMap<String, String>,
}

pub fn capture_baseline() -> io::Result<()> {
    let module_dir = config::module_dir();
    fs::create_dir_all(&module_dir)?;
    let destination = module_dir.join(BASELINE_FILE);

    if destination.is_file() {
        load_baseline(&destination)?;
        return Ok(());
    }

    let mut values = BTreeMap::new();
    for path in sysctl::managed_sysctl_paths() {
        if !Path::new(path).exists() {
            continue;
        }
        if let Ok(value) = sysctl::read_sysctl(path) {
            values.insert(path.to_string(), value);
        }
    }

    if values.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no managed sysctl nodes were readable for baseline capture",
        ));
    }

    let baseline = Baseline {
        version: BASELINE_VERSION,
        values,
    };
    let json = serde_json::to_vec_pretty(&baseline).map_err(io::Error::other)?;
    let temporary = module_dir.join(format!("{BASELINE_FILE}.tmp"));
    fs::write(&temporary, json)?;
    fs::rename(temporary, destination)
}

pub fn restore_baseline() -> io::Result<()> {
    let destination = config::module_dir().join(BASELINE_FILE);
    let baseline = load_baseline(&destination)?;
    let mut failures = Vec::new();

    for (path, value) in baseline.values {
        if !Path::new(&path).exists() {
            continue;
        }
        if let Err(error) = sysctl::write_sysctl(&path, &value) {
            failures.push(format!("{path}: {error}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "baseline restore failed for {} node(s): {}",
            failures.len(),
            failures.join("; ")
        )))
    }
}

fn load_baseline(path: &Path) -> io::Result<Baseline> {
    let content = fs::read(path)?;
    let baseline: Baseline = serde_json::from_slice(&content).map_err(io::Error::other)?;
    validate_baseline(&baseline)?;
    Ok(baseline)
}

fn validate_baseline(baseline: &Baseline) -> io::Result<()> {
    if baseline.version != BASELINE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported sysctl baseline version {}", baseline.version),
        ));
    }

    let allowed = sysctl::managed_sysctl_paths()
        .into_iter()
        .collect::<HashSet<_>>();
    if let Some(path) = baseline
        .values
        .keys()
        .find(|path| !allowed.contains(path.as_str()))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("baseline contains unmanaged sysctl path: {path}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_round_trips_as_json() {
        let baseline = Baseline {
            version: BASELINE_VERSION,
            values: BTreeMap::from([(
                "/proc/sys/net/ipv4/tcp_congestion_control".to_string(),
                "cubic".to_string(),
            )]),
        };
        let json = serde_json::to_vec(&baseline).unwrap();
        let decoded: Baseline = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.version, BASELINE_VERSION);
        assert_eq!(
            decoded.values["/proc/sys/net/ipv4/tcp_congestion_control"],
            "cubic"
        );
        validate_baseline(&decoded).unwrap();
    }

    #[test]
    fn baseline_rejects_unmanaged_paths() {
        let baseline = Baseline {
            version: BASELINE_VERSION,
            values: BTreeMap::from([(
                "/proc/sys/kernel/hostname".to_string(),
                "unexpected".to_string(),
            )]),
        };
        assert!(validate_baseline(&baseline).is_err());
    }
}
