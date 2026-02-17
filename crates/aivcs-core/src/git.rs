//! Git integration utilities for capturing repository state.

use std::path::Path;
use std::process::Command;

use crate::domain::error::{AivcsError, Result};

/// Capture the HEAD commit SHA from a git repository.
///
/// Runs `git rev-parse HEAD` in the given directory. Returns an error if the
/// directory is not inside a git repository or if git is not available.
pub fn capture_head_sha(repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .map_err(|e| AivcsError::GitError(format!("failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AivcsError::GitError(format!(
            "git rev-parse HEAD failed: {stderr}"
        )));
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        return Err(AivcsError::GitError(
            "git rev-parse HEAD returned empty output".to_string(),
        ));
    }

    Ok(sha)
}

/// Check whether a directory is inside a git work tree.
pub fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as StdCommand;

    fn run_git(repo_dir: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(repo_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn make_git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.name", "test-user"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["commit", "--allow-empty", "-m", "initial"]);
        dir
    }

    #[test]
    fn capture_head_sha_returns_40_hex_chars() {
        let repo = make_git_repo();
        let sha = capture_head_sha(repo.path()).unwrap();
        assert_eq!(sha.len(), 40, "SHA should be 40 hex chars, got: {sha}");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn capture_head_sha_fails_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = capture_head_sha(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn is_git_repo_true_for_repo() {
        let repo = make_git_repo();
        assert!(is_git_repo(repo.path()));
    }

    #[test]
    fn is_git_repo_false_for_non_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_git_repo(dir.path()));
    }
}
