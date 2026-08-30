//! Context providers and invocation-bound resolution.

pub(crate) mod provider;
pub(crate) mod session_registry;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::contract::{DirectorySelectionToken, SelectedProjectDirectory};

const DIRECTORY_TOKEN_TTL: Duration = Duration::from_secs(5 * 60);

struct PendingDirectorySelection {
    path: PathBuf,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionTokenError {
    DirectoryUnavailable,
    UnknownOrExpired,
}

#[derive(Default)]
pub(crate) struct DirectorySelectionRegistry {
    pending: HashMap<DirectorySelectionToken, PendingDirectorySelection>,
}

impl DirectorySelectionRegistry {
    pub(crate) fn issue(
        &mut self,
        selected_path: &Path,
        now: Instant,
    ) -> Result<SelectedProjectDirectory, SelectionTokenError> {
        let canonical_path = fs::canonicalize(selected_path)
            .map_err(|_| SelectionTokenError::DirectoryUnavailable)?;
        if !canonical_path.is_dir() {
            return Err(SelectionTokenError::DirectoryUnavailable);
        }

        self.pending
            .retain(|_, selection| selection.expires_at > now);
        let selected_directory_token = DirectorySelectionToken::new();
        let suggested_name = safe_suggested_name(&canonical_path);
        self.pending.insert(
            selected_directory_token,
            PendingDirectorySelection {
                path: canonical_path,
                expires_at: now + DIRECTORY_TOKEN_TTL,
            },
        );

        Ok(SelectedProjectDirectory {
            selected_directory_token,
            suggested_name,
        })
    }

    pub(crate) fn consume(
        &mut self,
        token: DirectorySelectionToken,
        now: Instant,
    ) -> Result<PathBuf, SelectionTokenError> {
        let selection = self
            .pending
            .remove(&token)
            .ok_or(SelectionTokenError::UnknownOrExpired)?;
        if selection.expires_at <= now {
            return Err(SelectionTokenError::UnknownOrExpired);
        }

        let revalidated = fs::canonicalize(&selection.path)
            .map_err(|_| SelectionTokenError::DirectoryUnavailable)?;
        if revalidated != selection.path || !revalidated.is_dir() {
            return Err(SelectionTokenError::DirectoryUnavailable);
        }
        Ok(revalidated)
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

pub(crate) struct ProjectDirectoryIdentity {
    pub(crate) project_key: Option<String>,
    pub(crate) project_path: String,
    #[allow(dead_code, reason = "consumed by the live-source registry in T14")]
    pub(crate) worktree_path: Option<String>,
    #[allow(dead_code, reason = "consumed by the live-source registry in T14")]
    pub(crate) branch_name: Option<String>,
}

pub(crate) fn inspect_project_directory(
    selected_path: &Path,
) -> Result<ProjectDirectoryIdentity, SelectionTokenError> {
    let project_path = selected_path
        .to_str()
        .ok_or(SelectionTokenError::DirectoryUnavailable)?
        .to_owned();
    let git_common_directory = git_output(
        selected_path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .and_then(|output| fs::canonicalize(output).ok())
    .and_then(|path| path.to_str().map(str::to_owned));
    let worktree_path = git_output(
        selected_path,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
    )
    .and_then(|output| fs::canonicalize(output).ok())
    .and_then(|path| path.to_str().map(str::to_owned));
    let branch_name = git_output(
        selected_path,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    );

    Ok(ProjectDirectoryIdentity {
        project_key: git_common_directory,
        project_path,
        worktree_path,
        branch_name,
    })
}

fn git_output(directory: &Path, arguments: &[&str]) -> Option<String> {
    Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
}

fn safe_suggested_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let safe: String = name
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(100)
        .collect();
    if safe.is_empty() {
        "Project".to_owned()
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        process::Command,
        time::{Duration, Instant},
    };

    use tempfile::tempdir;

    use super::{
        DIRECTORY_TOKEN_TTL, DirectorySelectionRegistry, SelectionTokenError,
        inspect_project_directory,
    };

    #[test]
    fn picker_selection_exposes_only_token_and_safe_name() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = DirectorySelectionRegistry::default();

        let selection = registry.issue(directory.path(), now).unwrap();
        let serialized = serde_json::to_value(&selection).unwrap();

        assert!(serialized.get("selectedDirectoryToken").is_some());
        assert!(serialized.get("suggestedName").is_some());
        assert_eq!(serialized.as_object().unwrap().len(), 2);
        assert!(
            !serialized
                .to_string()
                .contains(&directory.path().display().to_string())
        );
    }

    #[test]
    fn directory_tokens_are_single_use() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = DirectorySelectionRegistry::default();
        let selection = registry.issue(directory.path(), now).unwrap();

        let consumed = registry
            .consume(selection.selected_directory_token, now)
            .unwrap();
        assert_eq!(consumed, directory.path().canonicalize().unwrap());
        assert!(matches!(
            registry.consume(selection.selected_directory_token, now),
            Err(SelectionTokenError::UnknownOrExpired)
        ));
    }

    #[test]
    fn expired_directory_token_is_rejected_and_removed() {
        let directory = tempdir().unwrap();
        let now = Instant::now();
        let mut registry = DirectorySelectionRegistry::default();
        let selection = registry.issue(directory.path(), now).unwrap();

        assert!(matches!(
            registry.consume(
                selection.selected_directory_token,
                now + DIRECTORY_TOKEN_TTL + Duration::from_millis(1),
            ),
            Err(SelectionTokenError::UnknownOrExpired)
        ));
        assert_eq!(registry.pending_count(), 0);
    }

    #[test]
    fn non_git_directory_is_a_valid_project_without_a_git_key() {
        let directory = tempdir().unwrap();

        let identity = inspect_project_directory(directory.path()).unwrap();

        assert_eq!(identity.project_key, None);
        assert_eq!(identity.project_path, directory.path().to_str().unwrap());
        assert_eq!(identity.worktree_path, None);
        assert_eq!(identity.branch_name, None);
    }

    #[test]
    fn git_common_directory_is_the_stable_project_key() {
        let directory = tempdir().unwrap();
        let nested = directory.path().join("packages/app");
        fs::create_dir_all(&nested).unwrap();
        let initialized = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(directory.path())
            .status()
            .unwrap();
        assert!(initialized.success());

        let root_identity = inspect_project_directory(directory.path()).unwrap();
        let nested_identity = inspect_project_directory(&nested).unwrap();

        assert_eq!(root_identity.project_key, nested_identity.project_key);
        assert_eq!(
            root_identity.project_key,
            Some(
                directory
                    .path()
                    .join(".git")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into()
            )
        );
        assert_eq!(nested_identity.project_path, nested.to_str().unwrap());
        assert_eq!(
            nested_identity.worktree_path.as_deref(),
            directory.path().canonicalize().unwrap().to_str()
        );
    }

    #[test]
    fn linked_worktrees_share_a_project_key_and_keep_distinct_named_branches() {
        let directory = tempdir().unwrap();
        let linked = directory.path().join("linked");
        let repository = directory.path().join("repository");
        fs::create_dir(&repository).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet", "--initial-branch=main"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "--quiet", "--allow-empty", "-m", "initial"])
                .env("GIT_AUTHOR_NAME", "Lyn Test")
                .env("GIT_AUTHOR_EMAIL", "lyn@example.invalid")
                .env("GIT_COMMITTER_NAME", "Lyn Test")
                .env("GIT_COMMITTER_EMAIL", "lyn@example.invalid")
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    "feature",
                    linked.to_str().unwrap()
                ])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );

        let main = inspect_project_directory(&repository).unwrap();
        let feature = inspect_project_directory(&linked).unwrap();

        assert_eq!(main.project_key, feature.project_key);
        assert_eq!(main.branch_name.as_deref(), Some("main"));
        assert_eq!(feature.branch_name.as_deref(), Some("feature"));
        assert_ne!(main.worktree_path, feature.worktree_path);
    }

    #[test]
    fn detached_head_has_no_fabricated_branch_name() {
        let directory = tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "--quiet", "--allow-empty", "-m", "initial"])
                .env("GIT_AUTHOR_NAME", "Lyn Test")
                .env("GIT_AUTHOR_EMAIL", "lyn@example.invalid")
                .env("GIT_COMMITTER_NAME", "Lyn Test")
                .env("GIT_COMMITTER_EMAIL", "lyn@example.invalid")
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["checkout", "--quiet", "--detach", "HEAD"])
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );

        let identity = inspect_project_directory(directory.path()).unwrap();

        assert!(identity.project_key.is_some());
        assert_eq!(identity.branch_name, None);
    }
}
