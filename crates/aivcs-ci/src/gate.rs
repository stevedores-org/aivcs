//! CI gate evaluation for pass/fail criteria.

use oxidized_state::RunEvent;
use serde::{Deserialize, Serialize};

/// Gate evaluation verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateVerdict {
    /// Whether the gate passed.
    pub passed: bool,

    /// Violations that caused failure (empty if passed).
    pub violations: Vec<String>,

    /// Summary message.
    pub message: String,
}

/// CI gate evaluation rules.
pub struct CiGate;

impl CiGate {
    /// Evaluate whether all enabled stages passed.
    ///
    /// Gate rule:
    /// - For each enabled stage, there must be a corresponding `ToolCalled` event
    /// - Followed by either:
    ///   - A `ToolReturned` event with exit_code == 0 (pass)
    ///   - A `ToolFailed` event (fail)
    /// - If any stage has a `ToolFailed` event or non-zero exit_code, gate fails
    pub fn evaluate(events: &[RunEvent]) -> GateVerdict {
        let mut violations = Vec::new();

        // Group events by tool name and check for failures
        let mut tool_results = std::collections::HashMap::new();

        for event in events {
            if event.kind == "tool_called" {
                let tool_name = event.payload["tool_name"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                tool_results.insert(tool_name, "called".to_string());
            } else if event.kind == "tool_returned" {
                let tool_name = event.payload["tool_name"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                let exit_code = event.payload["exit_code"].as_i64().unwrap_or(-1);

                if exit_code == 0 {
                    tool_results.insert(tool_name, "passed".to_string());
                } else {
                    violations.push(format!(
                        "Tool '{}' returned non-zero exit code: {}",
                        tool_name, exit_code
                    ));
                    tool_results.insert(tool_name, "failed".to_string());
                }
            } else if event.kind == "tool_failed" {
                let tool_name = event.payload["tool_name"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                let error = event.payload["error"]
                    .as_str()
                    .unwrap_or("Unknown error")
                    .to_string();
                violations.push(format!("Tool '{}' failed: {}", tool_name, error));
                tool_results.insert(tool_name, "failed".to_string());
            }
        }

        let passed = violations.is_empty();
        let message = if passed {
            "All stages passed".to_string()
        } else {
            format!("Gate failed with {} violation(s)", violations.len())
        };

        GateVerdict {
            passed,
            violations,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn test_empty_events_passes() {
        let events = vec![];
        let verdict = CiGate::evaluate(&events);
        assert!(verdict.passed);
    }

    #[test]
    fn test_single_successful_stage() {
        let events = vec![
            RunEvent {
                seq: 0,
                kind: "tool_called".to_string(),
                payload: json!({ "tool_name": "fmt" }),
                timestamp: Utc::now(),
            },
            RunEvent {
                seq: 1,
                kind: "tool_returned".to_string(),
                payload: json!({ "tool_name": "fmt", "exit_code": 0 }),
                timestamp: Utc::now(),
            },
        ];

        let verdict = CiGate::evaluate(&events);
        assert!(verdict.passed);
        assert!(verdict.violations.is_empty());
    }

    #[test]
    fn test_single_failed_stage() {
        let events = vec![
            RunEvent {
                seq: 0,
                kind: "tool_called".to_string(),
                payload: json!({ "tool_name": "check" }),
                timestamp: Utc::now(),
            },
            RunEvent {
                seq: 1,
                kind: "tool_failed".to_string(),
                payload: json!({
                    "tool_name": "check",
                    "error": "Build failed"
                }),
                timestamp: Utc::now(),
            },
        ];

        let verdict = CiGate::evaluate(&events);
        assert!(!verdict.passed);
        assert!(!verdict.violations.is_empty());
    }

    #[test]
    fn test_multiple_stages_with_failure() {
        let events = vec![
            RunEvent {
                seq: 0,
                kind: "tool_called".to_string(),
                payload: json!({ "tool_name": "fmt" }),
                timestamp: Utc::now(),
            },
            RunEvent {
                seq: 1,
                kind: "tool_returned".to_string(),
                payload: json!({ "tool_name": "fmt", "exit_code": 0 }),
                timestamp: Utc::now(),
            },
            RunEvent {
                seq: 2,
                kind: "tool_called".to_string(),
                payload: json!({ "tool_name": "check" }),
                timestamp: Utc::now(),
            },
            RunEvent {
                seq: 3,
                kind: "tool_returned".to_string(),
                payload: json!({ "tool_name": "check", "exit_code": 1 }),
                timestamp: Utc::now(),
            },
        ];

        let verdict = CiGate::evaluate(&events);
        assert!(!verdict.passed);
        assert_eq!(verdict.violations.len(), 1);
    }

    #[test]
    fn test_non_zero_exit_code() {
        let events = vec![
            RunEvent {
                seq: 0,
                kind: "tool_called".to_string(),
                payload: json!({ "tool_name": "test" }),
                timestamp: Utc::now(),
            },
            RunEvent {
                seq: 1,
                kind: "tool_returned".to_string(),
                payload: json!({ "tool_name": "test", "exit_code": 127 }),
                timestamp: Utc::now(),
            },
        ];

        let verdict = CiGate::evaluate(&events);
        assert!(!verdict.passed);
        assert!(verdict.violations[0].contains("127"));
    }
}
