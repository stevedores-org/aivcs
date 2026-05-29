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
        let base_ref = self.octocrab
            .repos(&self.owner, &self.repo)
            .get_ref(&octocrab::params::repos::Reference::Branch(base.to_string()))
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

        let pr = self.octocrab
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

    /// Request a review from the Librarian Agent.
    pub async fn request_librarian_review(&self, pr_number: u64) -> Result<()> {
        let librarian = std::env::var("RELIC_LIBRARIAN_USERNAME")
            .unwrap_or_else(|_| "librarian-agent".to_string());
        
        info!("Requesting review from Librarian Agent ('{}') on PR #{}", librarian, pr_number);

        // octocrab 0.41 request_reviews takes (pr_number, reviewers, team_reviewers)
        self.octocrab
            .pulls(&self.owner, &self.repo)
            .request_reviews(pr_number, vec![librarian], vec![])
            .await
            .context(format!("failed to request review for PR #{}", pr_number))?;

        Ok(())
    }
}
