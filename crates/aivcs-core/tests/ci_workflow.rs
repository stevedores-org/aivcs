//! Validates CI workflow guardrails that prevent stale/duplicated runs.

use std::path::Path;

fn ci_workflow_content() -> String {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let path = workspace_root.join(".github/workflows/ci.yml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

#[test]
fn ci_workflow_has_concurrency_control() {
    let content = ci_workflow_content();
    assert!(
        content.contains("concurrency:"),
        "ci workflow should define concurrency control"
    );
    assert!(
        content.contains("cancel-in-progress: true"),
        "ci workflow should cancel superseded runs"
    );
}
