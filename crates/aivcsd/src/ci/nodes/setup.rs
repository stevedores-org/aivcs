/// SetupNode — writes request fields to context
use crate::ci::state::{context_keys, CiTaskParams};
use async_trait::async_trait;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::graph::{NodeExecutor, NodeOutput};
use oxidizedgraph::state::SharedState;
use serde_json::json;

pub struct SetupNode;

#[async_trait]
impl NodeExecutor for SetupNode {
    fn id(&self) -> &str {
        "setup"
    }

    async fn execute(&self, state: SharedState) -> Result<NodeOutput, NodeError> {
        let mut state_lock = state
            .write()
            .map_err(|e| NodeError::other(format!("Failed to acquire state lock: {}", e)))?;

        // Read task params from context (set by WorkerLoop before spawning graph)
        let params: CiTaskParams = state_lock
            .get_context::<CiTaskParams>("task_params")
            .ok_or_else(|| NodeError::other("Missing task_params in context".to_string()))?;

        // Write individual fields to CI context keys
        state_lock.set_context(context_keys::CI_REPO, json!(params.repo));
        state_lock.set_context(context_keys::CI_PR_NUMBER, json!(params.pr_number));
        state_lock.set_context(context_keys::CI_SHA, json!(params.sha));

        Ok(NodeOutput::Continue(None))
    }

    fn description(&self) -> Option<&str> {
        Some("Setup CI run context from request")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::state::{context_keys, CiTaskParams};
    use oxidizedgraph::state::AgentState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_writes_context_keys() {
        use std::sync::RwLock as StdRwLock;

        // Create initial state with task_params set
        let mut initial_state = AgentState::default();
        initial_state.set_context(
            "task_params",
            CiTaskParams {
                repo: "stevedores-org/aivcs".to_string(),
                pr_number: 123,
                sha: "abc123def456".to_string(),
            },
        );

        let state = Arc::new(StdRwLock::new(initial_state));
        let node = SetupNode;

        // Execute SetupNode
        let result = node.execute(state.clone()).await;
        assert!(
            result.is_ok(),
            "SetupNode execution should succeed: {:?}",
            result
        );

        // Verify context keys were written
        let final_state = state.read().unwrap();
        assert_eq!(
            final_state.get_context::<String>(context_keys::CI_REPO),
            Some("stevedores-org/aivcs".to_string()),
            "ci.repo should be set"
        );
        assert_eq!(
            final_state.get_context::<u64>(context_keys::CI_PR_NUMBER),
            Some(123),
            "ci.pr_number should be set"
        );
        assert_eq!(
            final_state.get_context::<String>(context_keys::CI_SHA),
            Some("abc123def456".to_string()),
            "ci.sha should be set"
        );
    }
}
