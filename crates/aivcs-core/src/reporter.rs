//! PR summary reporter artifacts.
//!
//! Provides two output artifacts for CI consumers:
//! - `EvalResults` — machine-readable per-case eval outcomes + aggregate stats (eval_results.json)
//! - `DiffReport` — human-readable Markdown comparison of two runs (diff_summary.md)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::diff::node_paths::NodePathDiff;
use crate::diff::state_diff::ScopedStateDiff;
use crate::diff::tool_calls::{ToolCallChange, ToolCallDiff};
use crate::domain::eval::EvalSuite;

// ── eval_results.json schema ──────────────────────────────────────────────

/// Outcome of a single eval test case.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaseOutcome {
    pub case_id: Uuid,
    pub tags: Vec<String>,
    pub passed: bool,
    pub score: f32,
    pub reason: Option<String>,
}

/// Aggregate evaluation results for an entire suite run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalResults {
    pub suite_id: Uuid,
    pub suite_name: String,
    pub suite_version: String,
    pub run_at: DateTime<Utc>,
    pub outcomes: Vec<CaseOutcome>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f32,
}

impl EvalResults {
    /// Build `EvalResults` from a suite and a vec of per-case outcomes.
    pub fn new(suite: &EvalSuite, outcomes: Vec<CaseOutcome>) -> Self {
        let total = outcomes.len();
        let passed = outcomes.iter().filter(|o| o.passed).count();
        let failed = total - passed;
        let pass_rate = if total == 0 {
            0.0
        } else {
            passed as f32 / total as f32
        };

        Self {
            suite_id: suite.suite_id,
            suite_name: suite.name.clone(),
            suite_version: suite.version.clone(),
            run_at: Utc::now(),
            outcomes,
            total,
            passed,
            failed,
            pass_rate,
        }
    }
}

// ── diff_summary.md rendering ─────────────────────────────────────────────

/// A diff report comparing two runs across tool calls, node paths, and state.
pub struct DiffReport<'a> {
    pub run_id_a: &'a str,
    pub run_id_b: &'a str,
    pub tool_call_diff: Option<&'a ToolCallDiff>,
    pub node_path_diff: Option<&'a NodePathDiff>,
    pub state_diff: Option<&'a ScopedStateDiff>,
}

impl<'a> DiffReport<'a> {
    /// Render the diff report as a Markdown string.
    pub fn render_markdown(&self) -> String {
        let mut md = format!("# Diff Summary: {} vs {}\n", self.run_id_a, self.run_id_b);

        // Tool Calls section
        md.push_str("\n## Tool Calls\n\n");
        match self.tool_call_diff {
            Some(diff) if !diff.is_empty() => {
                for change in &diff.changes {
                    match change {
                        ToolCallChange::Added(call) => {
                            md.push_str(&format!("- **Added**: `{}`\n", call.tool_name));
                        }
                        ToolCallChange::Removed(call) => {
                            md.push_str(&format!("- **Removed**: `{}`\n", call.tool_name));
                        }
                        ToolCallChange::Reordered {
                            call,
                            from_index,
                            to_index,
                        } => {
                            md.push_str(&format!(
                                "- **Reordered**: `{}` (index {} → {})\n",
                                call.tool_name, from_index, to_index
                            ));
                        }
                        ToolCallChange::ParamChanged {
                            tool_name, deltas, ..
                        } => {
                            md.push_str(&format!(
                                "- **ParamChanged**: `{}` ({} delta(s))\n",
                                tool_name,
                                deltas.len()
                            ));
                        }
                    }
                }
            }
            _ => {
                md.push_str("identical\n");
            }
        }

        // Node Path section
        md.push_str("\n## Node Path\n\n");
        match self.node_path_diff {
            Some(diff) if !diff.is_empty() => {
                if let Some(ref div) = diff.divergence {
                    md.push_str(&format!(
                        "- Common prefix: [{}]\n",
                        div.common_prefix.join(", ")
                    ));
                    let tail_a_ids: Vec<&str> =
                        div.tail_a.iter().map(|s| s.node_id.as_str()).collect();
                    let tail_b_ids: Vec<&str> =
                        div.tail_b.iter().map(|s| s.node_id.as_str()).collect();
                    md.push_str(&format!("- tail_a: [{}]\n", tail_a_ids.join(", ")));
                    md.push_str(&format!("- tail_b: [{}]\n", tail_b_ids.join(", ")));
                }
            }
            _ => {
                md.push_str("identical\n");
            }
        }

        // State section
        md.push_str("\n## State\n\n");
        match self.state_diff {
            Some(diff) if !diff.is_empty() => {
                md.push_str(&format!("{} delta(s):\n", diff.deltas.len()));
                for delta in &diff.deltas {
                    md.push_str(&format!(
                        "- `{}`: {} → {}\n",
                        delta.pointer, delta.before, delta.after
                    ));
                }
            }
            _ => {
                md.push_str("identical\n");
            }
        }

        md
    }
}
