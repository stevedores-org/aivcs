//! GitHub integration for autonomous version control operations.
//!
//! Provides high-level APIs for branch creation, file commits, and Pull Request
//! management, including automated reviewer assignment (Librarian Agent).

use anyhow::{Context, Result};
use octocrab::Octocrab;
use tracing::info;

/// Client for performing GitHub operations.
pub struct GitHubClient {
    octocrab: Octocrab,
    owner: String,
    repo: String,
}

impl GitHubClient {
    /// Create a new GitHub client using a bearer token (PAT or GitHub App installation token).
    pub fn new(token: String, owner: String, repo: String) -> Result<Self> {
        let octocrab = Octocrab::builder()
            .personal_token(token)
            .build()
            .context("failed to initialize GitHub client")?;

        Ok(Self {
            octocrab,
            owner,
            repo,
        })
    }

    /// Create a new branch from a base reference (usually "main").
    pub async fn create_branch(&self, branch_name: &str, base: &str) -> Result<String> {
        info!("Creating branch '{}' from '{}'", branch_name, base);

        // Get base branch SHA
        let base_ref = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .get_ref(&octocrab::params::repos::Reference::Branch(
                base.to_string(),
            ))
            .await
            .context(format!("failed to get base ref '{}'", base))?;

        let sha = match base_ref.object {
            octocrab::models::repos::Object::Commit { sha, .. } => sha,
            octocrab::models::repos::Object::Tag { sha, .. } => sha,
            _ => return Err(anyhow::anyhow!("unsupported base ref type")),
        };

        // Create new ref
        self.octocrab
            .repos(&self.owner, &self.repo)
            .create_ref(
                &octocrab::params::repos::Reference::Branch(branch_name.to_string()),
                &sha,
            )
            .await
            .context(format!("failed to create branch '{}'", branch_name))?;

        Ok(sha)
    }

    /// Commit a single file to a specific branch. Returns the resulting commit SHA.
    ///
    /// Currently UTF-8 text only — octocrab's `create_file` takes `String`
    /// content. Binary commits need a different API path; see the inline
    /// reject in `cmd_pr_commit`.
    pub async fn commit_file(
        &self,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Result<String> {
        info!("Committing file '{}' to branch '{}'", path, branch);

        let update = self
            .octocrab
            .repos(&self.owner, &self.repo)
            .create_file(path, message, content)
            .branch(branch)
            .send()
            .await
            .context(format!("failed to commit file '{}'", path))?;

        update.commit.sha.ok_or_else(|| {
            anyhow::anyhow!("GitHub Contents API returned no commit SHA for '{}'", path)
        })
    }

    /// Open a Pull Request and optionally request review from the Librarian Agent.
    pub async fn open_pr(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
        request_librarian: bool,
    ) -> Result<u64> {
        info!("Opening PR: '{}' ({} -> {})", title, head, base);

        let pr = self
            .octocrab
            .pulls(&self.owner, &self.repo)
            .create(title, head, base)
            .body(body)
            .send()
            .await
            .context("failed to create pull request")?;

        let pr_number = pr.number;
        info!("PR #{} opened successfully", pr_number);

        if request_librarian {
            self.request_librarian_review(pr_number).await?;
        }

        Ok(pr_number)
    }

    /// Add a comment to an existing issue or Pull Request.
    pub async fn add_comment(&self, issue_number: u64, body: &str) -> Result<u64> {
        info!("Adding comment to #{}", issue_number);

        let comment = self
            .octocrab
            .issues(&self.owner, &self.repo)
            .create_comment(issue_number, body)
            .await
            .context(format!("failed to add comment to #{}", issue_number))?;

        Ok(comment.id.0)
    }

    /// Request a review from the Librarian Agent.
    ///
    /// The Librarian's GitHub username is read from `RELIC_LIBRARIAN_USERNAME`.
    /// This is required when called: a missing or empty env var aborts before any
    /// API call, rather than silently requesting review from a placeholder user
    /// that may not exist and failing partway through a multi-step pipeline.
    pub async fn request_librarian_review(&self, pr_number: u64) -> Result<()> {
        let librarian = resolve_librarian_username(std::env::var("RELIC_LIBRARIAN_USERNAME"))?;

        info!(
            "Requesting review from Librarian Agent ('{}') on PR #{}",
            librarian, pr_number
        );

        // octocrab 0.41 request_reviews takes (pr_number, reviewers, team_reviewers)
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .request_reviews(pr_number, vec![librarian], vec![])
            .await
            .context(format!("failed to request review for PR #{}", pr_number))?;

        Ok(())
    }
}

/// Resolve a GitHub bearer token for autonomous agent Jobs.
///
/// Prefers `GITHUB_TOKEN` (typical ESO projected env var). Falls back to
/// `GITHUB_TOKEN_FILE` (Kubernetes secret volume mount path) so tokens never
/// need to be written to shell history.
pub fn resolve_github_token() -> Result<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Ok(path) = std::env::var("GITHUB_TOKEN_FILE") {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read GITHUB_TOKEN_FILE at '{path}'"))?;
        let trimmed = content.trim();
        anyhow::ensure!(
            !trimmed.is_empty(),
            "GITHUB_TOKEN_FILE at '{path}' is empty"
        );
        return Ok(trimmed.to_string());
    }

    anyhow::bail!("GITHUB_TOKEN or GITHUB_TOKEN_FILE must be set for GitHub API access")
}

/// Validate the raw `RELIC_LIBRARIAN_USERNAME` env-var lookup result.
///
/// Split out so the validation contract can be unit-tested without an HTTP
/// client. A missing or whitespace-only value is rejected eagerly: the
/// alternative is silently requesting review from a placeholder user that may
/// not exist and failing partway through a multi-step pipeline.
fn resolve_librarian_username(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<String> {
    let value = match raw {
        Ok(v) => v,
        Err(std::env::VarError::NotPresent) => {
            anyhow::bail!(
                "RELIC_LIBRARIAN_USERNAME must be set to request a Librarian Agent review"
            )
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("RELIC_LIBRARIAN_USERNAME contains non-UTF-8 bytes")
        }
    };
    anyhow::ensure!(
        !value.trim().is_empty(),
        "RELIC_LIBRARIAN_USERNAME is set but empty"
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::VarError;

    #[test]
    fn resolve_librarian_username_missing_env_is_rejected() {
        let err = resolve_librarian_username(Err(VarError::NotPresent)).unwrap_err();
        assert!(
            format!("{err:#}").contains("RELIC_LIBRARIAN_USERNAME must be set"),
            "expected missing-env error, got: {err:#}"
        );
    }

    #[test]
    fn resolve_librarian_username_empty_string_is_rejected() {
        let err = resolve_librarian_username(Ok(String::new())).unwrap_err();
        assert!(
            format!("{err:#}").contains("set but empty"),
            "expected empty-value error, got: {err:#}"
        );
    }

    #[test]
    fn resolve_librarian_username_whitespace_only_is_rejected() {
        let err = resolve_librarian_username(Ok("   \t\n".to_string())).unwrap_err();
        assert!(
            format!("{err:#}").contains("set but empty"),
            "expected whitespace-only to be treated as empty, got: {err:#}"
        );
    }

    #[test]
    fn resolve_librarian_username_valid_value_is_returned_verbatim() {
        let username = resolve_librarian_username(Ok("librarian-bot".to_string())).unwrap();
        assert_eq!(username, "librarian-bot");
    }

    #[test]
    fn resolve_librarian_username_not_unicode_is_rejected_with_distinct_message() {
        // NotUnicode means the env var IS set but contains invalid UTF-8 —
        // distinguish it from "missing" so an operator debugging an ESO
        // projection sees the real failure mode.
        let err = resolve_librarian_username(Err(VarError::NotUnicode(std::ffi::OsString::from(
            "ignored",
        ))))
        .unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("non-UTF-8"),
            "expected non-UTF-8-specific error, got: {rendered}"
        );
        assert!(
            !rendered.contains("must be set"),
            "non-UTF-8 error must not claim the variable is unset, got: {rendered}"
        );
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            // SAFETY: tests run serially within this module; each guard restores on drop.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn resolve_github_token_prefers_env_var() {
        let _token = EnvGuard::set("GITHUB_TOKEN", "ghp_from_env");
        let _file = EnvGuard::set("GITHUB_TOKEN_FILE", "/should/not/read");

        let token = resolve_github_token().unwrap();
        assert_eq!(token, "ghp_from_env");
    }

    #[test]
    fn resolve_github_token_reads_file_when_env_empty() {
        let _token = EnvGuard::set("GITHUB_TOKEN", "   ");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "ghp_from_file\n").unwrap();
        let _file = EnvGuard::set("GITHUB_TOKEN_FILE", path.to_str().unwrap());

        let token = resolve_github_token().unwrap();
        assert_eq!(token, "ghp_from_file");
    }

    #[test]
    fn resolve_github_token_missing_both_is_rejected() {
        let _token = EnvUnsetGuard::unset("GITHUB_TOKEN");
        let _file = EnvUnsetGuard::unset("GITHUB_TOKEN_FILE");

        let err = resolve_github_token().unwrap_err();
        assert!(
            format!("{err:#}").contains("GITHUB_TOKEN or GITHUB_TOKEN_FILE"),
            "expected missing-token error, got: {err:#}"
        );
    }

    struct EnvUnsetGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvUnsetGuard {
        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvUnsetGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
}
