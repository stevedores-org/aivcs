use aivcs_core::{diff_tool_calls, ToolCallChange};
use chrono::Utc;
use oxidized_state::RunEvent;
use serde_json::json;

fn tool_event(seq: u64, tool_name: &str, params: serde_json::Value) -> RunEvent {
    let mut payload = params;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("tool_name".to_string(), json!(tool_name));
    } else {
        payload = json!({ "tool_name": tool_name });
    }
    RunEvent {
        seq,
        kind: "tool_called".to_string(),
        payload,
        timestamp: Utc::now(),
    }
}

fn non_tool_event(seq: u64) -> RunEvent {
    RunEvent {
        seq,
        kind: "node_entered".to_string(),
        payload: json!({"node_id": "n1"}),
        timestamp: Utc::now(),
    }
}

#[test]
fn identical_calls_produces_empty_diff() {
    let events = vec![tool_event(
        1,
        "search",
        json!({"tool_name": "search", "query": "rust"}),
    )];
    let diff = diff_tool_calls(&events, &events);
    assert!(
        diff.is_empty(),
        "identical sequences should produce no changes"
    );
}

#[test]
fn added_call_detected() {
    let a: Vec<RunEvent> = vec![];
    let b = vec![tool_event(
        1,
        "search",
        json!({"tool_name": "search", "query": "rust"}),
    )];
    let diff = diff_tool_calls(&a, &b);
    assert_eq!(diff.changes.len(), 1);
    assert!(matches!(&diff.changes[0], ToolCallChange::Added(c) if c.tool_name == "search"));
}

#[test]
fn removed_call_detected() {
    let a = vec![tool_event(
        1,
        "search",
        json!({"tool_name": "search", "query": "rust"}),
    )];
    let b: Vec<RunEvent> = vec![];
    let diff = diff_tool_calls(&a, &b);
    assert_eq!(diff.changes.len(), 1);
    assert!(matches!(&diff.changes[0], ToolCallChange::Removed(c) if c.tool_name == "search"));
}

#[test]
fn param_changed_detected() {
    let a = vec![tool_event(
        1,
        "search",
        json!({"tool_name": "search", "query": "rust"}),
    )];
    let b = vec![tool_event(
        1,
        "search",
        json!({"tool_name": "search", "query": "python"}),
    )];
    let diff = diff_tool_calls(&a, &b);
    assert_eq!(diff.changes.len(), 1);
    match &diff.changes[0] {
        ToolCallChange::ParamChanged {
            tool_name, deltas, ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(deltas.len(), 1);
            assert_eq!(deltas[0].key, "query");
            assert_eq!(deltas[0].before, json!("rust"));
            assert_eq!(deltas[0].after, json!("python"));
        }
        other => panic!("expected ParamChanged, got {:?}", other),
    }
}

#[test]
fn reordered_calls_detected() {
    let a = vec![
        tool_event(1, "search", json!({"tool_name": "search", "q": "a"})),
        tool_event(2, "lookup", json!({"tool_name": "lookup", "id": "1"})),
    ];
    let b = vec![
        tool_event(1, "lookup", json!({"tool_name": "lookup", "id": "1"})),
        tool_event(2, "search", json!({"tool_name": "search", "q": "a"})),
    ];
    let diff = diff_tool_calls(&a, &b);
    let reordered: Vec<_> = diff
        .changes
        .iter()
        .filter(|c| matches!(c, ToolCallChange::Reordered { .. }))
        .collect();
    assert!(
        !reordered.is_empty(),
        "should detect reordering when seq positions change"
    );
}

#[test]
fn symmetry_added_becomes_removed() {
    let a: Vec<RunEvent> = vec![];
    let b = vec![tool_event(
        1,
        "search",
        json!({"tool_name": "search", "q": "x"}),
    )];

    let diff_ab = diff_tool_calls(&a, &b);
    let diff_ba = diff_tool_calls(&b, &a);

    let added_count = diff_ab
        .changes
        .iter()
        .filter(|c| matches!(c, ToolCallChange::Added(_)))
        .count();
    let removed_count = diff_ba
        .changes
        .iter()
        .filter(|c| matches!(c, ToolCallChange::Removed(_)))
        .count();

    assert_eq!(added_count, 1);
    assert_eq!(removed_count, 1);
    assert_eq!(added_count, removed_count);
}

#[test]
fn empty_inputs_produce_empty_diff() {
    let empty: Vec<RunEvent> = vec![];
    let diff = diff_tool_calls(&empty, &empty);
    assert!(diff.is_empty());
}

#[test]
fn mixed_changes_all_detected() {
    // A has: search(seq=1), lookup(seq=2)
    // B has: search(seq=1, different param), create(seq=2)
    // Expected: ParamChanged for search, Removed for lookup, Added for create
    let a = vec![
        non_tool_event(0), // should be filtered out
        tool_event(1, "search", json!({"tool_name": "search", "query": "old"})),
        tool_event(2, "lookup", json!({"tool_name": "lookup", "id": "42"})),
    ];
    let b = vec![
        non_tool_event(0),
        tool_event(1, "search", json!({"tool_name": "search", "query": "new"})),
        tool_event(2, "create", json!({"tool_name": "create", "name": "foo"})),
    ];

    let diff = diff_tool_calls(&a, &b);

    let has_param_changed = diff.changes.iter().any(
        |c| matches!(c, ToolCallChange::ParamChanged { tool_name, .. } if tool_name == "search"),
    );
    let has_removed = diff
        .changes
        .iter()
        .any(|c| matches!(c, ToolCallChange::Removed(call) if call.tool_name == "lookup"));
    let has_added = diff
        .changes
        .iter()
        .any(|c| matches!(c, ToolCallChange::Added(call) if call.tool_name == "create"));

    assert!(has_param_changed, "should detect param change on search");
    assert!(has_removed, "should detect removal of lookup");
    assert!(has_added, "should detect addition of create");
}
