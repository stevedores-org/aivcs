//! End-to-end tests for provenance tracking, run-outcome feedback, and cross-run learning.

use aivcs_core::memory::context::{assemble_context, ContextBudget};
use aivcs_core::memory::index::{IndexQuery, MemoryEntryKind, MemoryIndex};
use aivcs_core::memory::learning::{
    boost_risky_decisions, query_decision_history, DecisionHistory,
};
use aivcs_core::memory::provenance::{
    finalize_run_outcome, ingest_rationale, ProvenanceRecord, ProvenanceStore,
};
use aivcs_core::memory::rationale::{DecisionRationale, RationaleEntry, RationaleOutcome};

// ---------------------------------------------------------------------------
// Provenance ingestion
// ---------------------------------------------------------------------------

#[test]
fn test_ingest_rationale_creates_entry_and_provenance() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    let re = RationaleEntry::new(
        DecisionRationale::new("use rebase", "cleaner history").with_confidence(0.85),
        "run-100",
        3,
    )
    .with_tag("merge:strategy");

    let id = ingest_rationale(&mut index, &mut prov, &re, "spec-abc").unwrap();

    assert_eq!(id, "rat-run-100-3");
    let entry = index.get(&id).unwrap();
    assert_eq!(entry.kind, MemoryEntryKind::Rationale);
    assert!(entry.summary.contains("use rebase"));
    assert!(entry.tags.contains(&"merge:strategy".to_string()));
    assert!((entry.relevance - 0.85).abs() < f64::EPSILON);

    let pr = prov.for_entry(&id).unwrap();
    assert_eq!(pr.source_run_id, "run-100");
    assert_eq!(pr.agent_spec_digest, "spec-abc");
}

#[test]
fn test_ingest_multiple_rationales_from_same_run() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    for seq in 1..=5 {
        let re = RationaleEntry::new(
            DecisionRationale::new(&format!("decision {seq}"), "reason"),
            "run-200",
            seq,
        );
        ingest_rationale(&mut index, &mut prov, &re, "spec-def").unwrap();
    }

    assert_eq!(index.len(), 5);
    assert_eq!(prov.for_run("run-200").len(), 5);
}

// ---------------------------------------------------------------------------
// Run-outcome feedback
// ---------------------------------------------------------------------------

#[test]
fn test_finalize_failure_boosts_relevance_and_tags() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    let re = RationaleEntry::new(
        DecisionRationale::new("fast-forward merge", "quick").with_confidence(0.5),
        "run-300",
        1,
    )
    .with_tag("merge:strategy");
    let id = ingest_rationale(&mut index, &mut prov, &re, "spec-ghi").unwrap();

    let updated = finalize_run_outcome(
        &mut index,
        &mut prov,
        "run-300",
        RationaleOutcome::Failure,
        0.25,
    );
    assert_eq!(updated, 1);

    let entry = index.get(&id).unwrap();
    assert!((entry.relevance - 0.75).abs() < f64::EPSILON);
    assert!(entry.tags.contains(&"outcome:failure".to_string()));
}

#[test]
fn test_finalize_success_adds_tag_without_boost() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    let re = RationaleEntry::new(
        DecisionRationale::new("ort merge", "safe").with_confidence(0.8),
        "run-400",
        1,
    );
    let id = ingest_rationale(&mut index, &mut prov, &re, "spec-jkl").unwrap();

    finalize_run_outcome(
        &mut index,
        &mut prov,
        "run-400",
        RationaleOutcome::Success,
        0.25,
    );

    let entry = index.get(&id).unwrap();
    assert!((entry.relevance - 0.8).abs() < f64::EPSILON);
    assert!(entry.tags.contains(&"outcome:success".to_string()));
}

#[test]
fn test_finalize_only_affects_target_run() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    for (run, seq) in &[("run-A", 1u64), ("run-B", 1)] {
        let re = RationaleEntry::new(
            DecisionRationale::new("d", "r").with_confidence(0.5),
            run,
            *seq,
        );
        ingest_rationale(&mut index, &mut prov, &re, "spec").unwrap();
    }

    finalize_run_outcome(
        &mut index,
        &mut prov,
        "run-A",
        RationaleOutcome::Failure,
        0.3,
    );

    let a = index.get("rat-run-A-1").unwrap();
    let b = index.get("rat-run-B-1").unwrap();
    assert!(a.tags.contains(&"outcome:failure".to_string()));
    assert!(!b.tags.contains(&"outcome:failure".to_string()));
}

// ---------------------------------------------------------------------------
// Cross-run learning
// ---------------------------------------------------------------------------

#[test]
fn test_query_decision_history_mixed_outcomes() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    let outcomes = [
        ("r1", RationaleOutcome::Success),
        ("r2", RationaleOutcome::Failure),
        ("r3", RationaleOutcome::Failure),
        ("r4", RationaleOutcome::Partial),
    ];

    for (i, (run_id, outcome)) in outcomes.iter().enumerate() {
        let re = RationaleEntry::new(DecisionRationale::new("merge", "reason"), run_id, i as u64)
            .with_tag("merge:strategy");
        let id = ingest_rationale(&mut index, &mut prov, &re, "spec").unwrap();
        prov.update_run_outcome(run_id, outcome.clone());
        // Also set outcome on provenance record for the entry
        if let Some(pr) = prov.for_entry(&id) {
            let _ = pr; // outcome already set via update_run_outcome
        }
    }

    let h = query_decision_history(&index, &prov, "merge:strategy");
    assert_eq!(h.total, 4);
    assert_eq!(h.successes, 1);
    assert_eq!(h.failures, 2);
    assert_eq!(h.partial, 1);
    assert!((h.failure_rate - 0.5).abs() < f64::EPSILON);
    assert!(h.is_risky(0.4));
}

#[test]
fn test_boost_risky_decisions_e2e() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    // Create 3 failed decisions in same category
    for i in 0..3 {
        let re = RationaleEntry::new(
            DecisionRationale::new("bad strategy", "seemed fine").with_confidence(0.4),
            &format!("run-{i}"),
            1,
        )
        .with_tag("deploy:strategy");
        ingest_rationale(&mut index, &mut prov, &re, "spec").unwrap();
        prov.update_run_outcome(&format!("run-{i}"), RationaleOutcome::Failure);
    }

    let boosted = boost_risky_decisions(&mut index, &prov, "deploy:strategy", 0.5, 0.3);
    assert_eq!(boosted, 3);

    // All entries should have boosted relevance
    for i in 0..3 {
        let entry = index.get(&format!("rat-run-{i}-1")).unwrap();
        assert!((entry.relevance - 0.7).abs() < f64::EPSILON);
    }
}

#[test]
fn test_boost_safe_category_does_nothing() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    let re = RationaleEntry::new(
        DecisionRationale::new("good strategy", "always works").with_confidence(0.9),
        "run-ok",
        1,
    )
    .with_tag("safe:cat");
    ingest_rationale(&mut index, &mut prov, &re, "spec").unwrap();
    prov.update_run_outcome("run-ok", RationaleOutcome::Success);

    let boosted = boost_risky_decisions(&mut index, &prov, "safe:cat", 0.5, 0.3);
    assert_eq!(boosted, 0);
}

// ---------------------------------------------------------------------------
// Full pipeline: ingest → finalize → learn → assemble context
// ---------------------------------------------------------------------------

#[test]
fn test_full_provenance_learning_context_pipeline() {
    let mut index = MemoryIndex::new();
    let mut prov = ProvenanceStore::new();

    // Run 1: successful merge decision
    let re1 = RationaleEntry::new(
        DecisionRationale::new("ort merge", "safe default")
            .with_alternative("rebase")
            .with_confidence(0.7),
        "run-1",
        1,
    )
    .with_tag("merge:strategy");
    ingest_rationale(&mut index, &mut prov, &re1, "spec-v1").unwrap();
    finalize_run_outcome(
        &mut index,
        &mut prov,
        "run-1",
        RationaleOutcome::Success,
        0.2,
    );

    // Run 2: failed rebase decision
    let re2 = RationaleEntry::new(
        DecisionRationale::new("rebase", "cleaner history")
            .with_alternative("ort merge")
            .with_confidence(0.6),
        "run-2",
        1,
    )
    .with_tag("merge:strategy");
    ingest_rationale(&mut index, &mut prov, &re2, "spec-v1").unwrap();
    finalize_run_outcome(
        &mut index,
        &mut prov,
        "run-2",
        RationaleOutcome::Failure,
        0.2,
    );

    // Run 3: another failed rebase
    let re3 = RationaleEntry::new(
        DecisionRationale::new("rebase", "try again").with_confidence(0.5),
        "run-3",
        1,
    )
    .with_tag("merge:strategy");
    ingest_rationale(&mut index, &mut prov, &re3, "spec-v2").unwrap();
    finalize_run_outcome(
        &mut index,
        &mut prov,
        "run-3",
        RationaleOutcome::Failure,
        0.2,
    );

    // Query history
    let history = query_decision_history(&index, &prov, "merge:strategy");
    assert_eq!(history.total, 3);
    assert_eq!(history.failures, 2);
    assert!(history.is_risky(0.5));

    // Boost risky decisions
    boost_risky_decisions(&mut index, &prov, "merge:strategy", 0.5, 0.15);

    // Assemble context — failed decisions should rank higher now
    let results = index.query(&IndexQuery::all().with_tag("merge:strategy"));
    let budget = ContextBudget::new(1000, 100).unwrap();
    let window = assemble_context(&results.entries, &budget);

    assert_eq!(window.items.len(), 3);
    // The failed+boosted entries should appear before the successful one
    // run-2 started at 0.6, +0.2 (failure finalize) + 0.15 (boost) = 0.95
    // run-3 started at 0.5, +0.2 (failure finalize) + 0.15 (boost) = 0.85
    // run-1 started at 0.7, +0.15 (boost, no failure boost) = 0.85
    // So run-2's failure should be first
    assert!(window.items[0].text.contains("rebase"));
}

// ---------------------------------------------------------------------------
// Serde roundtrips
// ---------------------------------------------------------------------------

#[test]
fn test_provenance_store_serde_roundtrip() {
    let mut store = ProvenanceStore::new();
    store.record(
        ProvenanceRecord::new("e1", "run-1", 1, "spec-a").with_outcome(RationaleOutcome::Success),
    );
    store.record(ProvenanceRecord::new("e2", "run-2", 3, "spec-b"));

    let json = serde_json::to_string_pretty(&store).unwrap();
    let back: ProvenanceStore = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(
        back.for_entry("e1").unwrap().outcome,
        Some(RationaleOutcome::Success)
    );
    assert!(back.for_entry("e2").unwrap().outcome.is_none());
}

#[test]
fn test_decision_history_serde_roundtrip() {
    let h = DecisionHistory {
        category: "test:cat".into(),
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
