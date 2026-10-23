//! Shared build/runtime bundle format. No filesystem or module loading here.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_MODULE_BYTES: usize = 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 4096;
pub const MODULE_NAME: &str = "liblyn-gnome-native.so";
pub const MANIFEST_NAME: &str = "manifest.json";
pub const TARGET: &str = "x86_64-unknown-linux-gnu";

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub protocol: u32,
    pub target: String,
    pub gtk: [u32; 3],
    pub vte: [u32; 3],
    pub length: usize,
    pub sha256: String,
}

pub fn validate(manifest: &[u8], module: &[u8]) -> Result<Manifest, &'static str> {
    if manifest.len() > MAX_MANIFEST_BYTES || module.len() > MAX_MODULE_BYTES {
        return Err("native bundle exceeds size limit");
    }
    let manifest: Manifest =
        serde_json::from_slice(manifest).map_err(|_| "invalid native manifest")?;
    if manifest.version != 1
        || manifest.protocol != 1
        || manifest.target != TARGET
        || manifest.gtk != [3, 24, 33]
        || manifest.vte != [0, 68, 0]
        || manifest.length != module.len()
    {
        return Err("unsupported native manifest");
    }
    let digest: String = Sha256::digest(module)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if manifest.sha256 != digest {
        return Err("native digest mismatch");
    }
    // Deliberately a header/target fence, not a full ELF loader or code audit.
    if module.len() < 64
        || &module[..7] != b"\x7fELF\x02\x01\x01"
        || module[16..18] != 3_u16.to_le_bytes()
        || module[18..20] != 62_u16.to_le_bytes()
        || module[20..24] != 1_u32.to_le_bytes()
        || module[52..54] != 64_u16.to_le_bytes()
        || module
            .windows(b"LYN_GNOME_TEST_".len())
            .any(|part| part == b"LYN_GNOME_TEST_")
    {
        return Err("unsupported native ELF or diagnostic module");
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn reference() -> (Manifest, Vec<u8>) {
        let mut module = vec![0; 64];
        module[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        module[16..18].copy_from_slice(&3_u16.to_le_bytes());
        module[18..20].copy_from_slice(&62_u16.to_le_bytes());
        module[20..24].copy_from_slice(&1_u32.to_le_bytes());
        module[52..54].copy_from_slice(&64_u16.to_le_bytes());
        let manifest = Manifest {
            version: 1,
            protocol: 1,
            target: TARGET.into(),
            gtk: [3, 24, 33],
            vte: [0, 68, 0],
            length: module.len(),
            sha256: digest(&module),
        };
        (manifest, module)
    }

    #[test]
    fn reference_header_and_closed_manifest_are_accepted() {
        let (manifest, module) = reference();
        assert_eq!(
            validate(&serde_json::to_vec(&manifest).unwrap(), &module),
            Ok(manifest)
        );
    }

    #[test]
    fn manifest_versions_target_stack_digest_and_length_are_fenced() {
        let (manifest, module) = reference();
        let original = serde_json::to_value(manifest).unwrap();
        for (key, value) in [
            ("version", serde_json::json!(2)),
            ("protocol", serde_json::json!(0)),
            ("target", serde_json::json!("aarch64-unknown-linux-gnu")),
            ("gtk", serde_json::json!([3, 24, 34])),
            ("vte", serde_json::json!([0, 70, 0])),
            ("length", serde_json::json!(65)),
            ("sha256", serde_json::json!("0".repeat(64))),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut changed = original.clone();
            changed[key] = value;
            assert!(
                validate(&serde_json::to_vec(&changed).unwrap(), &module).is_err(),
                "{key}"
            );
        }
        let duplicate =
            serde_json::to_string(&original)
                .unwrap()
                .replacen('{', "{\"version\":1,", 1);
        assert!(validate(duplicate.as_bytes(), &module).is_err());
    }

    #[test]
    fn changed_elf_headers_and_diagnostic_builds_refuse_even_with_matching_hash() {
        let (mut manifest, module) = reference();
        for offset in [0, 4, 5, 6, 16, 18, 20, 52] {
            let mut changed = module.clone();
            changed[offset] ^= 1;
            manifest.sha256 = digest(&changed);
            assert!(
                validate(&serde_json::to_vec(&manifest).unwrap(), &changed).is_err(),
                "{offset}"
            );
        }
        let mut diagnostic = module;
        diagnostic.extend_from_slice(b"LYN_GNOME_TEST_METRICS");
        manifest.length = diagnostic.len();
        manifest.sha256 = digest(&diagnostic);
        assert!(validate(&serde_json::to_vec(&manifest).unwrap(), &diagnostic).is_err());
    }

    #[test]
    fn truncated_and_oversized_inputs_refuse() {
        let (manifest, module) = reference();
        let encoded = serde_json::to_vec(&manifest).unwrap();
        for length in 0..64 {
            assert!(validate(&encoded, &module[..length]).is_err());
        }
        assert!(validate(&vec![b' '; MAX_MANIFEST_BYTES + 1], &module).is_err());
        assert!(validate(&encoded, &vec![0; MAX_MODULE_BYTES + 1]).is_err());
    }
}
