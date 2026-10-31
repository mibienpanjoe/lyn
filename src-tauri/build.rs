mod native_artifact;

fn embed_native_bundle() {
    use std::{fs, io::Read, path::Path};
    println!("cargo:rustc-check-cfg=cfg(lyn_gnome_native_embedded)");
    println!("cargo:rerun-if-env-changed=LYN_GNOME_NATIVE_BUNDLE");
    println!("cargo:rerun-if-changed=native_artifact.rs");
    let Some(directory) = std::env::var_os("LYN_GNOME_NATIVE_BUNDLE") else {
        return;
    };
    println!("cargo:rerun-if-changed={}", Path::new(&directory).display());
    assert_eq!(
        std::env::var("TARGET").unwrap(),
        native_artifact::TARGET,
        "unsupported native bundle target"
    );
    let directory = Path::new(&directory)
        .canonicalize()
        .expect("native bundle directory unavailable");
    let read = |name: &str, limit: usize| {
        let path = directory.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        assert!(
            fs::symlink_metadata(&path)
                .expect("native bundle entry unavailable")
                .is_file(),
            "native bundle requires regular files without symlinks"
        );
        let file = fs::File::open(&path).expect("native bundle file unavailable");
        let metadata = file.metadata().expect("native bundle metadata unavailable");
        assert!(
            metadata.is_file() && metadata.len() <= limit as u64,
            "native bundle requires a bounded regular file"
        );
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .expect("native bundle read failed");
        assert!(bytes.len() <= limit, "native bundle exceeds size limit");
        bytes
    };
    let manifest = read(
        native_artifact::MANIFEST_NAME,
        native_artifact::MAX_MANIFEST_BYTES,
    );
    let module = read(
        native_artifact::MODULE_NAME,
        native_artifact::MAX_MODULE_BYTES,
    );
    native_artifact::validate(&manifest, &module).expect("native bundle refused");
    let output = std::env::var_os("OUT_DIR").unwrap();
    for (name, bytes) in [
        (native_artifact::MODULE_NAME, module),
        (native_artifact::MANIFEST_NAME, manifest),
    ] {
        fs::write(Path::new(&output).join(name), bytes).expect("native bundle output failed");
    }
    println!("cargo:rustc-cfg=lyn_gnome_native_embedded");
}

fn main() {
    embed_native_bundle();
    // Listing commands enables generated allow-/deny- permissions and turns off
    // the default "every registered command is available to every window" behavior.
    // See https://v2.tauri.app/security/capabilities/
    const COMMANDS: &[&str] = &[
        "get_active_capture_session",
        "cancel_capture_session",
        "list_capture_context_sources",
        "select_capture_context_source",
        "save_text_capture",
        "stage_clipboard_image",
        "discard_staged_media",
        "save_image_capture",
        "start_audio_recording",
        "stop_audio_recording",
        "play_staged_audio",
        "stop_audio_playback",
        "save_audio_capture",
        "pick_project_directory",
        "create_context",
        "list_contexts",
        "delete_project",
        "list_captures",
        "get_capture",
        "delete_capture",
        "search_captures",
        "play_media",
        "open_media_external",
        "dismiss_capture_popup",
        "set_capture_popup_layout",
        "get_app_version",
        "get_settings",
        "update_settings",
        "get_integration_statuses",
        "manage_gnome_activation",
        "install_integration",
        "get_speech_model_status",
        "install_speech_model",
        "cancel_speech_model_install",
        "remove_speech_model",
        "retry_voice_transcription",
        "update_voice_caption",
    ];

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
