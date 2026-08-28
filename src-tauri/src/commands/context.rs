use std::{collections::BTreeMap, sync::Mutex, time::Instant};

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    context::{DirectorySelectionRegistry, inspect_project_directory},
    contract::{
        CreateContextInput, CreateContextResult, ListContextsInput, ListContextsResult,
        PickProjectDirectoryResult,
    },
    error::{AppError, CommandResult, ErrorCode, ErrorDetailKey, ErrorDetailValue, ErrorDetails},
    storage::{Database, contexts::ContextRepository},
};

const MAX_CONTEXT_NAME_CHARS: usize = 100;
const MAX_CONTEXT_QUERY_CHARS: usize = 100;

#[tauri::command]
pub(crate) async fn pick_project_directory(
    app: AppHandle,
    selections: State<'_, Mutex<DirectorySelectionRegistry>>,
) -> Result<CommandResult<PickProjectDirectoryResult>, ()> {
    #[cfg(desktop)]
    let selected = app
        .dialog()
        .file()
        .set_title("Choose a project directory")
        .blocking_pick_folder();

    #[cfg(not(desktop))]
    let selected: Option<tauri_plugin_dialog::FilePath> = None;

    let Some(selected) = selected else {
        return Ok(CommandResult::success(PickProjectDirectoryResult {
            selection: None,
        }));
    };
    let Ok(path) = selected.into_path() else {
        return Ok(CommandResult::failure(permission_error()));
    };
    let Ok(mut selections) = selections.lock() else {
        return Ok(CommandResult::failure(internal_error()));
    };
    match selections.issue(&path, Instant::now()) {
        Ok(selection) => Ok(CommandResult::success(PickProjectDirectoryResult {
            selection: Some(selection),
        })),
        Err(_) => Ok(CommandResult::failure(permission_error())),
    }
}

#[tauri::command]
pub(crate) fn create_context(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
    selections: State<'_, Mutex<DirectorySelectionRegistry>>,
) -> CommandResult<CreateContextResult> {
    create_context_value_at(input, database.inner(), selections.inner(), Instant::now())
}

fn create_context_value_at(
    input: serde_json::Value,
    database: &Mutex<Database>,
    selections: &Mutex<DirectorySelectionRegistry>,
    now: Instant,
) -> CommandResult<CreateContextResult> {
    let Ok(input) = serde_json::from_value(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    create_context_at(input, database, selections, now)
}

fn create_context_at(
    input: CreateContextInput,
    database: &Mutex<Database>,
    selections: &Mutex<DirectorySelectionRegistry>,
    now: Instant,
) -> CommandResult<CreateContextResult> {
    let result = match input {
        CreateContextInput::Standalone { name } => {
            let name = match validate_context_name(&name) {
                Ok(name) => name,
                Err(error) => return CommandResult::failure(error),
            };
            let Ok(database) = database.lock() else {
                return CommandResult::failure(internal_error());
            };
            ContextRepository::new(database.connection()).create_standalone(name)
        }
        CreateContextInput::Project {
            name,
            selected_directory_token,
        } => {
            let name = match validate_context_name(&name) {
                Ok(name) => name,
                Err(error) => return CommandResult::failure(error),
            };
            let selected_path = {
                let Ok(mut selections) = selections.lock() else {
                    return CommandResult::failure(internal_error());
                };
                match selections.consume(selected_directory_token, now) {
                    Ok(path) => path,
                    Err(_) => return CommandResult::failure(permission_error()),
                }
            };
            let identity = match inspect_project_directory(&selected_path) {
                Ok(identity) => identity,
                Err(_) => return CommandResult::failure(permission_error()),
            };
            let Ok(database) = database.lock() else {
                return CommandResult::failure(internal_error());
            };
            ContextRepository::new(database.connection()).create_project(
                name,
                identity.project_key.as_deref(),
                &identity.project_path,
            )
        }
    };

    match result {
        Ok(context) => CommandResult::success(CreateContextResult { context }),
        Err(_) => CommandResult::failure(storage_write_error()),
    }
}

#[tauri::command]
pub(crate) fn list_contexts(
    input: serde_json::Value,
    database: State<'_, Mutex<Database>>,
) -> CommandResult<ListContextsResult> {
    list_contexts_value(input, database.inner())
}

fn list_contexts_value(
    input: serde_json::Value,
    database: &Mutex<Database>,
) -> CommandResult<ListContextsResult> {
    let Ok(input) = serde_json::from_value(input) else {
        return CommandResult::failure(validation_error("input"));
    };
    list_contexts_impl(input, database)
}

fn list_contexts_impl(
    input: ListContextsInput,
    database: &Mutex<Database>,
) -> CommandResult<ListContextsResult> {
    if !(1..=100).contains(&input.limit) {
        return CommandResult::failure(validation_error("limit"));
    }
    if let Some(query) = input.query.as_deref()
        && (query.chars().count() > MAX_CONTEXT_QUERY_CHARS || query.chars().any(char::is_control))
    {
        return CommandResult::failure(validation_error("query"));
    }

    let Ok(database) = database.lock() else {
        return CommandResult::failure(internal_error());
    };
    match ContextRepository::new(database.connection()).list(
        input.kind,
        input.query.as_deref(),
        input.limit,
    ) {
        Ok(contexts) => CommandResult::success(ListContextsResult { contexts }),
        Err(_) => CommandResult::failure(storage_unavailable_error()),
    }
}

fn validate_context_name(name: &str) -> Result<&str, AppError> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().count() > MAX_CONTEXT_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(validation_error("name"));
    }
    Ok(name)
}

fn details(key: ErrorDetailKey, value: ErrorDetailValue) -> ErrorDetails {
    ErrorDetails(BTreeMap::from([(key, value)]))
}

fn validation_error(field: &str) -> AppError {
    AppError {
        code: ErrorCode::ValidationError,
        message: "The context request is invalid".to_owned(),
        retryable: false,
        details: details(
            ErrorDetailKey::Field,
            ErrorDetailValue::String(field.to_owned()),
        ),
    }
}

fn permission_error() -> AppError {
    AppError {
        code: ErrorCode::PermissionDenied,
        message: "Choose the project directory again".to_owned(),
        retryable: true,
        details: details(
            ErrorDetailKey::ResourceKind,
            ErrorDetailValue::String("project_directory".to_owned()),
        ),
    }
}

fn storage_write_error() -> AppError {
    AppError {
        code: ErrorCode::StorageWriteFailed,
        message: "The context could not be saved".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn storage_unavailable_error() -> AppError {
    AppError {
        code: ErrorCode::StorageUnavailable,
        message: "Contexts are temporarily unavailable".to_owned(),
        retryable: true,
        details: ErrorDetails::default(),
    }
}

fn internal_error() -> AppError {
    AppError {
        code: ErrorCode::InternalError,
        message: "Lyn could not complete the request".to_owned(),
        retryable: false,
        details: ErrorDetails::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Instant};

    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        context::DirectorySelectionRegistry,
        contract::{CreateContextInput, DirectorySelectionToken, ListContextsInput},
        error::{CommandResult, ErrorCode},
        storage::Database,
    };

    use super::{
        create_context_at, create_context_value_at, list_contexts_impl, list_contexts_value,
    };

    #[test]
    fn standalone_context_can_be_created_and_listed() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let selections = Mutex::new(DirectorySelectionRegistry::default());

        let created = create_context_at(
            CreateContextInput::Standalone {
                name: "Notes".to_owned(),
            },
            &database,
            &selections,
            Instant::now(),
        );
        let listed = list_contexts_impl(
            ListContextsInput {
                kind: None,
                query: None,
                limit: 100,
            },
            &database,
        );

        let CommandResult::Success { data: created, .. } = created else {
            panic!("creation failed")
        };
        let CommandResult::Success { data: listed, .. } = listed else {
            panic!("listing failed")
        };
        assert_eq!(listed.contexts, vec![created.context]);
    }

    #[test]
    fn invalid_names_and_limits_return_stable_validation_errors() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let selections = Mutex::new(DirectorySelectionRegistry::default());

        let invalid_name = create_context_at(
            CreateContextInput::Standalone {
                name: "   ".to_owned(),
            },
            &database,
            &selections,
            Instant::now(),
        );
        let invalid_limit = list_contexts_impl(
            ListContextsInput {
                kind: None,
                query: None,
                limit: 101,
            },
            &database,
        );

        let CommandResult::Failure { error, .. } = invalid_name else {
            panic!("invalid name succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
        let CommandResult::Failure { error, .. } = invalid_limit else {
            panic!("invalid limit succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
    }

    #[test]
    fn forged_or_replayed_directory_tokens_are_rejected() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let selections = Mutex::new(DirectorySelectionRegistry::default());
        let directory = tempdir().unwrap();
        let now = Instant::now();

        let forged = create_context_at(
            CreateContextInput::Project {
                name: "Forged".to_owned(),
                selected_directory_token: DirectorySelectionToken::new(),
            },
            &database,
            &selections,
            now,
        );
        let token = selections
            .lock()
            .unwrap()
            .issue(directory.path(), now)
            .unwrap();
        let input = CreateContextInput::Project {
            name: "Project".to_owned(),
            selected_directory_token: token.selected_directory_token,
        };
        let created = create_context_at(input.clone(), &database, &selections, now);
        let replayed = create_context_at(input, &database, &selections, now);

        let CommandResult::Failure {
            error: forged_error,
            ..
        } = forged
        else {
            panic!("forged token succeeded")
        };
        assert_eq!(forged_error.code, ErrorCode::PermissionDenied);
        assert!(matches!(created, CommandResult::Success { .. }));
        let CommandResult::Failure {
            error: replay_error,
            ..
        } = replayed
        else {
            panic!("replayed token succeeded")
        };
        assert_eq!(replay_error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn malformed_ipc_is_a_stable_error_without_storage_mutation() {
        let database = Mutex::new(Database::open_in_memory().unwrap());
        let selections = Mutex::new(DirectorySelectionRegistry::default());
        let malformed = create_context_value_at(
            json!({
                "kind": "project",
                "name": "Private project",
                "selectedDirectoryToken": "not-a-uuid",
                "projectPath": "/private/work"
            }),
            &database,
            &selections,
            Instant::now(),
        );
        let unknown_list_field = list_contexts_value(
            json!({ "kind": null, "query": null, "limit": 100, "offset": 0 }),
            &database,
        );
        let listed = list_contexts_impl(
            ListContextsInput {
                kind: None,
                query: None,
                limit: 100,
            },
            &database,
        );

        let CommandResult::Failure { error, .. } = malformed else {
            panic!("malformed create input succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
        let CommandResult::Failure { error, .. } = unknown_list_field else {
            panic!("malformed list input succeeded")
        };
        assert_eq!(error.code, ErrorCode::ValidationError);
        let CommandResult::Success { data, .. } = listed else {
            panic!("listing failed")
        };
        assert!(data.contexts.is_empty());
    }
}
