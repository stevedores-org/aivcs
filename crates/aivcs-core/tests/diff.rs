//! Integration tests for tool-call sequence diffing.

use aivcs_core::diff_tool_calls;
use aivcs_core::ToolCallChange;
use chrono::Utc;
use oxidized_state::storage_traits::RunEvent;

fn make_tool_event(
    seq: u64,
    tool_name: &str,
    extra_payload: Option<serde_json::Value>,
) -> RunEvent {
    let mut payload = serde_json::json!({
        "tool_name": tool_name,
    });
    if let Some(extra) = extra_payload {
        if let serde_json::Value::Object(ref mut obj) = payload {
            if let serde_json::Value::Object(ref extra_obj) = extra {
                for (k, v) in extra_obj.iter() {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
    }
    RunEvent {
        seq,
        kind: "tool_called".to_string(),
        payload,
        timestamp: Utc::now(),
    }
}

#[test]
fn test_identical_runs_no_diff() {
    let events_a = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "fetch", None),
    ];
    let events_b = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "fetch", None),
    ];

    let diff = diff_tool_calls(&events_a, &events_b);

    assert!(diff.is_empty(), "Expected identical diff");
    assert!(
        diff.changes.is_empty(),
        "Expected no changes, got: {:?}",
        diff.changes
    );
}

#[test]
fn test_tool_added() {
    let events_a = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "fetch", None),
    ];
    let events_b = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "translate", None),
        make_tool_event(3, "fetch", None),
    ];

    let diff = diff_tool_calls(&events_a, &events_b);

    assert!(!diff.is_empty(), "Expected non-identical diff");

    let added: Vec<_> = diff
        .changes
        .iter()
        .filter(|c| matches!(c, ToolCallChange::Added(_)))
        .collect();
    assert!(
        !added.is_empty(),
        "Expected at least one Added change, got: {:?}",
        diff.changes
    );

    match &added[0] {
        ToolCallChange::Added(call) => {
            assert_eq!(call.tool_name, "translate");
        }
        other => panic!("Expected Added, got {:?}", other),
    }
}

#[test]
fn test_tool_removed() {
    let events_a = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "translate", None),
        make_tool_event(3, "fetch", None),
    ];
    let events_b = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "fetch", None),
    ];

    let diff = diff_tool_calls(&events_a, &events_b);

    assert!(!diff.is_empty(), "Expected non-identical diff");

    let removed: Vec<_> = diff
        .changes
        .iter()
        .filter(|c| matches!(c, ToolCallChange::Removed(_)))
        .collect();
    assert!(
        !removed.is_empty(),
        "Expected at least one Removed change, got: {:?}",
        diff.changes
    );

    match &removed[0] {
        ToolCallChange::Removed(call) => {
            assert_eq!(call.tool_name, "translate");
        }
        other => panic!("Expected Removed, got {:?}", other),
    }
}

#[test]
fn test_param_delta() {
    let events_a = vec![make_tool_event(
        1,
        "search",
        Some(serde_json::json!({"query": "cats"})),
    )];
    let events_b = vec![make_tool_event(
        1,
        "search",
        Some(serde_json::json!({"query": "dogs"})),
    )];

    let diff = diff_tool_calls(&events_a, &events_b);

    assert!(!diff.is_empty(), "Expected non-identical diff");

    let param_changes: Vec<_> = diff
        .changes
        .iter()
        .filter(|c| matches!(c, ToolCallChange::ParamChanged { .. }))
        .collect();
    assert!(
        !param_changes.is_empty(),
        "Expected at least one ParamChanged, got: {:?}",
        diff.changes
    );

    match &param_changes[0] {
        ToolCallChange::ParamChanged {
            tool_name, deltas, ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(deltas.len(), 1, "Expected 1 param change");
            assert_eq!(deltas[0].key, "query");
        }
        other => panic!("Expected ParamChanged, got {:?}", other),
    }
}

#[test]
fn test_symmetry_property() {
    let events_a = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "fetch", None),
    ];
    let events_b = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "translate", None),
        make_tool_event(3, "fetch", None),
    ];

    let diff_ab = diff_tool_calls(&events_a, &events_b);
    let diff_ba = diff_tool_calls(&events_b, &events_a);

    // diff(a,b) should have Added { translate }
    assert!(diff_ab
        .changes
        .iter()
        .any(|c| matches!(c, ToolCallChange::Added(call) if call.tool_name == "translate")));

    // diff(b,a) should have Removed { translate }
    assert!(diff_ba
        .changes
        .iter()
        .any(|c| matches!(c, ToolCallChange::Removed(call) if call.tool_name == "translate")));
}

#[test]
fn test_empty_vs_nonempty() {
    let events_a = vec![];
    let events_b = vec![
        make_tool_event(1, "search", None),
        make_tool_event(2, "fetch", None),
    ];

    let diff = diff_tool_calls(&events_a, &events_b);

    assert!(!diff.is_empty(), "Expected non-identical diff");
    assert_eq!(
        diff.changes.len(),
        2,
        "Expected 2 changes, got: {:?}",
        diff.changes
    );

    for change in &diff.changes {
        assert!(
            matches!(change, ToolCallChange::Added(_)),
            "Expected all changes to be Added, got: {:?}",
            change
        );
    }
}
