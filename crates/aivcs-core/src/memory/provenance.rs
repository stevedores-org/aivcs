//! Provenance tracking for memory entries.
//!
//! Links memory entries to their source runs and updates outcomes
//! when runs complete, enabling feedback loops between execution
//! results and stored memory.

use super::error::MemoryResult;
use super::index::{MemoryEntry, MemoryEntryKind, MemoryIndex};
use super::rationale::{RationaleEntry, RationaleOutcome};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Provenance metadata linking a memory entry to its source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    /// The memory entry id this record describes.
    pub entry_id: String,
    /// The run that produced this entry.
    pub source_run_id: String,
    /// The event sequence number within the run.
    pub source_event_seq: u64,
    /// The agent spec digest of the producing run.
    pub agent_spec_digest: String,
    /// When the entry was recorded.
    pub recorded_at: DateTime<Utc>,
    /// Outcome if known (updated after run completes).
    pub outcome: Option<RationaleOutcome>,
}

impl ProvenanceRecord {
    pub fn new(
        entry_id: &str,
        source_run_id: &str,
        source_event_seq: u64,
        agent_spec_digest: &str,
    ) -> Self {
        Self {
            entry_id: entry_id.into(),
            source_run_id: source_run_id.into(),
            source_event_seq,
            agent_spec_digest: agent_spec_digest.into(),
            recorded_at: Utc::now(),
            outcome: None,
        }
    }

    pub fn with_outcome(mut self, outcome: RationaleOutcome) -> Self {
        self.outcome = Some(outcome);
        self
    }
}

/// Tracks provenance for all memory entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvenanceStore {
    records: Vec<ProvenanceRecord>,
}

impl ProvenanceStore {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Record provenance for a memory entry.
    pub fn record(&mut self, record: ProvenanceRecord) {
        self.records.push(record);
    }

    /// Find all provenance records for a given run.
    pub fn for_run(&self, run_id: &str) -> Vec<&ProvenanceRecord> {
        self.records
            .iter()
            .filter(|r| r.source_run_id == run_id)
            .collect()
    }

    /// Find the provenance record for a specific entry.
    pub fn for_entry(&self, entry_id: &str) -> Option<&ProvenanceRecord> {
        self.records.iter().find(|r| r.entry_id == entry_id)
    }

    /// Update outcomes for all entries from a given run.
    /// Returns the number of records updated.
    pub fn update_run_outcome(&mut self, run_id: &str, outcome: RationaleOutcome) -> usize {
        let mut count = 0;
        for record in &mut self.records {
            if record.source_run_id == run_id {
                record.outcome = Some(outcome.clone());
                count += 1;
            }
        }
        count
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Ingest a `RationaleEntry` into both the memory index and provenance store.
///
/// Creates a `MemoryEntry` from the rationale and records its provenance.
pub fn ingest_rationale(
    index: &mut MemoryIndex,
    provenance: &mut ProvenanceStore,
    entry: &RationaleEntry,
    agent_spec_digest: &str,
) -> MemoryResult<String> {
    let id = format!("rat-{}-{}", entry.run_id, entry.event_seq);

    let mem_entry = MemoryEntry {
        id: id.clone(),
        kind: MemoryEntryKind::Rationale,
        summary: format!(
            "{}: {}",
            entry.rationale.decision, entry.rationale.reasoning
        ),
        content_digest: format!("rationale_{}", id),
        created_at: entry.decided_at,
        tags: entry.tags.clone(),
        token_estimate: entry.token_estimate(),
        relevance: entry.rationale.confidence,
    };

    index.insert(mem_entry)?;

    let prov = ProvenanceRecord::new(&id, &entry.run_id, entry.event_seq, agent_spec_digest);
    let prov = if let Some(ref outcome) = entry.outcome {
        prov.with_outcome(outcome.clone())
    } else {
        prov
    };
    provenance.record(prov);

    Ok(id)
}

/// Mark all memory entries from a run with an outcome.
///
/// Updates both provenance records and adjusts relevance scores
/// in the memory index: failed runs get a relevance boost (so agents
/// remember what went wrong), successful runs retain their original score.
pub fn finalize_run_outcome(
    index: &mut MemoryIndex,
    provenance: &mut ProvenanceStore,
    run_id: &str,
    outcome: RationaleOutcome,
    failure_relevance_boost: f64,
) -> usize {
    let updated = provenance.update_run_outcome(run_id, outcome.clone());

    // Boost relevance for failed entries so they surface in future context
    if outcome == RationaleOutcome::Failure {
        let entry_ids: Vec<String> = provenance
            .for_run(run_id)
            .iter()
            .map(|r| r.entry_id.clone())
            .collect();

        for id in entry_ids {
            if let Some(entry) = index.entries_mut().get_mut(&id) {
                entry.relevance = (entry.relevance + failure_relevance_boost).clamp(0.0, 1.0);
                if !entry.tags.contains(&"outcome:failure".to_string()) {
                    entry.tags.push("outcome:failure".into());
                }
            }
        }
    } else if outcome == RationaleOutcome::Success {
        let entry_ids: Vec<String> = provenance
            .for_run(run_id)
            .iter()
            .map(|r| r.entry_id.clone())
            .collect();

        for id in entry_ids {
            if let Some(entry) = index.entries_mut().get_mut(&id) {
                if !entry.tags.contains(&"outcome:success".to_string()) {
                    entry.tags.push("outcome:success".into());
                }
            }
        }
    }

    updated
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::memory::rationale::DecisionRationale;

    fn make_rationale(run_id: &str, seq: u64) -> RationaleEntry {
        RationaleEntry::new(
            DecisionRationale::new("test decision", "test reasoning").with_confidence(0.7),
            run_id,
            seq,
        )
        .with_tag("agent:coder")
    }

    #[test]
    fn test_provenance_record_builder() {
        let r = ProvenanceRecord::new("entry-1", "run-1", 5, "spec-abc")
            .with_outcome(RationaleOutcome::Success);
        assert_eq!(r.entry_id, "entry-1");
        assert_eq!(r.source_run_id, "run-1");
        assert_eq!(r.outcome, Some(RationaleOutcome::Success));
    }

    #[test]
    fn test_provenance_store_crud() {
        let mut store = ProvenanceStore::new();
        assert!(store.is_empty());

        store.record(ProvenanceRecord::new("e1", "run-1", 1, "spec-a"));
        store.record(ProvenanceRecord::new("e2", "run-1", 2, "spec-a"));
        store.record(ProvenanceRecord::new("e3", "run-2", 1, "spec-b"));

        assert_eq!(store.len(), 3);
        assert_eq!(store.for_run("run-1").len(), 2);
        assert_eq!(store.for_run("run-2").len(), 1);
        assert!(store.for_entry("e1").is_some());
        assert!(store.for_entry("missing").is_none());
    }

    #[test]
    fn test_update_run_outcome() {
        let mut store = ProvenanceStore::new();
        store.record(ProvenanceRecord::new("e1", "run-1", 1, "spec-a"));
        store.record(ProvenanceRecord::new("e2", "run-1", 2, "spec-a"));
        store.record(ProvenanceRecord::new("e3", "run-2", 1, "spec-b"));

        let count = store.update_run_outcome("run-1", RationaleOutcome::Failure);
        assert_eq!(count, 2);
        assert_eq!(
            store.for_entry("e1").unwrap().outcome,
            Some(RationaleOutcome::Failure)
        );
        assert!(store.for_entry("e3").unwrap().outcome.is_none());
    }

    #[test]
    fn test_ingest_rationale() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        let re = make_rationale("run-42", 7);
        let id = ingest_rationale(&mut index, &mut prov, &re, "spec-xyz").unwrap();

        assert_eq!(id, "rat-run-42-7");
        assert_eq!(index.len(), 1);
        let entry = index.get(&id).unwrap();
        assert_eq!(entry.kind, MemoryEntryKind::Rationale);
        assert!(entry.summary.contains("test decision"));
        assert!(prov.for_entry(&id).is_some());
    }

    #[test]
    fn test_ingest_duplicate_rejected() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        let re = make_rationale("run-1", 1);
        ingest_rationale(&mut index, &mut prov, &re, "spec-a").unwrap();
        let err = ingest_rationale(&mut index, &mut prov, &re, "spec-a");
        assert!(err.is_err());
    }

    #[test]
    fn test_finalize_run_failure_boosts_relevance() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        let re = make_rationale("run-1", 1);
        let id = ingest_rationale(&mut index, &mut prov, &re, "spec-a").unwrap();

        let original_relevance = index.get(&id).unwrap().relevance;
        finalize_run_outcome(
            &mut index,
            &mut prov,
            "run-1",
            RationaleOutcome::Failure,
            0.2,
        );

        let boosted = index.get(&id).unwrap();
        assert!((boosted.relevance - (original_relevance + 0.2)).abs() < f64::EPSILON);
        assert!(boosted.tags.contains(&"outcome:failure".to_string()));
    }

    #[test]
    fn test_finalize_run_success_tags_only() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        let re = make_rationale("run-1", 1);
        let id = ingest_rationale(&mut index, &mut prov, &re, "spec-a").unwrap();

        let original_relevance = index.get(&id).unwrap().relevance;
        finalize_run_outcome(
            &mut index,
            &mut prov,
            "run-1",
            RationaleOutcome::Success,
            0.2,
        );

        let entry = index.get(&id).unwrap();
        assert!((entry.relevance - original_relevance).abs() < f64::EPSILON);
        assert!(entry.tags.contains(&"outcome:success".to_string()));
    }

    #[test]
    fn test_provenance_serde_roundtrip() {
        let mut store = ProvenanceStore::new();
        store.record(
            ProvenanceRecord::new("e1", "run-1", 1, "spec-a")
                .with_outcome(RationaleOutcome::Success),
        );
        let json = serde_json::to_string(&store).unwrap();
        let back: ProvenanceStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(
            back.for_entry("e1").unwrap().outcome,
            Some(RationaleOutcome::Success)
        );
    }
}
