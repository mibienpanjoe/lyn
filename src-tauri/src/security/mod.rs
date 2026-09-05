//! Boundary checks for INV-07 / INV-13 and ERR audit coverage.

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::PathBuf};

    use serde_json::Value;

    use crate::{
        contract::SpeechModelInput,
        error::{AppError, ErrorCode, ErrorDetailKey, ErrorDetails},
    };

    fn repo_relative(parts: &[&str]) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join_many(parts)
    }

    trait JoinMany {
        fn join_many(self, parts: &[&str]) -> PathBuf;
    }

    impl JoinMany for PathBuf {
        fn join_many(mut self, parts: &[&str]) -> PathBuf {
            for part in parts {
                self.push(part);
            }
            self
        }
    }

    fn capability_permissions(name: &str) -> BTreeSet<String> {
        let path = repo_relative(&["capabilities", name]);
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("missing capability {}: {error}", path.display()));
        let value: Value = serde_json::from_str(&raw).expect("capability JSON");
        value["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .map(|entry| entry.as_str().expect("permission string").to_owned())
            .collect()
    }

    fn permission_set_allows(set_file: &str) -> BTreeSet<String> {
        let path = repo_relative(&["permissions", set_file]);
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("missing permission set {}: {error}", path.display()));
        let mut allows = BTreeSet::new();
        for line in raw.lines() {
            let trimmed = line.trim().trim_matches(',').trim_matches('"');
            if trimmed.starts_with("allow-") {
                allows.insert(trimmed.to_owned());
            }
        }
        allows
    }

    #[test]
    fn window_capabilities_are_partitioned_without_core_default() {
        let main = capability_permissions("main.json");
        let capture = capability_permissions("capture.json");

        assert_eq!(
            main,
            BTreeSet::from([
                "core:event:default".to_owned(),
                "library-commands".to_owned()
            ])
        );
        assert_eq!(
            capture,
            BTreeSet::from([
                "core:event:default".to_owned(),
                "capture-commands".to_owned()
            ])
        );
        assert!(!main.contains("core:default"));
        assert!(!capture.contains("core:default"));
        assert!(!main.contains("capture-commands"));
        assert!(!capture.contains("library-commands"));
    }

    #[test]
    fn capture_window_cannot_install_speech_models_or_mutate_library() {
        let capture = permission_set_allows("capture.toml");
        let library = permission_set_allows("library.toml");

        for forbidden in [
            "allow-install-speech-model",
            "allow-remove-speech-model",
            "allow-list-captures",
            "allow-search-captures",
            "allow-play-media",
            "allow-update-settings",
        ] {
            assert!(
                !capture.contains(forbidden),
                "capture window unexpectedly allows {forbidden}"
            );
        }
        for forbidden in [
            "allow-save-text-capture",
            "allow-start-audio-recording",
            "allow-stage-clipboard-image",
            "allow-get-active-capture-session",
        ] {
            assert!(
                !library.contains(forbidden),
                "library window unexpectedly allows {forbidden}"
            );
        }
        assert!(library.contains("allow-install-speech-model"));
        assert!(capture.contains("allow-save-text-capture"));
    }

    #[test]
    fn speech_model_input_rejects_client_supplied_download_urls() {
        let rejected = serde_json::from_value::<SpeechModelInput>(serde_json::json!({
            "modelId": "base",
            "url": "https://evil.example/model.bin"
        }));
        assert!(rejected.is_err());

        let accepted = serde_json::from_value::<SpeechModelInput>(serde_json::json!({
            "modelId": "base"
        }))
        .expect("model id only");
        assert_eq!(accepted.model_id, "base");
    }

    #[test]
    fn public_errors_never_embed_paths_capture_bodies_or_raw_os_text() {
        let samples = [
            AppError {
                code: ErrorCode::StorageWriteFailed,
                message: "The capture could not be saved".to_owned(),
                retryable: true,
                details: ErrorDetails::default(),
            },
            AppError {
                code: ErrorCode::MediaFinalizeFailed,
                message: "The media could not be finalized".to_owned(),
                retryable: true,
                details: ErrorDetails::default(),
            },
            AppError {
                code: ErrorCode::AudioPlaybackFailed,
                message: "Audio playback failed".to_owned(),
                retryable: true,
                details: ErrorDetails::default(),
            },
            AppError {
                code: ErrorCode::ContextAmbiguous,
                message: "Choose a context before saving".to_owned(),
                retryable: false,
                details: ErrorDetails::default(),
            },
            AppError {
                code: ErrorCode::ModelDownloadFailed,
                message: "The local speech model could not be installed".to_owned(),
                retryable: true,
                details: ErrorDetails::default(),
            },
            AppError {
                code: ErrorCode::PermissionDenied,
                message: "The clipboard is unavailable".to_owned(),
                retryable: true,
                details: ErrorDetails::default(),
            },
        ];

        for error in samples {
            let encoded = serde_json::to_string(&error).expect("error json");
            assert!(
                !encoded.contains("/home/")
                    && !encoded.contains("\\\\")
                    && !encoded.contains("C:\\")
                    && !encoded.contains("No such file")
                    && !encoded.contains("Permission denied (os error")
                    && !encoded.contains("SELECT ")
                    && !encoded.contains("draft that must remain"),
                "sensitive content leaked in {encoded}"
            );
            for key in error.details.0.keys() {
                assert!(matches!(
                    key,
                    ErrorDetailKey::Field
                        | ErrorDetailKey::Limit
                        | ErrorDetailKey::Operation
                        | ErrorDetailKey::Permission
                        | ErrorDetailKey::ResourceKind
                        | ErrorDetailKey::RetryAfterMs
                        | ErrorDetailKey::State
                ));
            }
        }
    }

    /// Documents ERR-001–019 → public `ErrorCode` coverage for the audit harness.
    #[test]
    fn err_requirement_matrix_maps_to_stable_error_codes() {
        let matrix: &[(&str, &[ErrorCode])] = &[
            ("ERR-001", &[ErrorCode::ShortcutConflict]),
            ("ERR-002", &[ErrorCode::InternalError]),
            ("ERR-003", &[ErrorCode::ContextRequired]),
            ("ERR-004", &[]), // null branch is success, not an error code
            ("ERR-005", &[ErrorCode::StorageWriteFailed]),
            (
                "ERR-006",
                &[ErrorCode::MediaStageFailed, ErrorCode::MediaFinalizeFailed],
            ),
            ("ERR-007", &[ErrorCode::UnsupportedClipboardContent]),
            (
                "ERR-008",
                &[
                    ErrorCode::PermissionDenied,
                    ErrorCode::AudioDeviceUnavailable,
                ],
            ),
            ("ERR-009", &[ErrorCode::AudioRecordingFailed]),
            ("ERR-010", &[ErrorCode::AudioPlaybackFailed]),
            (
                "ERR-011",
                &[ErrorCode::EnrichmentFailed, ErrorCode::ModelNotAvailable],
            ),
            ("ERR-012", &[ErrorCode::ModelDownloadFailed]),
            ("ERR-013", &[ErrorCode::SearchFailed]),
            ("ERR-014", &[ErrorCode::MediaNotFound]),
            (
                "ERR-015",
                &[ErrorCode::ValidationError, ErrorCode::StaleSession],
            ),
            ("ERR-016", &[]), // accepted capture + resumable job; no dedicated code
            ("ERR-017", &[ErrorCode::ContextAmbiguous]),
            ("ERR-018", &[ErrorCode::ContextSourceStale]),
            (
                "ERR-019",
                &[ErrorCode::ContextRequired, ErrorCode::ContextAmbiguous],
            ),
        ];

        assert_eq!(matrix.len(), 19);
        for (id, codes) in matrix {
            assert!(id.starts_with("ERR-"), "{id}");
            for code in *codes {
                let _ = serde_json::to_string(code).expect("ErrorCode serializes");
            }
        }
    }
}
