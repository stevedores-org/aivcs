//! Validates the aivcs-eval.yml workflow template structure.
//!
//! These tests ensure the reusable eval workflow has the required
//! triggers, inputs, outputs, jobs, and artifact upload steps.

use std::path::Path;

fn workflow_content() -> String {
    // Navigate from the test binary's crate root to the workspace .github dir
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let path = workspace_root.join(".github/workflows/aivcs-eval.yml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

#[test]
fn workflow_file_exists_and_is_nonempty() {
    let content = workflow_content();
    assert!(
        !content.is_empty(),
        "aivcs-eval.yml should exist and be non-empty"
    );
}

#[test]
fn workflow_is_callable_via_workflow_call() {
    let content = workflow_content();
    assert!(
        content.contains("workflow_call"),
        "must be a reusable workflow (workflow_call trigger)"
    );
}

#[test]
fn workflow_has_eval_suite_input() {
    let content = workflow_content();
    assert!(
        content.contains("eval-suite"),
        "must accept eval-suite input"
    );
}

#[test]
fn workflow_has_fail_on_gate_input() {
    let content = workflow_content();
    assert!(
        content.contains("fail-on-gate"),
        "must accept fail-on-gate input"
    );
}

#[test]
fn workflow_has_gate_passed_output() {
    let content = workflow_content();
    assert!(
        content.contains("gate-passed"),
        "must expose gate-passed output"
    );
}

#[test]
fn workflow_uploads_artifacts() {
    let content = workflow_content();
    assert!(
        content.contains("actions/upload-artifact"),
        "must upload eval artifacts"
    );
    assert!(
        content.contains("aivcs-eval-results"),
        "artifact name must be aivcs-eval-results"
    );
}

#[test]
fn workflow_runs_cargo_test() {
    let content = workflow_content();
    assert!(
        content.contains("cargo test"),
        "must run cargo test as part of eval"
    );
}

#[test]
fn workflow_has_gate_enforcement_step() {
    let content = workflow_content();
    assert!(
        content.contains("Enforce gate"),
        "must have gate enforcement step"
    );
}
