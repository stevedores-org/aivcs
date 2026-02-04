//! Semantic-RAG-Merge: Semantic Merging with RAG and LLM Arbiter
//!
//! This crate provides the semantic version control features for AIVCS,
//! allowing intelligent merging of divergent agent states and memory.
//!
//! ## Layer 3 - VCS Logic
//!
//! Focus: Semantic conflict resolution and memory synthesis.

use anyhow::Result;
use oxidized_state::{CommitId, MemoryRecord, SurrealHandle};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Difference between two memory vector stores
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStoreDelta {
    /// Memories only in commit A
    pub only_in_a: Vec<MemoryRecord>,
    /// Memories only in commit B
    pub only_in_b: Vec<MemoryRecord>,
    /// Memories that differ between A and B (same key, different content)
    pub conflicts: Vec<MemoryConflict>,
}

/// A conflict between two memory records
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConflict {
    /// Memory key
    pub key: String,
    /// Memory from commit A
    pub memory_a: MemoryRecord,
    /// Memory from commit B
    pub memory_b: MemoryRecord,
}

/// Result of automatic conflict resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResolvedValue {
    /// The resolved value
    pub value: serde_json::Value,
    /// Which branch the resolution favored (if any)
    pub favored_branch: Option<String>,
    /// Reasoning for the resolution
    pub reasoning: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
}

/// Result of a semantic merge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    /// The new merge commit ID
    pub merge_commit_id: CommitId,
    /// Number of automatic resolutions
    pub auto_resolved: usize,
    /// Any conflicts that couldn't be auto-resolved
    pub manual_conflicts: Vec<MemoryConflict>,
    /// Summary of the merge
    pub summary: String,
}

/// Diff memory vectors between two commits
///
/// # TDD: test_memory_diff_shows_only_new_vectors
pub async fn diff_memory_vectors(
    handle: &SurrealHandle,
    commit_a: &str,
    commit_b: &str,
) -> Result<VectorStoreDelta> {
    let memories_a = handle.get_memories(commit_a).await?;
    let memories_b = handle.get_memories(commit_b).await?;

    let keys_a: std::collections::HashSet<_> = memories_a.iter().map(|m| &m.key).collect();
    let keys_b: std::collections::HashSet<_> = memories_b.iter().map(|m| &m.key).collect();

    let only_in_a: Vec<_> = memories_a
        .iter()
        .filter(|m| !keys_b.contains(&m.key))
        .cloned()
        .collect();

    let only_in_b: Vec<_> = memories_b
        .iter()
        .filter(|m| !keys_a.contains(&m.key))
        .cloned()
        .collect();

    // Find conflicts (same key, different content)
    let mut conflicts = Vec::new();
    for mem_a in &memories_a {
        if let Some(mem_b) = memories_b.iter().find(|m| m.key == mem_a.key) {
            if mem_a.content != mem_b.content {
                conflicts.push(MemoryConflict {
                    key: mem_a.key.clone(),
                    memory_a: mem_a.clone(),
                    memory_b: mem_b.clone(),
                });
            }
        }
    }

    Ok(VectorStoreDelta {
        only_in_a,
        only_in_b,
        conflicts,
    })
}

/// Resolve a state conflict using LLM Arbiter
///
/// # TDD: test_arbiter_resolves_value_conflict_based_on_CoT
pub async fn resolve_conflict_state(
    _trace_a: &[serde_json::Value],
    _trace_b: &[serde_json::Value],
    conflict: &MemoryConflict,
) -> Result<AutoResolvedValue> {
    // TODO: Implement LLM-based conflict resolution
    // For now, use a simple heuristic: prefer the longer content

    let (value, favored, reasoning) = if conflict.memory_a.content.len() >= conflict.memory_b.content.len() {
        (
            serde_json::json!({"content": conflict.memory_a.content}),
            Some("A".to_string()),
            "Chose branch A: more detailed content".to_string(),
        )
    } else {
        (
            serde_json::json!({"content": conflict.memory_b.content}),
            Some("B".to_string()),
            "Chose branch B: more detailed content".to_string(),
        )
    };

    Ok(AutoResolvedValue {
        value,
        favored_branch: favored,
        reasoning,
        confidence: 0.6, // Low confidence for heuristic resolution
    })
}

/// Synthesize two memory stores into one
///
/// # TDD: test_merge_synthesizes_two_memories_into_one_new_commit
pub async fn synthesize_memory(
    handle: &SurrealHandle,
    commit_a: &str,
    commit_b: &str,
    new_commit_id: &str,
) -> Result<Vec<MemoryRecord>> {
    let delta = diff_memory_vectors(handle, commit_a, commit_b).await?;

    let mut merged_memories = Vec::new();

    // Include all memories unique to A
    for mut mem in delta.only_in_a {
        mem.commit_id = new_commit_id.to_string();
        mem.id = None;
        merged_memories.push(mem);
    }

    // Include all memories unique to B
    for mut mem in delta.only_in_b {
        mem.commit_id = new_commit_id.to_string();
        mem.id = None;
        merged_memories.push(mem);
    }

    // Resolve conflicts
    for conflict in delta.conflicts {
        let resolved = resolve_conflict_state(&[], &[], &conflict).await?;
        let merged_mem = MemoryRecord::new(
            new_commit_id,
            &conflict.key,
            resolved.value.get("content")
                .and_then(|v| v.as_str())
                .unwrap_or(&conflict.memory_a.content),
        )
        .with_metadata(serde_json::json!({
            "merged_from": [commit_a, commit_b],
            "resolution": resolved.reasoning,
            "confidence": resolved.confidence,
        }));
        merged_memories.push(merged_mem);
    }

    Ok(merged_memories)
}

/// Perform a semantic merge of two branches
pub async fn semantic_merge(
    handle: &SurrealHandle,
    commit_a: &str,
    commit_b: &str,
    message: &str,
    author: &str,
) -> Result<MergeResult> {
    // Create the merge commit ID
    let state_data = format!("merge:{}:{}", commit_a, commit_b);
    let merge_commit_id = CommitId::from_state(state_data.as_bytes());

    // Synthesize memories
    let merged_memories = synthesize_memory(handle, commit_a, commit_b, &merge_commit_id.hash).await?;

    // Save merged memories
    for mem in &merged_memories {
        handle.save_memory(mem).await?;
    }

    // Create merge commit record
    let commit = oxidized_state::CommitRecord::new(
        merge_commit_id.clone(),
        Some(commit_a.to_string()), // Primary parent
        message,
        author,
    );
    handle.save_commit(&commit).await?;

    // Save graph edges for both parents
    handle.save_commit_graph_edge(&merge_commit_id.hash, commit_a).await?;
    handle.save_commit_graph_edge(&merge_commit_id.hash, commit_b).await?;

    // Get delta for summary
    let delta = diff_memory_vectors(handle, commit_a, commit_b).await?;

    Ok(MergeResult {
        merge_commit_id,
        auto_resolved: delta.conflicts.len(),
        manual_conflicts: vec![], // All resolved automatically for now
        summary: format!(
            "Merged {} memories from A, {} from B, resolved {} conflicts",
            delta.only_in_a.len(),
            delta.only_in_b.len(),
            delta.conflicts.len()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_diff_shows_only_new_vectors() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        // Create memories for commit A
        let mem_a1 = MemoryRecord::new("commit-a", "shared-key", "shared content");
        let mem_a2 = MemoryRecord::new("commit-a", "only-a-key", "only in A");
        handle.save_memory(&mem_a1).await.unwrap();
        handle.save_memory(&mem_a2).await.unwrap();

        // Create memories for commit B
        let mem_b1 = MemoryRecord::new("commit-b", "shared-key", "shared content");
        let mem_b2 = MemoryRecord::new("commit-b", "only-b-key", "only in B");
        handle.save_memory(&mem_b1).await.unwrap();
        handle.save_memory(&mem_b2).await.unwrap();

        let delta = diff_memory_vectors(&handle, "commit-a", "commit-b").await.unwrap();

        assert_eq!(delta.only_in_a.len(), 1);
        assert_eq!(delta.only_in_a[0].key, "only-a-key");

        assert_eq!(delta.only_in_b.len(), 1);
        assert_eq!(delta.only_in_b[0].key, "only-b-key");

        assert_eq!(delta.conflicts.len(), 0); // Same content = no conflict
    }

    #[tokio::test]
    async fn test_memory_diff_detects_conflicts() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let mem_a = MemoryRecord::new("commit-a", "conflict-key", "content version A");
        let mem_b = MemoryRecord::new("commit-b", "conflict-key", "content version B");
        handle.save_memory(&mem_a).await.unwrap();
        handle.save_memory(&mem_b).await.unwrap();

        let delta = diff_memory_vectors(&handle, "commit-a", "commit-b").await.unwrap();

        assert_eq!(delta.conflicts.len(), 1);
        assert_eq!(delta.conflicts[0].key, "conflict-key");
    }

    #[tokio::test]
    async fn test_arbiter_resolves_value_conflict_based_on_cot() {
        let conflict = MemoryConflict {
            key: "test-key".to_string(),
            memory_a: MemoryRecord::new("a", "test-key", "short"),
            memory_b: MemoryRecord::new("b", "test-key", "longer content here"),
        };

        let resolved = resolve_conflict_state(&[], &[], &conflict).await.unwrap();

        assert!(resolved.confidence > 0.0);
        assert!(resolved.favored_branch.is_some());
        assert!(!resolved.reasoning.is_empty());
    }
}
