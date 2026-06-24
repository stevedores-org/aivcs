/// SetupNode — writes request fields to context

use crate::ci::state::{CiTaskParams, context_keys};
use oxidizedgraph::graph::{NodeExecutor, NodeOutput};
use oxidizedgraph::error::NodeError;
use oxidizedgraph::state::SharedState;
use serde_json::json;
use async_trait::async_trait;

pub struct SetupNode;

#[async_trait]
impl NodeExecutor for SetupNode {
    fn id(&self) -> &str {
        "setup"
    }

    async fn execute(&self, state: SharedState) -> Result<NodeOutput, NodeError> {
        let mut state_lock = state.write().map_err(|e| {
            NodeError::other(format!("Failed to acquire state lock: {}", e))
        })?;

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
