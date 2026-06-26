/// CheckNode — runs a single CI check
use crate::ci::runner::CheckRunner;
use crate::ci::state::{context_keys, CheckResult};
use async_trait::async_trait;
use oxidizedgraph::error::NodeError;
use oxidizedgraph::graph::NodeExecutor;
use oxidizedgraph::graph::NodeOutput;
use oxidizedgraph::state::SharedState;
use std::sync::Arc;

pub struct CheckNode {
    pub name: String,
    pub cmd: String,
    pub args: Vec<String>,
    pub runner: Arc<dyn CheckRunner>,
}

#[async_trait]
impl NodeExecutor for CheckNode {
    fn id(&self) -> &str {
        &self.name
    }

    async fn execute(&self, state: SharedState) -> Result<NodeOutput, NodeError> {
        // Run the check command
        let (exit_code, output, duration_ms) = self
            .runner
            .run(
                &self.cmd,
                &self.args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                &[],
            )
            .await
            .map_err(|e| NodeError::other(format!("Check execution failed: {}", e)))?;

        // Determine status based on exit code
        let status = if exit_code == 0 { "passed" } else { "failed" };

        // Create CheckResult
        let result = CheckResult {
            name: self.name.clone(),
            status: status.to_string(),
            duration_ms,
            output: Some(output),
        };

        // Write result to context
        let mut state_lock = state
            .write()
            .map_err(|e| NodeError::other(format!("Failed to acquire state lock: {}", e)))?;

        // Get or create checks array
        let mut checks: Vec<CheckResult> = state_lock
            .get_context(context_keys::CI_CHECKS)
            .unwrap_or_default();
        checks.push(result);

        state_lock.set_context(context_keys::CI_CHECKS, checks);

        Ok(NodeOutput::Continue(None))
    }

    fn description(&self) -> Option<&str> {
        Some("Run CI check")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidizedgraph::state::AgentState;
    use std::sync::RwLock as StdRwLock;

    struct MockCheckRunner;

    #[async_trait]
    impl CheckRunner for MockCheckRunner {
        async fn run(
            &self,
            cmd: &str,
            _args: &[&str],
            _env: &[(&str, &str)],
        ) -> anyhow::Result<(i32, String, u64)> {
            // Simulate different commands
            match cmd {
                "pass" => Ok((0, "output".to_string(), 100)),
                "fail" => Ok((1, "error output".to_string(), 200)),
                _ => Err(anyhow::anyhow!("unknown command")),
            }
        }
    }

    #[tokio::test]
    async fn test_passed_on_zero_exit_code() {
        let state = Arc::new(StdRwLock::new(AgentState::default()));
        let node = CheckNode {
            name: "test-check".to_string(),
            cmd: "pass".to_string(),
            args: vec![],
            runner: Arc::new(MockCheckRunner),
        };

        let result = node.execute(state.clone()).await;
        assert!(result.is_ok(), "execution should succeed");

        let final_state = state.read().unwrap();
        let checks: Vec<CheckResult> = final_state
            .get_context(context_keys::CI_CHECKS)
            .unwrap_or_default();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, "passed");
        assert_eq!(checks[0].duration_ms, 100);
    }

    #[tokio::test]
    async fn test_failed_on_nonzero_exit_code() {
        let state = Arc::new(StdRwLock::new(AgentState::default()));
        let node = CheckNode {
            name: "test-check".to_string(),
            cmd: "fail".to_string(),
            args: vec![],
            runner: Arc::new(MockCheckRunner),
        };

        let result = node.execute(state.clone()).await;
        assert!(result.is_ok(), "execution should succeed");

        let final_state = state.read().unwrap();
        let checks: Vec<CheckResult> = final_state
            .get_context(context_keys::CI_CHECKS)
            .unwrap_or_default();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, "failed");
        assert_eq!(checks[0].duration_ms, 200);
    }

    #[tokio::test]
    async fn test_captures_duration_ms() {
        let state = Arc::new(StdRwLock::new(AgentState::default()));
        let node = CheckNode {
            name: "timer-check".to_string(),
            cmd: "pass".to_string(),
            args: vec![],
            runner: Arc::new(MockCheckRunner),
        };

        node.execute(state.clone()).await.ok();

        let final_state = state.read().unwrap();
        let checks: Vec<CheckResult> = final_state
            .get_context(context_keys::CI_CHECKS)
            .unwrap_or_default();
        assert!(checks[0].duration_ms > 0, "duration should be captured");
    }
}
