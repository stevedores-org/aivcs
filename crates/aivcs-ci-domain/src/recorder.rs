//! EventBus recorder for CI - persists events to RunLedger + CAS
//!
//! Story 4.2: Recorder adapter integrates with existing AIVCS EventBus adapters.
//! The recorder takes an event stream and:
//! 1. Records events in an append-only log
//! 2. Updates RunLedger (indexed query view)
//! 3. References large payloads in CAS (digests stored in events)

use crate::events::*;
use crate::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::debug;

/// Configuration for the event recorder
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// SurrealDB namespace
    pub namespace: String,
    /// SurrealDB database name
    pub database: String,
    /// Events table name
    pub events_table: String,
    /// Ledger table name
    pub ledger_table: String,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        RecorderConfig {
            namespace: "aivcs".to_string(),
            database: "ci".to_string(),
            events_table: "ci_events".to_string(),
            ledger_table: "ci_ledger".to_string(),
        }
    }
}

/// In-memory event storage (Phase 1: local testing)
/// Phase 2 will integrate with actual SurrealDB following oxidized-state pattern
type EventStore = Arc<Mutex<HashMap<String, Vec<EventRecord>>>>;
type LedgerStore = Arc<Mutex<HashMap<String, RunLedgerEntry>>>;

/// Records CI events to in-memory store (and eventually SurrealDB)
///
/// This is the persistence adapter for the CI event stream.
/// All writes are append-only (immutable events).
pub struct EventRecorder {
    config: RecorderConfig,
    events: EventStore,
    ledger: LedgerStore,
}

impl EventRecorder {
    /// Create a new event recorder
    pub async fn new(config: RecorderConfig) -> Result<Self> {
        debug!(
            "Initializing event recorder: ns={}, db={}",
            config.namespace, config.database
        );

        let recorder = EventRecorder {
            config,
            events: Arc::new(Mutex::new(HashMap::new())),
            ledger: Arc::new(Mutex::new(HashMap::new())),
        };

        Ok(recorder)
    }

    /// Record an event (append-only)
    ///
    /// Returns the event sequence number
    pub async fn record_event(&self, event: CIEvent) -> Result<u64> {
        let run_id = self.extract_run_id(&event);
        let timestamp = chrono::Utc::now();

        let mut store = self.events.lock().unwrap();
        let run_events = store.entry(run_id.clone()).or_insert_with(Vec::new);
        let sequence = run_events.len() as u64;

        let record = EventRecord {
            id: None,
            event,
            sequence,
            run_id: run_id.clone(),
            recorded_at: timestamp,
        };

        run_events.push(record);

        debug!("Recorded event #{} for run {}", sequence, run_id);
        Ok(sequence)
    }

    /// Update ledger entry from a run (derived data)
    pub async fn update_ledger(&self, entry: &RunLedgerEntry) -> Result<()> {
        let mut ledger = self.ledger.lock().unwrap();
        ledger.insert(entry.run_id.clone(), entry.clone());
        debug!("Updated ledger for run {}", entry.run_id);
        Ok(())
    }

    /// Query runs by status
    pub async fn query_by_status(&self, status: &str) -> Result<Vec<RunLedgerEntry>> {
        let ledger = self.ledger.lock().unwrap();
        let results: Vec<RunLedgerEntry> = ledger
            .values()
            .filter(|entry| entry.status == status)
            .cloned()
            .collect();
        debug!("Query runs with status: {} (found: {})", status, results.len());
        Ok(results)
    }

    /// Get all events for a run (in order)
    pub async fn get_run_events(&self, run_id: &str) -> Result<Vec<EventRecord>> {
        let store = self.events.lock().unwrap();
        let events = store.get(run_id).cloned().unwrap_or_default();
        debug!("Get {} events for run: {}", events.len(), run_id);
        Ok(events)
    }

    /// Extract run_id from an event
    fn extract_run_id(&self, event: &CIEvent) -> String {
        match event {
            CIEvent::RunStarted(e) => e.run_id.clone(),
            CIEvent::StageStarted(e) => e.run_id.clone(),
            CIEvent::StageFinished(e) => e.run_id.clone(),
            CIEvent::RunFinished(e) => e.run_id.clone(),
            CIEvent::DiagnosticsProduced(e) => e.run_id.clone(),
            CIEvent::RepairPlanned(e) => e.run_id.clone(),
            CIEvent::PatchApplied(e) => e.run_id.clone(),
            CIEvent::VerificationFinished(e) => e.original_run_id.clone(),
            CIEvent::GateEvaluated(e) => e.run_id.clone(),
            CIEvent::PromotionApplied(e) => e.run_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recorder_config_default() {
        let config = RecorderConfig::default();
        assert_eq!(config.namespace, "aivcs");
        assert_eq!(config.database, "ci");
    }

    #[tokio::test]
    async fn test_extract_run_id() {
        let config = RecorderConfig::default();
        let recorder = EventRecorder::new(config).await.expect("recorder creation");

        let event = CIEvent::RunStarted(RunStartedEvent {
            event_id: EventId::new(),
            run_id: "test-run-123".to_string(),
            snapshot_id: "snap".to_string(),
            spec_digest: "spec".to_string(),
            policy_digest: "policy".to_string(),
            runner_version: "1.0".to_string(),
            timestamp: chrono::Utc::now(),
        });

        let run_id = recorder.extract_run_id(&event);
        assert_eq!(run_id, "test-run-123");
    }

    #[tokio::test]
    async fn test_record_event() {
        let config = RecorderConfig::default();
        let recorder = EventRecorder::new(config).await.expect("recorder creation");

        let event = CIEvent::RunStarted(RunStartedEvent {
            event_id: EventId::new(),
            run_id: "test-run-456".to_string(),
            snapshot_id: "snap".to_string(),
            spec_digest: "spec".to_string(),
            policy_digest: "policy".to_string(),
            runner_version: "1.0".to_string(),
            timestamp: chrono::Utc::now(),
        });

        let seq = recorder.record_event(event).await.expect("record event");
        assert_eq!(seq, 0);
    }

    #[tokio::test]
    async fn test_record_multiple_events() {
        let config = RecorderConfig::default();
        let recorder = EventRecorder::new(config).await.expect("recorder creation");
        let run_id = "test-run-789";

        let event1 = CIEvent::RunStarted(RunStartedEvent {
            event_id: EventId::new(),
            run_id: run_id.to_string(),
            snapshot_id: "snap".to_string(),
            spec_digest: "spec".to_string(),
            policy_digest: "policy".to_string(),
            runner_version: "1.0".to_string(),
            timestamp: chrono::Utc::now(),
        });

        let seq1 = recorder.record_event(event1).await.expect("record 1");
        assert_eq!(seq1, 0);

        let event2 = CIEvent::StageStarted(StageStartedEvent {
            event_id: EventId::new(),
            run_id: run_id.to_string(),
            stage_name: "fmt".to_string(),
            timestamp: chrono::Utc::now(),
        });

        let seq2 = recorder.record_event(event2).await.expect("record 2");
        assert_eq!(seq2, 1);

        let events = recorder
            .get_run_events(run_id)
            .await
            .expect("get events");
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_query_by_status() {
        let config = RecorderConfig::default();
        let recorder = EventRecorder::new(config).await.expect("recorder creation");

        let entry = RunLedgerEntry {
            id: None,
            run_id: "run-1".to_string(),
            snapshot_id: "snap-1".to_string(),
            status: "succeeded".to_string(),
            passed: true,
            duration_ms: 5000,
            diagnostic_count: 0,
            max_severity: None,
            repair_action_count: 0,
            stage_count: 3,
            stages_passed: 3,
            result_digest: None,
            diagnostics_digest: None,
            repair_plan_digest: None,
            verification_run_id: None,
            started_at: chrono::Utc::now(),
            finished_at: Some(chrono::Utc::now()),
            updated_at: chrono::Utc::now(),
        };

        recorder
            .update_ledger(&entry)
            .await
            .expect("update ledger");

        let results = recorder
            .query_by_status("succeeded")
            .await
            .expect("query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].run_id, "run-1");
    }
}
