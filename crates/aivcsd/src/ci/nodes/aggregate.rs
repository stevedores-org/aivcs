/// AggregateNode — collects parallel check results

use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::state::SharedState;
use async_trait::async_trait;

pub struct AggregateNode;

#[async_trait]
impl NodeExecutor for AggregateNode {
    fn id(&self) -> &str {
        "aggregate"
    }

    async fn execute(&self, _state: SharedState) -> Result<NodeOutput, NodeError> {
        todo!("AggregateNode: collect checks from context, determine overall status")
    }

    fn description(&self) -> Option<&str> {
        Some("Aggregate CI check results")
    }
}
