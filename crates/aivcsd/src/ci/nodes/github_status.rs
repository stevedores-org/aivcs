/// GitHubStatusNode — updates commit status on GitHub

use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::state::SharedState;
use async_trait::async_trait;

pub struct GitHubStatusNode;

#[async_trait]
impl NodeExecutor for GitHubStatusNode {
    fn id(&self) -> &str {
        "github_status"
    }

    async fn execute(&self, _state: SharedState) -> Result<NodeOutput, NodeError> {
        todo!("GitHubStatusNode: post commit status to GitHub (success/failure)")
    }

    fn description(&self) -> Option<&str> {
        Some("Update GitHub commit status")
    }
}
