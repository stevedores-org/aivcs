//! Cross-run learning from prior decisions and their outcomes.
//!
//! Queries the memory index for similar past decisions, computes
//! failure rates, and provides relevance adjustments so agents
//! can learn from history.

use super::index::{IndexQuery, MemoryEntryKind, MemoryIndex};
use super::provenance::ProvenanceStore;
use super::rationale::RationaleOutcome;
use serde::{Deserialize, Serialize};

/// Summary of historical decision outcomes for a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionHistory {
    /// The tag/category queried.
    pub category: String,
    /// Total decisions found.
    pub total: usize,
    /// Count by outcome.
    pub successes: usize,
    pub failures: usize,
    pub partial: usize,
    pub skipped: usize,
    /// Entries with no outcome recorded yet.
    pub pending: usize,
    /// Failure rate (0.0–1.0), NaN-safe.
    pub failure_rate: f64,
}

impl DecisionHistory {
    /// Whether the failure rate exceeds a threshold.
    pub fn is_risky(&self, threshold: f64) -> bool {
        self.failure_rate.is_finite() && self.failure_rate >= threshold
    }
}

/// Query the memory index for prior decisions in a category and compute
/// outcome statistics using provenance data.
pub fn query_decision_history(
    index: &MemoryIndex,
    provenance: &ProvenanceStore,
    category_tag: &str,
) -> DecisionHistory {
    let results = index.query(
        &IndexQuery::all()
            .with_kind(MemoryEntryKind::Rationale)
            .with_tag(category_tag),
    );

    let mut successes = 0usize;
    let mut failures = 0usize;
    let mut partial = 0usize;
    let mut skipped = 0usize;
    let mut pending = 0usize;

    for entry in &results.entries {
        match provenance.for_entry(&entry.id) {
            Some(prov) => match &prov.outcome {
                Some(RationaleOutcome::Success) => successes += 1,
                Some(RationaleOutcome::Failure) => failures += 1,
                Some(RationaleOutcome::Partial) => partial += 1,
                Some(RationaleOutcome::Skipped) => skipped += 1,
                None => pending += 1,
            },
            None => pending += 1,
        }
    }

    let resolved = successes + failures + partial;
    let failure_rate = if resolved > 0 {
        failures as f64 / resolved as f64
    } else {
        0.0
    };

    DecisionHistory {
        category: category_tag.to_string(),
        total: results.total_matches,
        successes,
        failures,
        partial,
        skipped,
        pending,
        failure_rate,
    }
}

/// Boost relevance of entries tagged with high-failure categories.
///
/// Scans the index for rationale entries matching `category_tag`. If the
/// failure rate exceeds `risk_threshold`, boosts their relevance by `boost`
/// so they surface in future context assembly.
///
/// Returns the number of entries boosted.
pub fn boost_risky_decisions(
    index: &mut MemoryIndex,
    provenance: &ProvenanceStore,
    category_tag: &str,
    risk_threshold: f64,
    boost: f64,
) -> usize {
    let history = query_decision_history(index, provenance, category_tag);

    if !history.is_risky(risk_threshold) {
        return 0;
    }

    let entry_ids: Vec<String> = index
        .query(
            &IndexQuery::all()
                .with_kind(MemoryEntryKind::Rationale)
                .with_tag(category_tag),
        )
        .entries
        .iter()
        .map(|e| e.id.clone())
        .collect();

    let mut boosted = 0;
    for id in entry_ids {
        if let Some(entry) = index.entries_mut().get_mut(&id) {
            entry.relevance = (entry.relevance + boost).clamp(0.0, 1.0);
            boosted += 1;
        }
    }

    boosted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::index::{MemoryEntry, MemoryEntryKind, MemoryIndex};
    use crate::memory::provenance::{ProvenanceRecord, ProvenanceStore};
    use crate::memory::rationale::RationaleOutcome;
    use chrono::Utc;

    fn rationale_entry(id: &str, tag: &str, relevance: f64) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            kind: MemoryEntryKind::Rationale,
            summary: format!("decision {id}"),
            content_digest: format!("d_{id}"),
            created_at: Utc::now(),
            tags: vec![tag.into()],
            token_estimate: 50,
            relevance,
        }
    }

    #[test]
    fn test_empty_history() {
        let index = MemoryIndex::new();
        let prov = ProvenanceStore::new();
        let h = query_decision_history(&index, &prov, "merge:strategy");
        assert_eq!(h.total, 0);
        assert!((h.failure_rate - 0.0).abs() < f64::EPSILON);
        assert!(!h.is_risky(0.5));
    }

    #[test]
    fn test_history_with_mixed_outcomes() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        for (id, outcome) in &[
            ("r1", RationaleOutcome::Success),
            ("r2", RationaleOutcome::Failure),
            ("r3", RationaleOutcome::Failure),
            ("r4", RationaleOutcome::Partial),
        ] {
            index
                .insert(rationale_entry(id, "merge:strategy", 0.5))
                .unwrap();
            prov.record(
                ProvenanceRecord::new(id, &format!("run-{id}"), 1, "spec")
                    .with_outcome(outcome.clone()),
            );
        }

        let h = query_decision_history(&index, &prov, "merge:strategy");
        assert_eq!(h.total, 4);
        assert_eq!(h.successes, 1);
        assert_eq!(h.failures, 2);
        assert_eq!(h.partial, 1);
        // failure_rate = 2 / (1+2+1) = 0.5
        assert!((h.failure_rate - 0.5).abs() < f64::EPSILON);
        assert!(h.is_risky(0.5));
        assert!(!h.is_risky(0.6));
    }

    #[test]
    fn test_history_pending_entries() {
        let mut index = MemoryIndex::new();
        let prov = ProvenanceStore::new();

        index
            .insert(rationale_entry("r1", "test:cat", 0.5))
            .unwrap();

        let h = query_decision_history(&index, &prov, "test:cat");
        assert_eq!(h.total, 1);
        assert_eq!(h.pending, 1);
        assert!((h.failure_rate - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_boost_risky_decisions_below_threshold() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        index
            .insert(rationale_entry("r1", "safe:cat", 0.5))
            .unwrap();
        prov.record(
            ProvenanceRecord::new("r1", "run-1", 1, "spec").with_outcome(RationaleOutcome::Success),
        );

        let boosted = boost_risky_decisions(&mut index, &prov, "safe:cat", 0.5, 0.2);
        assert_eq!(boosted, 0);
        assert!((index.get("r1").unwrap().relevance - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_boost_risky_decisions_above_threshold() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        for id in &["r1", "r2", "r3"] {
            index.insert(rationale_entry(id, "risky:cat", 0.4)).unwrap();
            prov.record(
                ProvenanceRecord::new(id, &format!("run-{id}"), 1, "spec")
                    .with_outcome(RationaleOutcome::Failure),
            );
        }

        let boosted = boost_risky_decisions(&mut index, &prov, "risky:cat", 0.5, 0.3);
        assert_eq!(boosted, 3);
        assert!((index.get("r1").unwrap().relevance - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_boost_clamps_to_one() {
        let mut index = MemoryIndex::new();
        let mut prov = ProvenanceStore::new();

        index
            .insert(rationale_entry("r1", "risky:cat", 0.9))
            .unwrap();
        prov.record(
            ProvenanceRecord::new("r1", "run-1", 1, "spec").with_outcome(RationaleOutcome::Failure),
        );

        boost_risky_decisions(&mut index, &prov, "risky:cat", 0.5, 0.5);
        assert!((index.get("r1").unwrap().relevance - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_decision_history_serde_roundtrip() {
        let h = DecisionHistory {
            category: "merge:strategy".into(),
            total: 10,
            successes: 5,
            failures: 3,
            partial: 1,
            skipped: 0,
            pending: 1,
            failure_rate: 3.0 / 9.0,
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: DecisionHistory = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
    }
}
