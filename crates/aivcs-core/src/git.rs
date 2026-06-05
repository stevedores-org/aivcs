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

/// Resolve `owner/name` for GitHub event payloads.
///
/// Prefers `GITHUB_REPOSITORY`, then parses `git remote get-url origin`.
pub fn detect_github_repository() -> Option<String> {
    std::env::var("GITHUB_REPOSITORY")
        .ok()
        .filter(|value| is_owner_repo(value))
        .or_else(detect_github_repository_from_origin)
}

fn detect_github_repository_from_origin() -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let remote = String::from_utf8(output.stdout).ok()?;
    parse_github_remote(remote.trim())
}

/// Parse `owner/name` from common GitHub remote URL formats.
pub fn parse_github_remote(remote: &str) -> Option<String> {
    let without_suffix = remote.strip_suffix(".git").unwrap_or(remote);
    let candidate = without_suffix
        .strip_prefix("git@github.com:")
        .or_else(|| without_suffix.strip_prefix("https://github.com/"))?;

    is_owner_repo(candidate).then(|| candidate.to_string())
}

fn is_owner_repo(value: &str) -> bool {
    let mut parts = value.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(owner), Some(repo), None) if !owner.is_empty() && !repo.is_empty()
    )
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

    #[test]
    fn parse_github_remote_supports_https_and_ssh() {
        assert_eq!(
            parse_github_remote("https://github.com/stevedores-org/aivcs.git"),
            Some("stevedores-org/aivcs".to_string())
        );
        assert_eq!(
            parse_github_remote("git@github.com:stevedores-org/aivcs.git"),
            Some("stevedores-org/aivcs".to_string())
        );
    }

    #[test]
    fn parse_github_remote_rejects_invalid_values() {
        assert_eq!(parse_github_remote("https://gitlab.com/org/repo"), None);
        assert_eq!(parse_github_remote("not-a-url"), None);
    }
}
