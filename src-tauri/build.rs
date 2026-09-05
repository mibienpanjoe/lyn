fn main() {
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
        "list_captures",
        "get_capture",
        "search_captures",
        "play_media",
        "open_media_external",
        "dismiss_capture_popup",
        "set_capture_popup_layout",
        "get_settings",
        "update_settings",
        "get_speech_model_status",
        "install_speech_model",
        "cancel_speech_model_install",
        "remove_speech_model",
    ];

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
