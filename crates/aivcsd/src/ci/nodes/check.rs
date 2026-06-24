/// CheckNode — runs a single CI check

use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::state::SharedState;
use async_trait::async_trait;

pub struct CheckNode {
    pub name: String,
    pub cmd: String,
    pub args: Vec<String>,
}

#[async_trait]
impl NodeExecutor for CheckNode {
    fn id(&self) -> &str {
        &self.name
    }

    async fn execute(&self, _state: SharedState) -> Result<NodeOutput, NodeError> {
        todo!("CheckNode: run command, capture output, store CheckResult")
    }

    fn description(&self) -> Option<&str> {
        Some("Run CI check")
    }
}
