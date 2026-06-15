use oxidized_state::RunEvent;
use serde_json::Value;
use std::collections::HashSet;

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

// ---------------------------------------------------------------------------
// Alignment (LCS)
// ---------------------------------------------------------------------------

fn lcs_alignment(calls_a: &[ToolCall], calls_b: &[ToolCall]) -> Vec<(usize, usize)> {
    let m = calls_a.len();
    let n = calls_b.len();

    if m == 0 || n == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0usize; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if calls_a[i - 1].tool_name == calls_b[j - 1].tool_name {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i][j - 1].max(dp[i - 1][j]);
            }
        }
    }

    let mut alignment = Vec::new();
    let mut i = m;
    let mut j = n;

    while i > 0 && j > 0 {
        if calls_a[i - 1].tool_name == calls_b[j - 1].tool_name {
            alignment.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i][j - 1] > dp[i - 1][j] {
            j -= 1;
        } else {
            i -= 1;
        }
    }

    alignment.reverse();
    alignment
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

/// Diff two ordered `RunEvent` sequences, producing a `ToolCallDiff`.
///
/// Uses Longest Common Subsequence (LCS) alignment on tool names to detect
/// added, removed, and reordered tool calls accurately.
pub fn diff_tool_calls(a: &[RunEvent], b: &[RunEvent]) -> ToolCallDiff {
    let calls_a = extract_tool_calls(a);
    let calls_b = extract_tool_calls(b);

    let alignment = lcs_alignment(&calls_a, &calls_b);

    let mut aligned_a = HashSet::new();
    let mut aligned_b = HashSet::new();
    for (i_a, i_b) in &alignment {
        aligned_a.insert(*i_a);
        aligned_b.insert(*i_b);
    }

    let mut changes = Vec::new();

    // 1. Param changes on aligned calls.
    //
    // Reordering is deliberately NOT derived from the aligned pairs: LCS
    // alignment is monotonically increasing in both index sequences, so an
    // aligned pair can never represent a reorder. The previous `i_a != i_b`
    // test only reflected index shifts caused by surrounding insertions or
    // removals and emitted a spurious `Reordered` change for every aligned
    // call that followed an add/remove. True reorders are detected below: a
    // tool that disappears from A and reappears (same name) in B.
    for (i_a, i_b) in alignment {
        let ca = &calls_a[i_a];
        let cb = &calls_b[i_b];

        let deltas = param_delta(&ca.params, &cb.params);
        if !deltas.is_empty() {
            changes.push(ToolCallChange::ParamChanged {
                tool_name: ca.tool_name.clone(),
                seq_a: ca.seq,
                seq_b: cb.seq,
                deltas,
            });
        }
    }

    // 2. Reconcile unaligned calls. A tool name present in A but not aligned,
    // that reappears unaligned in B, is a reorder; the rest are genuine
    // removals (only in A) or additions (only in B).
    let mut matched_b: HashSet<usize> = HashSet::new();
    for (i_a, call_a) in calls_a.iter().enumerate() {
        if aligned_a.contains(&i_a) {
            continue;
        }
        let reorder_target = (0..calls_b.len()).find(|i_b| {
            !aligned_b.contains(i_b)
                && !matched_b.contains(i_b)
                && calls_b[*i_b].tool_name == call_a.tool_name
        });
        match reorder_target {
            Some(i_b) => {
                matched_b.insert(i_b);
                changes.push(ToolCallChange::Reordered {
                    call: calls_b[i_b].clone(),
                    from_index: i_a,
                    to_index: i_b,
                });
            }
            None => changes.push(ToolCallChange::Removed(call_a.clone())),
        }
    }

    // 3. Unaligned B calls not matched as reorders are genuine additions.
    for (i_b, call_b) in calls_b.iter().enumerate() {
        if !aligned_b.contains(&i_b) && !matched_b.contains(&i_b) {
            changes.push(ToolCallChange::Added(call_b.clone()));
        }
    }

    ToolCallDiff { changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn tool_event(seq: u64, name: &str) -> RunEvent {
        RunEvent {
            seq,
            kind: "tool_called".to_string(),
            payload: json!({ "tool_name": name }),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn insertion_does_not_produce_spurious_reorders() {
        // A = [search, fetch]; B = [translate, search, fetch].
        // Only `translate` was added; nothing was reordered. The old
        // absolute-index check reported two phantom `Reordered` changes here.
        let a = vec![tool_event(1, "search"), tool_event(2, "fetch")];
        let b = vec![
            tool_event(1, "translate"),
            tool_event(2, "search"),
            tool_event(3, "fetch"),
        ];
        let diff = diff_tool_calls(&a, &b);
        assert_eq!(diff.changes.len(), 1);
        assert!(matches!(diff.changes[0], ToolCallChange::Added(_)));
    }

    #[test]
    fn swapped_calls_report_a_single_reorder() {
        // A = [search, fetch]; B = [fetch, search] — a genuine reorder.
        let a = vec![tool_event(1, "search"), tool_event(2, "fetch")];
        let b = vec![tool_event(1, "fetch"), tool_event(2, "search")];
        let diff = diff_tool_calls(&a, &b);
        let reorders = diff
            .changes
            .iter()
            .filter(|c| matches!(c, ToolCallChange::Reordered { .. }))
            .count();
        assert_eq!(reorders, 1);
        // No phantom add/remove for the reordered tool.
        assert!(!diff
            .changes
            .iter()
            .any(|c| matches!(c, ToolCallChange::Added(_) | ToolCallChange::Removed(_))));
    }
}
