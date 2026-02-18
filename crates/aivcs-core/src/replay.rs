//! Replay functionality for recorded runs.
//!
//! This module provides the `replay_run` function to fetch and replay
//! all events for a given run from the `RunLedger`, computing a deterministic
//! digest over the event sequence for golden equality testing.

use oxidized_state::storage_traits::{
    ContentDigest, RunEvent, RunId, RunLedger, RunStatus as StorageRunStatus,
};
use tracing::info;

use crate::domain::{AivcsError, Result};

/// Summary produced after replaying a run's events.
#[derive(Debug, Clone)]
pub struct ReplaySummary {
    /// The run ID that was replayed.
    pub run_id: String,
    /// The agent name that produced the run.
    pub agent_name: String,
    /// The final status of the run (Running, Completed, Failed).
    pub status: StorageRunStatus,
    /// Number of events in the run.
    pub event_count: usize,
    /// SHA-256 hex digest of `serde_json::to_vec(&events)`.
    ///
    /// Used for golden equality checks: two runs with identical events
    /// will produce identical digests.
    pub replay_digest: String,
}

/// Fetch and replay all events for `run_id_str` from the ledger.
///
/// Events are returned in `seq` ascending order. The `ReplaySummary`
/// includes a deterministic SHA-256 digest computed over the serialized
/// event sequence.
///
/// # Errors
///
/// Returns `AivcsError::StorageError` when the run does not exist.
/// This is the "missing artifact rejection" test gate.
///
/// # Example
///
/// ```no_run
/// # use aivcs_core::replay_run;
/// # use oxidized_state::fakes::MemoryRunLedger;
/// # use oxidized_state::RunLedger;
/// # use std::sync::Arc;
/// # #[tokio::main]
/// # async fn main() {
/// let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
/// // Would fail with StorageError for non-existent run
/// let _result = replay_run(&*ledger, "run-12345").await;
/// # }
/// ```
pub async fn replay_run(
    ledger: &dyn RunLedger,
    run_id_str: &str,
) -> Result<(Vec<RunEvent>, ReplaySummary)> {
    let _span = crate::obs::RunSpan::enter(run_id_str);

    let run_id = RunId(run_id_str.to_string());

    // Fetch run record — returns StorageError::RunNotFound if absent
    let record = ledger
        .get_run(&run_id)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?;

    // Fetch events in seq order (both MemoryRunLedger and SurrealRunLedger sort by seq)
    let events = ledger
        .get_events(&run_id)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?;

    // Compute deterministic digest: SHA-256 over serde_json::to_vec(&events)
    let events_json = serde_json::to_vec(&events).map_err(AivcsError::Serialization)?;
    let replay_digest = ContentDigest::from_bytes(&events_json).as_str().to_string();

    let summary = ReplaySummary {
        run_id: record.run_id.to_string(),
        agent_name: record.metadata.agent_name.clone(),
        status: record.status,
        event_count: events.len(),
        replay_digest: replay_digest.clone(),
    };

    info!(
        event = "replay.completed",
        run_id = %run_id_str,
        event_count = summary.event_count,
        digest = %replay_digest
    );

    Ok((events, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidized_state::fakes::MemoryRunLedger;
    use oxidized_state::storage_traits::{ContentDigest, RunMetadata};
    use std::sync::Arc;

    /// Helper to build a run with the specified number of node pairs.
    async fn build_run(
        ledger: &dyn RunLedger,
        n_nodes: u32,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<RunId> {
        let spec_digest = ContentDigest::from_bytes(b"test_spec");
        let metadata = RunMetadata {
            git_sha: Some("test_sha".to_string()),
            agent_name: "test_agent".to_string(),
            tags: serde_json::json!({}),
        };

        let run_id = ledger
            .create_run(&spec_digest, metadata)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))?;

        // Emit events
        let mut seq = 1u64;

        // GraphStarted
        let event = RunEvent {
            seq,
            kind: "graph_started".to_string(),
            payload: serde_json::json!({}),
            timestamp,
        };
        ledger
            .append_event(&run_id, event)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))?;
        seq += 1;

        // N node pairs
        for i in 0..n_nodes {
            // NodeEntered
            let event = RunEvent {
                seq,
                kind: "node_entered".to_string(),
                payload: serde_json::json!({"node_id": format!("node_{}", i)}),
                timestamp,
            };
            ledger
                .append_event(&run_id, event)
                .await
                .map_err(|e| AivcsError::StorageError(e.to_string()))?;
            seq += 1;

            // NodeExited
            let event = RunEvent {
                seq,
                kind: "node_exited".to_string(),
                payload: serde_json::json!({"node_id": format!("node_{}", i)}),
                timestamp,
            };
            ledger
                .append_event(&run_id, event)
                .await
                .map_err(|e| AivcsError::StorageError(e.to_string()))?;
            seq += 1;
        }

        // GraphCompleted
        let event = RunEvent {
            seq,
            kind: "graph_completed".to_string(),
            payload: serde_json::json!({}),
            timestamp,
        };
        ledger
            .append_event(&run_id, event)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))?;

        // Complete the run
        let summary = oxidized_state::storage_traits::RunSummary {
            total_events: seq,
            final_state_digest: None,
            duration_ms: 1000,
            success: true,
        };
        ledger
            .complete_run(&run_id, summary)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))?;

        Ok(run_id)
    }

    #[tokio::test]
    async fn test_replay_golden_digest_equality() {
        let ledger_a: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
        let ledger_b: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

        // Use fixed timestamp for both runs to ensure identical events
        let fixed_timestamp = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .expect("parse timestamp")
            .with_timezone(&chrono::Utc);

        // Build identical runs with same timestamp
        let run_a = build_run(&*ledger_a, 2, fixed_timestamp)
            .await
            .expect("build_run_a");
        let run_b = build_run(&*ledger_b, 2, fixed_timestamp)
            .await
            .expect("build_run_b");

        // Replay both
        let (_events_a, summary_a) = replay_run(&*ledger_a, &run_a.0).await.expect("replay_a");
        let (_events_b, summary_b) = replay_run(&*ledger_b, &run_b.0).await.expect("replay_b");

        // Golden digests must be equal (same events including timestamps)
        assert_eq!(summary_a.replay_digest, summary_b.replay_digest);
        assert_eq!(summary_a.event_count, summary_b.event_count);
    }

    #[tokio::test]
    async fn test_replay_missing_run_rejection() {
        let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

        let result = replay_run(&*ledger, "nonexistent-run-id").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AivcsError::StorageError(_) => { /* expected */ }
            other => panic!("Expected StorageError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_replay_event_order() {
        let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

        let spec_digest = ContentDigest::from_bytes(b"test_spec");
        let metadata = RunMetadata {
            git_sha: Some("test_sha".to_string()),
            agent_name: "test_agent".to_string(),
            tags: serde_json::json!({}),
        };

        let run_id = ledger
            .create_run(&spec_digest, metadata)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("create_run");

        // Append events in reverse seq order (out of order)
        let event3 = RunEvent {
            seq: 3,
            kind: "test_3".to_string(),
            payload: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
        };
        ledger
            .append_event(&run_id, event3)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("append");

        let event1 = RunEvent {
            seq: 1,
            kind: "test_1".to_string(),
            payload: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
        };
        ledger
            .append_event(&run_id, event1)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("append");

        let event2 = RunEvent {
            seq: 2,
            kind: "test_2".to_string(),
            payload: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
        };
        ledger
            .append_event(&run_id, event2)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("append");

        let summary = oxidized_state::storage_traits::RunSummary {
            total_events: 3,
            final_state_digest: None,
            duration_ms: 100,
            success: true,
        };
        ledger
            .complete_run(&run_id, summary)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("complete");

        // Replay should return events in seq order: 1, 2, 3
        let (events, _summary) = replay_run(&*ledger, &run_id.0).await.expect("replay");

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 3);
    }

    #[tokio::test]
    async fn test_replay_empty_run() {
        let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

        let spec_digest = ContentDigest::from_bytes(b"test_spec");
        let metadata = RunMetadata {
            git_sha: Some("test_sha".to_string()),
            agent_name: "test_agent".to_string(),
            tags: serde_json::json!({}),
        };

        let run_id = ledger
            .create_run(&spec_digest, metadata)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("create_run");

        // Complete without appending any events
        let summary = oxidized_state::storage_traits::RunSummary {
            total_events: 0,
            final_state_digest: None,
            duration_ms: 0,
            success: true,
        };
        ledger
            .complete_run(&run_id, summary)
            .await
            .map_err(|e| AivcsError::StorageError(e.to_string()))
            .expect("complete");

        let (events, summary) = replay_run(&*ledger, &run_id.0).await.expect("replay");

        assert_eq!(events.len(), 0);
        assert_eq!(summary.event_count, 0);
        // replay_digest should be valid hex (64 chars for SHA256)
        assert_eq!(summary.replay_digest.len(), 64);
        assert!(summary.replay_digest.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
