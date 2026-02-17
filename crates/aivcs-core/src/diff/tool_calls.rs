use oxidized_state::RunEvent;
use serde_json::Value;
use std::collections::HashMap;

/// A single tool call extracted from a `RunEvent` stream.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub seq: u64,
    pub tool_name: String,
    pub params: Value,
}

/// A single parameter-level delta between two tool calls.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDelta {
    /// Field name, or `"."` for root-level (non-object) changes.
    pub key: String,
    pub before: Value,
    pub after: Value,
}

/// A change detected between two tool-call sequences.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallChange {
    Added(ToolCall),
    Removed(ToolCall),
    Reordered {
        call: ToolCall,
        from_seq: u64,
        to_seq: u64,
    },
    ParamChanged {
        tool_name: String,
        seq_a: u64,
        seq_b: u64,
        deltas: Vec<ParamDelta>,
    },
}

/// The result of diffing two tool-call sequences.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallDiff {
    pub changes: Vec<ToolCallChange>,
}

impl ToolCallDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

fn extract_tool_calls(events: &[RunEvent]) -> Vec<ToolCall> {
    events
        .iter()
        .filter(|e| e.kind == "tool_called")
        .map(|e| ToolCall {
            seq: e.seq,
            tool_name: e
                .payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            params: e.payload.clone(),
        })
        .collect()
}

fn group_by_name(calls: Vec<ToolCall>) -> HashMap<String, Vec<ToolCall>> {
    let mut map: HashMap<String, Vec<ToolCall>> = HashMap::new();
    for call in calls {
        map.entry(call.tool_name.clone()).or_default().push(call);
    }
    map
}

// ---------------------------------------------------------------------------
// Param diffing
// ---------------------------------------------------------------------------

fn param_delta(a: &Value, b: &Value) -> Vec<ParamDelta> {
    if a == b {
        return Vec::new();
    }
    match (a.as_object(), b.as_object()) {
        (Some(obj_a), Some(obj_b)) => {
            let mut deltas = Vec::new();
            // Keys in A
            for (key, val_a) in obj_a {
                match obj_b.get(key) {
                    Some(val_b) if val_a != val_b => {
                        deltas.push(ParamDelta {
                            key: key.clone(),
                            before: val_a.clone(),
                            after: val_b.clone(),
                        });
                    }
                    None => {
                        deltas.push(ParamDelta {
                            key: key.clone(),
                            before: val_a.clone(),
                            after: Value::Null,
                        });
                    }
                    _ => {}
                }
            }
            // Keys only in B
            for (key, val_b) in obj_b {
                if !obj_a.contains_key(key) {
                    deltas.push(ParamDelta {
                        key: key.clone(),
                        before: Value::Null,
                        after: val_b.clone(),
                    });
                }
            }
            deltas.sort_by(|a, b| a.key.cmp(&b.key));
            deltas
        }
        _ => vec![ParamDelta {
            key: ".".to_string(),
            before: a.clone(),
            after: b.clone(),
        }],
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Diff two ordered `RunEvent` sequences, producing a `ToolCallDiff` that
/// captures added, removed, reordered, and param-changed tool calls.
pub fn diff_tool_calls(a: &[RunEvent], b: &[RunEvent]) -> ToolCallDiff {
    let calls_a = extract_tool_calls(a);
    let calls_b = extract_tool_calls(b);

    let group_a = group_by_name(calls_a);
    let group_b = group_by_name(calls_b);

    let mut changes = Vec::new();

    // Collect all tool names from both sides.
    let mut all_names: Vec<&String> = group_a.keys().chain(group_b.keys()).collect();
    all_names.sort();
    all_names.dedup();

    for name in all_names {
        let empty = Vec::new();
        let list_a = group_a.get(name).unwrap_or(&empty);
        let list_b = group_b.get(name).unwrap_or(&empty);

        match (list_a.is_empty(), list_b.is_empty()) {
            (true, false) => {
                for call in list_b {
                    changes.push(ToolCallChange::Added(call.clone()));
                }
            }
            (false, true) => {
                for call in list_a {
                    changes.push(ToolCallChange::Removed(call.clone()));
                }
            }
            _ => {
                let paired = list_a.len().min(list_b.len());

                for i in 0..paired {
                    let ca = &list_a[i];
                    let cb = &list_b[i];

                    // Check param changes
                    let deltas = param_delta(&ca.params, &cb.params);
                    if !deltas.is_empty() {
                        changes.push(ToolCallChange::ParamChanged {
                            tool_name: name.clone(),
                            seq_a: ca.seq,
                            seq_b: cb.seq,
                            deltas,
                        });
                    }

                    // Check reorder: compare relative position (seq) shift
                    if ca.seq != cb.seq {
                        changes.push(ToolCallChange::Reordered {
                            call: cb.clone(),
                            from_seq: ca.seq,
                            to_seq: cb.seq,
                        });
                    }
                }

                // Extra in A → Removed
                for call in list_a.iter().skip(paired) {
                    changes.push(ToolCallChange::Removed(call.clone()));
                }
                // Extra in B → Added
                for call in list_b.iter().skip(paired) {
                    changes.push(ToolCallChange::Added(call.clone()));
                }
            }
        }
    }

    ToolCallDiff { changes }
}
