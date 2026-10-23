//! Only compile-time payloads are admitted. No runtime path/environment input.

#![allow(dead_code)] // Installer consumes this boundary in the next increment.

#[path = "../../../native_artifact.rs"]
mod format;

pub(crate) struct Artifact {
    pub(crate) manifest: format::Manifest,
    pub(crate) bytes: &'static [u8],
}

pub(crate) fn embedded() -> Result<Option<Artifact>, &'static str> {
    #[cfg(lyn_gnome_native_embedded)]
    {
        let manifest = include_bytes!(concat!(env!("OUT_DIR"), "/manifest.json"));
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/liblyn-gnome-native.so"));
        Ok(Some(Artifact {
            manifest: format::validate(manifest, bytes)?,
            bytes,
        }))
    }
    #[cfg(not(lyn_gnome_native_embedded))]
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_payload_matches_build_configuration() {
        let artifact = embedded().unwrap();
        #[cfg(lyn_gnome_native_embedded)]
        {
            let artifact = artifact.expect("explicit bundle was not embedded");
            assert_eq!(artifact.manifest.length, artifact.bytes.len());
            // This path must exercise the actual shared object, not the tiny
            // synthetic ELF header used in the format refusal tests.
            assert!(artifact.bytes.len() > 16 * 1024);
        }
        #[cfg(not(lyn_gnome_native_embedded))]
        assert!(artifact.is_none());
    }
}
