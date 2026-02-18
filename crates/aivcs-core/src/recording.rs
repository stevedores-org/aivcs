//! Graph lifecycle adapter: bridges domain `Event` types to `RunLedger` persistence.

use std::sync::Arc;

use oxidized_state::{
    ContentDigest, RunEvent, RunId, RunLedger, RunMetadata, RunSummary, StorageResult,
};

use crate::domain::run::{Event, EventKind};

/// Extract the snake_case kind string from an `EventKind` via its serde tag.
fn event_kind_str(kind: &EventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v["type"].as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Adapter that records graph lifecycle [`Event`]s into a [`RunLedger`].
///
/// Usage:
/// 1. Call [`GraphRunRecorder::start`] to create a new run.
/// 2. Call [`GraphRunRecorder::record`] for each domain event.
/// 3. Call [`GraphRunRecorder::finish_ok`] or [`GraphRunRecorder::finish_err`] to finalize.
pub struct GraphRunRecorder {
    ledger: Arc<dyn RunLedger>,
    run_id: RunId,
}

impl GraphRunRecorder {
    /// Start a new run in the ledger, returning a recorder bound to that run.
    pub async fn start(
        ledger: Arc<dyn RunLedger>,
        spec_digest: &ContentDigest,
        metadata: RunMetadata,
    ) -> StorageResult<Self> {
        let run_id = ledger.create_run(spec_digest, metadata.clone()).await?;
        crate::obs::emit_run_started(run_id.to_string().as_str(), &metadata.agent_name);
        Ok(Self { ledger, run_id })
    }

    /// Record a single domain event into the ledger.
    pub async fn record(&self, event: &Event) -> StorageResult<()> {
        let kind_str = event_kind_str(&event.kind);
        let run_event = RunEvent {
            seq: event.seq,
            kind: kind_str.clone(),
            payload: event.payload.clone(),
            timestamp: event.timestamp,
        };
        crate::obs::emit_event_appended(&self.run_id.to_string(), &kind_str, event.seq);
        self.ledger.append_event(&self.run_id, run_event).await
    }

    /// Finalize the run as completed.
    pub async fn finish_ok(self, summary: RunSummary) -> StorageResult<()> {
        crate::obs::emit_run_finished(
            &self.run_id.to_string(),
            summary.duration_ms,
            summary.total_events,
            true,
        );
        self.ledger.complete_run(&self.run_id, summary).await
    }

    /// Finalize the run as failed.
    pub async fn finish_err(self, summary: RunSummary) -> StorageResult<()> {
        crate::obs::emit_run_finished(
            &self.run_id.to_string(),
            summary.duration_ms,
            summary.total_events,
            false,
        );
        self.ledger.fail_run(&self.run_id, summary).await
    }

    /// Return a reference to the run ID.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
}
