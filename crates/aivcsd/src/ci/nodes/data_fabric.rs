/// DataFabricNode — persists results and completes task

use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::state::SharedState;
use async_trait::async_trait;

pub struct DataFabricNode;

#[async_trait]
impl NodeExecutor for DataFabricNode {
    fn id(&self) -> &str {
        "data_fabric"
    }

    async fn execute(&self, _state: SharedState) -> Result<NodeOutput, NodeError> {
        todo!("DataFabricNode: ingest_aivcs_events, complete_task, write run_id to context")
    }

    fn description(&self) -> Option<&str> {
        Some("Persist results to data-fabric and complete task")
    }
}
