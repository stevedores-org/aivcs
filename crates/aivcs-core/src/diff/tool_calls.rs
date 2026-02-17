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
///
/// The `key` uses dot-separated JSON paths (e.g. `"config.retries"`) for
/// nested object fields. Root-level non-object changes use `"."`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDelta {
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
        from_index: usize,
        to_index: usize,
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
        .filter_map(|e| {
            let tool_name = e
                .payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())?;
            Some(ToolCall {
                seq: e.seq,
                tool_name,
                params: e.payload.clone(),
            })
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
// Param diffing (recursive)
// ---------------------------------------------------------------------------

fn param_delta_recursive(prefix: &str, a: &Value, b: &Value, out: &mut Vec<ParamDelta>) {
    if a == b {
        return;
    }
    match (a.as_object(), b.as_object()) {
        (Some(obj_a), Some(obj_b)) => {
            let mut all_keys: Vec<&String> = obj_a.keys().chain(obj_b.keys()).collect();
            all_keys.sort();
            all_keys.dedup();
            for key in all_keys {
                let child_path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                let val_a = obj_a.get(key).unwrap_or(&Value::Null);
                let val_b = obj_b.get(key).unwrap_or(&Value::Null);
                param_delta_recursive(&child_path, val_a, val_b, out);
            }
        }
        _ => {
            let key = if prefix.is_empty() {
                ".".to_string()
            } else {
                prefix.to_string()
            };
            out.push(ParamDelta {
                key,
                before: a.clone(),
                after: b.clone(),
            });
        }
    }
}

fn param_delta(a: &Value, b: &Value) -> Vec<ParamDelta> {
    let mut deltas = Vec::new();
    param_delta_recursive("", a, b, &mut deltas);
    deltas
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Diff two ordered `RunEvent` sequences, producing a `ToolCallDiff` that
/// captures added, removed, reordered, and param-changed tool calls.
///
/// Events with `kind == "tool_called"` that lack a valid `payload.tool_name`
/// string are silently skipped. Reorder detection uses the relative position
/// of each tool call within the extracted tool-call stream (not the global
/// run-level `seq`), so inserting or removing non-tool events between runs
/// does not cause false positives.
pub fn diff_tool_calls(a: &[RunEvent], b: &[RunEvent]) -> ToolCallDiff {
    let calls_a = extract_tool_calls(a);
    let calls_b = extract_tool_calls(b);

    // Build a map from tool_name to relative position within the full
    // tool-call stream (across all names). This is the basis for reorder
    // detection — it is independent of the global run-event seq.
    let position_a: HashMap<u64, usize> = calls_a
        .iter()
        .enumerate()
        .map(|(i, c)| (c.seq, i))
        .collect();
    let position_b: HashMap<u64, usize> = calls_b
        .iter()
        .enumerate()
        .map(|(i, c)| (c.seq, i))
        .collect();

    let group_a = group_by_name(calls_a);
    let group_b = group_by_name(calls_b);

    let mut changes = Vec::new();

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

                    // Check reorder by relative position in the tool-call stream
                    let idx_a = position_a.get(&ca.seq).copied().unwrap_or(0);
                    let idx_b = position_b.get(&cb.seq).copied().unwrap_or(0);
                    if idx_a != idx_b {
                        changes.push(ToolCallChange::Reordered {
                            call: cb.clone(),
                            from_index: idx_a,
                            to_index: idx_b,
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
