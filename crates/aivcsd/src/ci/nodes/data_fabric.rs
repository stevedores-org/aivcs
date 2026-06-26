/// DataFabricNode — persists results to data-fabric
use crate::ci::state::context_keys;
use crate::df::DataFabricGateway;
use async_trait::async_trait;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::state::SharedState;
use std::sync::Arc;

pub struct DataFabricNode {
    pub gateway: Arc<dyn DataFabricGateway>,
}

#[async_trait]
impl NodeExecutor for DataFabricNode {
    fn id(&self) -> &str {
        "data_fabric"
    }

    async fn execute(&self, state: SharedState) -> Result<NodeOutput, NodeError> {
        // Get task_id from context in a scoped block to release lock before async call
        let task_id = {
            let state_lock = state
                .write()
                .map_err(|e| NodeError::other(format!("Failed to acquire state lock: {}", e)))?;

            state_lock
                .get_context::<String>(context_keys::CI_TASK_ID)
                .unwrap_or_default()
        };

        // Complete the task (lock is released now)
        self.gateway
            .complete_task(&task_id)
            .await
            .map_err(|e| NodeError::other(format!("Failed to complete task: {}", e)))?;

        Ok(NodeOutput::Continue(None))
    }

    fn description(&self) -> Option<&str> {
        Some("Persist results to data-fabric and complete task")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::df::MockDataFabricGateway;
    use oxidizedgraph::state::AgentState;
    use std::sync::RwLock as StdRwLock;

    #[tokio::test]
    async fn test_calls_complete_task() {
        let gateway = Arc::new(MockDataFabricGateway::new());
        let mut initial_state = AgentState::default();
        initial_state.set_context(context_keys::CI_TASK_ID, "task-123".to_string());
        initial_state.set_context(context_keys::CI_STATUS, "passed".to_string());

        let state = Arc::new(StdRwLock::new(initial_state));
        let node = DataFabricNode {
            gateway: gateway.clone(),
        };

        let result = node.execute(state).await;
        assert!(result.is_ok(), "execution should succeed");

        // Verify task was completed
        let completed = gateway.completed_tasks.lock().unwrap();
        assert!(
            completed.contains(&"task-123".to_string()),
            "task should be marked complete"
        );
    }
}
