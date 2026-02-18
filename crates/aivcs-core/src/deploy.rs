//! Deploy-by-digest runner.
//!
//! Looks up the current release for an agent, creates a deterministic
//! run record in the ledger, and returns a [`ReplaySummary`] whose digest
//! can be compared across identical invocations (golden equality).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use oxidized_state::storage_traits::{
    ReleaseRegistry, RunEvent, RunId, RunLedger, RunMetadata, RunSummary,
};

use crate::domain::{AivcsError, Result};
use crate::replay::{replay_run, ReplaySummary};

/// Result of a deploy-by-digest invocation.
#[derive(Debug, Clone)]
pub struct DeployResult {
    /// The run ID created for this deployment.
    pub run_id: RunId,
    /// The spec digest from the current release.
    pub spec_digest: String,
    /// Replay summary with golden digest.
    pub summary: ReplaySummary,
}

/// Deploy an agent by looking up its current release digest.
///
/// Creates a deterministic run with two events (`graph_started` and
/// `graph_completed`), then replays the run to produce a [`ReplaySummary`]
/// suitable for golden equality testing.
///
/// Pass a fixed `timestamp` to get deterministic digests across invocations.
pub async fn deploy_by_digest(
    registry: &dyn ReleaseRegistry,
    ledger: Arc<dyn RunLedger>,
    agent_name: &str,
    inputs: serde_json::Value,
    timestamp: Option<DateTime<Utc>>,
) -> Result<DeployResult> {
    // 1. Look up the current release
    let release = registry
        .current(agent_name)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?
        .ok_or_else(|| {
            AivcsError::ReleaseConflict(format!("no current release for agent '{}'", agent_name))
        })?;

    // 2. Extract spec_digest
    let spec_digest_str = release.spec_digest.as_str().to_string();

    // 3. Create a run in the ledger
    let metadata = RunMetadata {
        git_sha: None,
        agent_name: agent_name.to_string(),
        tags: serde_json::json!({}),
    };
    let run_id = ledger
        .create_run(&release.spec_digest, metadata)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?;

    let ts = timestamp.unwrap_or_else(Utc::now);

    // 4. Emit deterministic events
    let event_started = RunEvent {
        seq: 1,
        kind: "graph_started".to_string(),
        payload: serde_json::json!({ "inputs": inputs }),
        timestamp: ts,
    };
    ledger
        .append_event(&run_id, event_started)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?;

    let event_completed = RunEvent {
        seq: 2,
        kind: "graph_completed".to_string(),
        payload: serde_json::json!({}),
        timestamp: ts,
    };
    ledger
        .append_event(&run_id, event_completed)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?;

    // 5. Complete the run
    let summary = RunSummary {
        total_events: 2,
        final_state_digest: None,
        duration_ms: 0,
        success: true,
    };
    ledger
        .complete_run(&run_id, summary)
        .await
        .map_err(|e| AivcsError::StorageError(e.to_string()))?;

    // 6. Replay to get the golden digest
    let (_events, replay_summary) = replay_run(&*ledger, &run_id.0).await?;

    // 7. Return DeployResult
    Ok(DeployResult {
        run_id,
        spec_digest: spec_digest_str,
        summary: replay_summary,
    })
}
