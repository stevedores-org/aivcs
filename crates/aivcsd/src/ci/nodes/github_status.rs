/// GitHubStatusNode — updates commit status on GitHub
use crate::ci::state::context_keys;
use crate::github::GitHubClient;
use async_trait::async_trait;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::state::SharedState;
use std::sync::Arc;

pub struct GitHubStatusNode {
    pub client: Arc<dyn GitHubClient>,
}

#[async_trait]
impl NodeExecutor for GitHubStatusNode {
    fn id(&self) -> &str {
        "github_status"
    }

    async fn execute(&self, state: SharedState) -> Result<NodeOutput, NodeError> {
        // Get required fields from context
        let repo = {
            let lock = state
                .write()
                .map_err(|e| NodeError::other(format!("Failed to acquire state lock: {}", e)))?;
            lock.get_context::<String>(context_keys::CI_REPO)
                .unwrap_or_default()
        };

        let sha = {
            let lock = state
                .write()
                .map_err(|e| NodeError::other(format!("Failed to acquire state lock: {}", e)))?;
            lock.get_context::<String>(context_keys::CI_SHA)
                .unwrap_or_default()
        };

        let status = {
            let lock = state
                .write()
                .map_err(|e| NodeError::other(format!("Failed to acquire state lock: {}", e)))?;
            lock.get_context::<String>(context_keys::CI_STATUS)
                .unwrap_or_default()
        };

        // Determine GitHub status state from our status
        let github_state = if status == "passed" {
            "success"
        } else {
            "failure"
        };

        // Post status to GitHub
        self.client
            .update_commit_status(&repo, &sha, github_state, "CI checks completed")
            .await
            .map_err(|e| NodeError::other(format!("Failed to update GitHub status: {}", e)))?;

        Ok(NodeOutput::Continue(None))
    }

    fn description(&self) -> Option<&str> {
        Some("Update GitHub commit status")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::MockGitHubClient;
    use oxidizedgraph::state::AgentState;
    use std::sync::RwLock as StdRwLock;

    #[tokio::test]
    async fn test_posts_success_status() {
        let client = Arc::new(MockGitHubClient::new());
        let mut initial_state = AgentState::default();
        initial_state.set_context(context_keys::CI_REPO, "test-repo".to_string());
        initial_state.set_context(context_keys::CI_SHA, "abc123".to_string());
        initial_state.set_context(context_keys::CI_STATUS, "passed".to_string());

        let state = Arc::new(StdRwLock::new(initial_state));
        let node = GitHubStatusNode {
            client: client.clone(),
        };

        let result = node.execute(state).await;
        assert!(result.is_ok(), "execution should succeed");

        let statuses = client.posted_statuses.lock().unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].0, "test-repo");
        assert_eq!(statuses[0].1, "abc123");
        assert_eq!(statuses[0].2, "success");
    }

    #[tokio::test]
    async fn test_posts_failure_status() {
        let client = Arc::new(MockGitHubClient::new());
        let mut initial_state = AgentState::default();
        initial_state.set_context(context_keys::CI_REPO, "test-repo".to_string());
        initial_state.set_context(context_keys::CI_SHA, "def456".to_string());
        initial_state.set_context(context_keys::CI_STATUS, "failed".to_string());

        let state = Arc::new(StdRwLock::new(initial_state));
        let node = GitHubStatusNode {
            client: client.clone(),
        };

        let result = node.execute(state).await;
        assert!(result.is_ok(), "execution should succeed");

        let statuses = client.posted_statuses.lock().unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].2, "failure");
    }
}
