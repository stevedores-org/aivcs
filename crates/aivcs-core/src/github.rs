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
    /// Create a new GitHub client using a personal access token.
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

    /// Commit a single file to a specific branch.
    pub async fn commit_file(
        &self,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Result<()> {
        info!("Committing file '{}' to branch '{}'", path, branch);

        self.octocrab
            .repos(&self.owner, &self.repo)
            .create_file(path, message, content)
            .branch(branch)
            .send()
            .await
            .context(format!("failed to commit file '{}'", path))?;

        Ok(())
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

/// Validate the raw `RELIC_LIBRARIAN_USERNAME` env-var lookup result.
///
/// Split out so the validation contract can be unit-tested without an HTTP
/// client. A missing or whitespace-only value is rejected eagerly: the
/// alternative is silently requesting review from a placeholder user that may
/// not exist and failing partway through a multi-step pipeline.
fn resolve_librarian_username(
    raw: std::result::Result<String, std::env::VarError>,
) -> Result<String> {
    let value =
        raw.context("RELIC_LIBRARIAN_USERNAME must be set to request a Librarian Agent review")?;
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
    fn resolve_librarian_username_not_unicode_is_rejected() {
        let err = resolve_librarian_username(Err(VarError::NotUnicode(std::ffi::OsString::from(
            "ignored",
        ))))
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("RELIC_LIBRARIAN_USERNAME must be set"),
            "expected missing-env-style error for non-unicode value, got: {err:#}"
        );
    }
}
