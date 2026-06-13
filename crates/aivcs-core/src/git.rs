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
///
/// Supported shapes (trailing `.git` is stripped before matching):
/// - `git@github.com:owner/name` (SCP-style SSH — the default git output)
/// - `https://github.com/owner/name`
/// - `ssh://git@github.com/owner/name` (RFC-style SSH that some tooling emits)
pub fn parse_github_remote(remote: &str) -> Option<String> {
    let without_suffix = remote.strip_suffix(".git").unwrap_or(remote);
    let candidate = without_suffix
        .strip_prefix("git@github.com:")
        .or_else(|| without_suffix.strip_prefix("https://github.com/"))
        .or_else(|| without_suffix.strip_prefix("ssh://git@github.com/"))?;

    is_owner_repo(candidate).then(|| candidate.to_string())
}

fn is_owner_repo(value: &str) -> bool {
    let mut parts = value.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(owner), Some(repo), None) if is_valid_segment(owner) && is_valid_segment(repo)
    )
}

/// GitHub's actual character class for owner / repo segments: ASCII alnum
/// plus `.`, `_`, `-`, AND segments may not start with `.` or `-` (GitHub
/// rejects these server-side). Anything outside that — whitespace,
/// newlines, shell metacharacters, leading dots / dashes — is rejected
/// so a malformed or hostile remote can't smuggle data through
/// `parse_github_remote`.
fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with(['.', '-'])
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Detect the current local git branch name.
///
/// Runs `git rev-parse --abbrev-ref HEAD` in the given directory.
pub fn detect_current_branch(repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .map_err(|e| AivcsError::GitError(format!("failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AivcsError::GitError(format!(
            "git rev-parse --abbrev-ref HEAD failed: {stderr}"
        )));
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return Err(AivcsError::GitError(
            "git rev-parse --abbrev-ref HEAD returned empty output".to_string(),
        ));
    }
    // `git rev-parse --abbrev-ref HEAD` returns the literal string "HEAD" when
    // the working tree is in detached-HEAD state (common in CI checkouts of
    // tag/PR refs). Returning that to callers caused `aivcs pr-note` to look
    // up a branch literally named "HEAD" in the DB and emit a confusing
    // "Branch 'HEAD' not found" error. Surface the state explicitly instead.
    if branch == "HEAD" {
        return Err(AivcsError::GitError(
            "git is in detached-HEAD state; pass --branch explicitly".to_string(),
        ));
    }

    Ok(branch)
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
        // All four shapes — HTTPS and SCP-style SSH, each with and without
        // the `.git` suffix. The bare forms appear when a user runs
        // `git clone` against a URL that was hand-typed without `.git`.
        assert_eq!(
            parse_github_remote("https://github.com/stevedores-org/aivcs.git"),
            Some("stevedores-org/aivcs".to_string())
        );
        assert_eq!(
            parse_github_remote("https://github.com/stevedores-org/aivcs"),
            Some("stevedores-org/aivcs".to_string())
        );
        assert_eq!(
            parse_github_remote("git@github.com:stevedores-org/aivcs.git"),
            Some("stevedores-org/aivcs".to_string())
        );
        assert_eq!(
            parse_github_remote("git@github.com:stevedores-org/aivcs"),
            Some("stevedores-org/aivcs".to_string())
        );
    }

    #[test]
    fn parse_github_remote_supports_rfc_ssh_url() {
        // RFC-style SSH URL — some tools (and `git remote -v` output on
        // certain Git versions / configs) emit this form. Slash separator
        // after the hostname, unlike the SCP-style colon form.
        assert_eq!(
            parse_github_remote("ssh://git@github.com/stevedores-org/aivcs.git"),
            Some("stevedores-org/aivcs".to_string())
        );
        assert_eq!(
            parse_github_remote("ssh://git@github.com/stevedores-org/aivcs"),
            Some("stevedores-org/aivcs".to_string())
        );
    }

    #[test]
    fn parse_github_remote_rejects_invalid_values() {
        assert_eq!(parse_github_remote("https://gitlab.com/org/repo"), None);
        assert_eq!(parse_github_remote("not-a-url"), None);
    }

    #[test]
    fn parse_github_remote_rejects_malformed_segments() {
        // Defense-in-depth: characters outside GitHub's owner/repo class
        // (alnum + . _ -) must be rejected so a malformed remote can't
        // splice arbitrary bytes into the returned owner/repo string.
        // Newline, space, shell metachars are the realistic vectors.
        assert_eq!(
            parse_github_remote("git@github.com:malicious\n/payload"),
            None
        );
        assert_eq!(
            parse_github_remote("git@github.com:owner with space/repo"),
            None
        );
        assert_eq!(
            parse_github_remote("git@github.com:owner;rm -rf/repo"),
            None
        );
        // Empty segments still rejected (regression guard).
        assert_eq!(parse_github_remote("git@github.com:/repo"), None);
        assert_eq!(parse_github_remote("git@github.com:owner/"), None);
        // Leading `.` and `-` are valid char-class members but rejected
        // by GitHub server-side; mirror that policy locally.
        assert_eq!(parse_github_remote("git@github.com:.evil/repo"), None);
        assert_eq!(parse_github_remote("git@github.com:owner/-evil"), None);
    }

    #[test]
    fn detect_current_branch_detached_head_returns_err() {
        let repo = make_git_repo();
        // Detach HEAD by checking out the commit SHA directly.
        let sha = capture_head_sha(repo.path()).unwrap();
        let status = Command::new("git")
            .args(["checkout", "--detach", &sha])
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert!(status.status.success(), "git checkout --detach failed");
        let err = detect_current_branch(repo.path()).expect_err("detached HEAD must error");
        let msg = err.to_string();
        // The state must be named so the operator sees what's wrong.
        assert!(
            msg.contains("detached-HEAD"),
            "error must name the state: {err}"
        );
        // The corrective action must survive future refactors of the
        // error message — this is the actual user-facing value-add.
        assert!(
            msg.contains("pass --branch explicitly"),
            "error must include the corrective action: {err}"
        );
    }

    #[test]
    fn detect_current_branch_returns_branch_name() {
        let repo = make_git_repo();
        let branch = detect_current_branch(repo.path()).unwrap();
        assert!(!branch.is_empty());
        assert!(branch == "master" || branch == "main");
    }
}
