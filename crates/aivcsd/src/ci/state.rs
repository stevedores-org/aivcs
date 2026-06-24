/// CI orchestration state and context types

use serde::{Deserialize, Serialize};

/// Request to trigger a CI run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiRunRequest {
    pub repo: String,
    pub pr_number: u64,
    pub sha: String,
}

/// Parameters from data-fabric task queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiTaskParams {
    pub repo: String,
    pub pr_number: u64,
    pub sha: String,
}

/// Result of a single check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub status: String, // "passed" | "failed"
    pub duration_ms: u64,
    pub output: Option<String>,
}

/// Aggregated CI status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CiStatus {
    Passed,
    Failed,
}

impl std::fmt::Display for CiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passed => write!(f, "passed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Context key constants for AgentState
pub mod context_keys {
    pub const CI_REPO: &str = "ci.repo";
    pub const CI_PR_NUMBER: &str = "ci.pr_number";
    pub const CI_SHA: &str = "ci.sha";
    pub const CI_CHECKS: &str = "ci.checks";
    pub const CI_STATUS: &str = "ci.status";
    pub const CI_RUN_ID: &str = "ci.run_id";
    pub const CI_TASK_ID: &str = "ci.task_id";
}

/// Serde helpers
pub mod serde_helpers {
    use serde_json::{json, Value};
    use super::CheckResult;

    /// Convert Vec<CheckResult> to JSON Value for context storage
    pub fn checks_to_value(checks: &[CheckResult]) -> Value {
        json!(checks)
    }

    /// Convert JSON Value back to Vec<CheckResult>
    pub fn value_to_checks(value: &Value) -> serde_json::Result<Vec<CheckResult>> {
        serde_json::from_value(value.clone())
    }

    /// Convert CiStatus to JSON string value
    pub fn status_to_value(status: &super::CiStatus) -> Value {
        Value::String(status.to_string())
    }

    /// Parse JSON string value to CiStatus
    pub fn value_to_status(value: &Value) -> Result<super::CiStatus, String> {
        let s = value.as_str().ok_or("expected string".to_string())?;
        match s {
            "passed" => Ok(super::CiStatus::Passed),
            "failed" => Ok(super::CiStatus::Failed),
            other => Err(format!("unknown status: {}", other)),
        }
    }
}
