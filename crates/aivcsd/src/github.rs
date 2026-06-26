use anyhow::Result;
/// GitHub client abstraction for commit status updates
use async_trait::async_trait;

/// GitHubClient trait for posting commit statuses
#[async_trait]
pub trait GitHubClient: Send + Sync {
    /// Update commit status on GitHub
    async fn update_commit_status(
        &self,
        repo: &str,
        sha: &str,
        state: &str,
        description: &str,
    ) -> Result<()>;
}

/// Mock implementation for testing
pub struct MockGitHubClient {
    pub posted_statuses: std::sync::Mutex<Vec<(String, String, String)>>,
}

impl MockGitHubClient {
    pub fn new() -> Self {
        Self {
            posted_statuses: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl GitHubClient for MockGitHubClient {
    async fn update_commit_status(
        &self,
        repo: &str,
        sha: &str,
        state: &str,
        _description: &str,
    ) -> Result<()> {
        self.posted_statuses.lock().unwrap().push((
            repo.to_string(),
            sha.to_string(),
            state.to_string(),
        ));
        Ok(())
    }
}

/// Real GitHub client using octocrab
pub struct OctocrabGitHubClient {
    token: String,
}

impl OctocrabGitHubClient {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl GitHubClient for OctocrabGitHubClient {
    async fn update_commit_status(
        &self,
        _repo: &str,
        _sha: &str,
        _state: &str,
        _description: &str,
    ) -> Result<()> {
        // TODO: Implement octocrab call
        // For now, just succeed silently
        Ok(())
    }
}
