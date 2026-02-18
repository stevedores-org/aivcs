use aivcs_core::diff::node_paths::{NodeDivergence, NodePathDiff, NodeStep};
use aivcs_core::diff::state_diff::{ScopedStateDiff, StateDelta};
use aivcs_core::diff::tool_calls::{ToolCall, ToolCallChange, ToolCallDiff};
use aivcs_core::{CaseOutcome, DiffReport, EvalResults, EvalSuite};
use serde_json::{json, Value};
use uuid::Uuid;

fn sample_suite() -> EvalSuite {
    EvalSuite::new("my-suite".to_string(), "1.0.0".to_string())
}

fn case_outcome(passed: bool, score: f32) -> CaseOutcome {
    CaseOutcome {
        case_id: Uuid::new_v4(),
        tags: vec!["smoke".to_string()],
        passed,
        score,
        reason: if passed {
            None
        } else {
            Some("mismatch".to_string())
        },
    }
}

// ── EvalResults tests ─────────────────────────────────────────────────────

#[test]
fn eval_results_serde_roundtrip() {
    let suite = sample_suite();
    let outcomes = vec![case_outcome(true, 1.0), case_outcome(false, 0.3)];
    let results = EvalResults::new(&suite, outcomes);

    let json_str = serde_json::to_string(&results).expect("serialize");
    let deserialized: EvalResults = serde_json::from_str(&json_str).expect("deserialize");

    assert_eq!(results, deserialized);
}

#[test]
fn eval_results_schema_fields_in_json() {
    let suite = sample_suite();
    let results = EvalResults::new(&suite, vec![case_outcome(true, 1.0)]);

    let v: Value = serde_json::to_value(&results).expect("to_value");
    let obj = v.as_object().expect("top-level object");

    for key in &[
        "suite_id",
        "suite_name",
        "suite_version",
        "run_at",
        "outcomes",
        "total",
        "passed",
        "failed",
        "pass_rate",
    ] {
        assert!(obj.contains_key(*key), "missing key: {}", key);
    }
}

#[test]
fn eval_results_pass_rate_all_pass() {
    let suite = sample_suite();
    let outcomes = vec![
        case_outcome(true, 1.0),
        case_outcome(true, 0.9),
        case_outcome(true, 1.0),
    ];
    let results = EvalResults::new(&suite, outcomes);

    assert_eq!(results.total, 3);
    assert_eq!(results.passed, 3);
    assert_eq!(results.failed, 0);
    assert!((results.pass_rate - 1.0).abs() < f32::EPSILON);
}

#[test]
fn eval_results_pass_rate_mixed() {
    let suite = sample_suite();
    let outcomes = vec![
        case_outcome(true, 1.0),
        case_outcome(false, 0.2),
        case_outcome(true, 0.8),
        case_outcome(false, 0.1),
    ];
    let results = EvalResults::new(&suite, outcomes);

    assert_eq!(results.total, 4);
    assert_eq!(results.passed, 2);
    assert_eq!(results.failed, 2);
    assert!((results.pass_rate - 0.5).abs() < f32::EPSILON);
}

#[test]
fn eval_results_empty_outcomes_zero_pass_rate() {
    let suite = sample_suite();
    let results = EvalResults::new(&suite, vec![]);

    assert_eq!(results.total, 0);
    assert_eq!(results.passed, 0);
    assert_eq!(results.failed, 0);
    assert!((results.pass_rate - 0.0).abs() < f32::EPSILON);
}

// ── DiffReport tests ─────────────────────────────────────────────────────

#[test]
fn diff_report_render_identical_runs() {
    let report = DiffReport {
        run_id_a: "run-A",
        run_id_b: "run-B",
        tool_call_diff: None,
        node_path_diff: None,
        state_diff: None,
    };
    let md = report.render_markdown();

    assert!(md.contains("# Diff Summary: run-A vs run-B"));
    assert!(md.contains("## Tool Calls"));
    assert!(md.contains("## Node Path"));
    assert!(md.contains("## State"));
    // All sections should say identical
    let identical_count = md.matches("identical").count();
    assert_eq!(identical_count, 3, "all 3 sections should say identical");
}

#[test]
fn diff_report_render_tool_call_added() {
    let diff = ToolCallDiff {
        changes: vec![ToolCallChange::Added(ToolCall {
            seq: 1,
            tool_name: "new_tool".to_string(),
            params: json!({}),
        })],
    };
    let report = DiffReport {
        run_id_a: "a",
        run_id_b: "b",
        tool_call_diff: Some(&diff),
        node_path_diff: None,
        state_diff: None,
    };
    let md = report.render_markdown();

    assert!(md.contains("**Added**: `new_tool`"));
}

#[test]
fn diff_report_render_tool_call_removed() {
    let diff = ToolCallDiff {
        changes: vec![ToolCallChange::Removed(ToolCall {
            seq: 2,
            tool_name: "old_tool".to_string(),
            params: json!({}),
        })],
    };
    let report = DiffReport {
        run_id_a: "a",
        run_id_b: "b",
        tool_call_diff: Some(&diff),
        node_path_diff: None,
        state_diff: None,
    };
    let md = report.render_markdown();

    assert!(md.contains("**Removed**: `old_tool`"));
}

#[test]
fn diff_report_render_node_divergence() {
    let diff = NodePathDiff {
        divergence: Some(NodeDivergence {
            common_prefix: vec!["start".to_string(), "middle".to_string()],
            tail_a: vec![NodeStep {
                seq: 3,
                node_id: "left".to_string(),
            }],
            tail_b: vec![NodeStep {
                seq: 3,
                node_id: "right".to_string(),
            }],
        }),
    };
    let report = DiffReport {
        run_id_a: "a",
        run_id_b: "b",
        tool_call_diff: None,
        node_path_diff: Some(&diff),
        state_diff: None,
    };
    let md = report.render_markdown();

    assert!(md.contains("Common prefix: [start, middle]"));
    assert!(md.contains("tail_a: [left]"));
    assert!(md.contains("tail_b: [right]"));
}

#[test]
fn diff_report_render_state_delta() {
    let diff = ScopedStateDiff {
        deltas: vec![StateDelta {
            pointer: "/config/retries".to_string(),
            before: json!(3),
            after: json!(5),
        }],
    };
    let report = DiffReport {
        run_id_a: "a",
        run_id_b: "b",
        tool_call_diff: None,
        node_path_diff: None,
        state_diff: Some(&diff),
    };
    let md = report.render_markdown();

    assert!(md.contains("1 delta(s)"));
    assert!(md.contains("`/config/retries`: 3 → 5"));
}
