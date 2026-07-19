use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

const MANIFEST_NAME: &str = "checksums.sha256";
const SIGNATURE_NAME: &str = "checksums.sig";
const PUBLIC_KEY: [u8; 32] = [
    0xb4, 0x48, 0x44, 0x86, 0x1b, 0x8e, 0x4f, 0xb5, 0x4e, 0x44, 0x81, 0xc4, 0x20, 0x1e, 0x6d, 0x79,
    0x55, 0xbe, 0xe0, 0x0b, 0xa8, 0x24, 0x8c, 0x48, 0x37, 0xf7, 0x22, 0x06, 0x3b, 0xce, 0x29, 0xc3,
];
const REQUIRED_FILES: &[&str] = &[
    "module.prop",
    "customize.sh",
    "service.sh",
    "post-fs-data.sh",
    "uninstall.sh",
    "webroot/index.html",
    "bin/arm64-v8a/tcp_optimiser",
    "bin/armeabi-v7a/tcp_optimiser",
    "bin/x86_64/tcp_optimiser",
];

struct ManifestEntry {
    relative_path: PathBuf,
    expected_hash: [u8; 32],
}

pub fn verify_module(root: &Path) -> io::Result<()> {
    let manifest = fs::read(root.join(MANIFEST_NAME))?;
    if manifest.len() > 1024 * 1024 {
        return Err(invalid_data("checksum manifest is too large"));
    }
    let signature_bytes = fs::read(root.join(SIGNATURE_NAME))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| invalid_data("invalid module signature length"))?;
    let verifying_key = VerifyingKey::from_bytes(&PUBLIC_KEY)
        .map_err(|_| invalid_data("invalid embedded verification key"))?;
    verifying_key
        .verify(&manifest, &signature)
        .map_err(|_| invalid_data("module signature verification failed"))?;

    let entries = parse_manifest(&manifest)?;
    let protected = entries
        .iter()
        .map(|entry| entry.relative_path.to_string_lossy().replace('\\', "/"))
        .collect::<HashSet<_>>();
    for required in REQUIRED_FILES {
        if !protected.contains(*required) {
            return Err(invalid_data(format!(
                "checksum manifest is missing required file {required}"
            )));
        }
    }

    for entry in entries {
        let path = root.join(&entry.relative_path);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(invalid_data(format!(
                "protected path is not a regular file: {}",
                entry.relative_path.display()
            )));
        }
        let actual = hash_file(&path)?;
        if actual != entry.expected_hash {
            return Err(invalid_data(format!(
                "module hash mismatch: {}",
                entry.relative_path.display()
            )));
        }
    }
    Ok(())
}

fn parse_manifest(raw: &[u8]) -> io::Result<Vec<ManifestEntry>> {
    let content = std::str::from_utf8(raw)
        .map_err(|_| invalid_data("checksum manifest is not valid UTF-8"))?;
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for line in content.lines() {
        if line.len() < 67 || !matches!(&line[64..66], "  " | " *") {
            return Err(invalid_data("malformed checksum manifest entry"));
        }
        let hash = &line[..64];
        let path = &line[66..];
        let relative_path = safe_relative_path(path)?;
        if !seen.insert(relative_path.clone()) {
            return Err(invalid_data("duplicate checksum manifest path"));
        }
        entries.push(ManifestEntry {
            relative_path,
            expected_hash: decode_sha256(hash)?,
        });
    }
    if entries.is_empty() {
        return Err(invalid_data("checksum manifest is empty"));
    }
    Ok(entries)
}

fn safe_relative_path(value: &str) -> io::Result<PathBuf> {
    if value.is_empty() || value.contains('\\') || value.contains('\0') {
        return Err(invalid_data("unsafe checksum manifest path"));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
        })
        || matches!(value, MANIFEST_NAME | SIGNATURE_NAME)
    {
        return Err(invalid_data("unsafe checksum manifest path"));
    }
    Ok(path.to_path_buf())
}

fn decode_sha256(value: &str) -> io::Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(invalid_data("invalid SHA-256 length"));
    }
    let mut output = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| invalid_data("invalid SHA-256"))?;
        output[index] =
            u8::from_str_radix(text, 16).map_err(|_| invalid_data("invalid SHA-256 encoding"))?;
    }
    Ok(output)
}

fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parser_rejects_traversal_and_duplicates() {
        let hash = "00".repeat(32);
        assert!(parse_manifest(format!("{hash}  ../module.prop\n").as_bytes()).is_err());
        assert!(
            parse_manifest(format!("{hash}  module.prop\n{hash}  module.prop\n").as_bytes())
                .is_err()
        );
    }

    #[test]
    fn manifest_parser_accepts_text_and_binary_sha256_markers() {
        let hash = "00".repeat(32);
        assert_eq!(
            parse_manifest(format!("{hash}  module.prop\n").as_bytes())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            parse_manifest(format!("{hash} *module.prop\n").as_bytes())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn sha256_decoder_is_strict() {
        assert_eq!(decode_sha256(&"00".repeat(32)).unwrap(), [0u8; 32]);
        assert!(decode_sha256("00").is_err());
        assert!(decode_sha256(&"gg".repeat(32)).is_err());
    }
}
