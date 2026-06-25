/// AggregateNode — collects parallel check results

use crate::ci::state::{CheckResult, CiStatus, context_keys};
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

    async fn execute(&self, state: SharedState) -> Result<NodeOutput, NodeError> {
        let mut state_lock = state.write().map_err(|e| {
            NodeError::other(format!("Failed to acquire state lock: {}", e))
        })?;

        // Get checks from context
        let checks: Vec<CheckResult> = state_lock
            .get_context(context_keys::CI_CHECKS)
            .unwrap_or_default();

        // Determine overall status: passed if all checks passed, failed if any failed
        let overall_status = if checks.is_empty() {
            CiStatus::Passed  // No checks = no failures
        } else if checks.iter().all(|c| c.status == "passed") {
            CiStatus::Passed
        } else {
            CiStatus::Failed
        };

        // Write overall status to context
        state_lock.set_context(context_keys::CI_STATUS, overall_status.to_string());

        Ok(NodeOutput::Continue(None))
    }

    fn description(&self) -> Option<&str> {
        Some("Aggregate CI check results")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidizedgraph::state::AgentState;
    use std::sync::RwLock as StdRwLock;

    #[tokio::test]
    async fn test_all_passed_yields_passed() {
        let mut initial_state = AgentState::default();
        initial_state.set_context(
            context_keys::CI_CHECKS,
            vec![
                CheckResult {
                    name: "check1".to_string(),
                    status: "passed".to_string(),
                    duration_ms: 100,
                    output: None,
                },
                CheckResult {
                    name: "check2".to_string(),
                    status: "passed".to_string(),
                    duration_ms: 200,
                    output: None,
                },
            ],
        );

        let state = std::sync::Arc::new(StdRwLock::new(initial_state));
        let node = AggregateNode;

        let result = node.execute(state.clone()).await;
        assert!(result.is_ok(), "execution should succeed");

        let final_state = state.read().unwrap();
        let status = final_state
            .get_context::<String>(context_keys::CI_STATUS)
            .unwrap_or_default();
        assert_eq!(status, "passed", "status should be 'passed' when all checks pass");
    }

    #[tokio::test]
    async fn test_one_failed_yields_failed() {
        let mut initial_state = AgentState::default();
        initial_state.set_context(
            context_keys::CI_CHECKS,
            vec![
                CheckResult {
                    name: "check1".to_string(),
                    status: "passed".to_string(),
                    duration_ms: 100,
                    output: None,
                },
                CheckResult {
                    name: "check2".to_string(),
                    status: "failed".to_string(),
                    duration_ms: 200,
                    output: Some("error".to_string()),
                },
            ],
        );

        let state = std::sync::Arc::new(StdRwLock::new(initial_state));
        let node = AggregateNode;

        let result = node.execute(state.clone()).await;
        assert!(result.is_ok(), "execution should succeed");

        let final_state = state.read().unwrap();
        let status = final_state
            .get_context::<String>(context_keys::CI_STATUS)
            .unwrap_or_default();
        assert_eq!(status, "failed", "status should be 'failed' when any check fails");
    }
}
