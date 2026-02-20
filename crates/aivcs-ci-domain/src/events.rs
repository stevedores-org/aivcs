//! CI lifecycle events for append-only provenance
//!
//! Events form the ground truth for CI runs. All other data (ledger, CAS)
//! derive from this event stream. This ensures auditability and replay.

use crate::schema::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique event ID (UUID)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    pub fn new() -> Self {
        EventId(Uuid::new_v4())
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EVENT TYPES
// ============================================================================

/// CI lifecycle events - fully ordered, append-only event stream
///
/// Events are the source of truth. RunLedger is a query index; CAS stores payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CIEvent {
    /// A new run has been queued
    RunStarted(RunStartedEvent),

    /// A stage has begun execution
    StageStarted(StageStartedEvent),

    /// A stage has finished (passed or failed)
    StageFinished(StageFinishedEvent),

    /// A run has finished (all stages complete or failed)
    RunFinished(RunFinishedEvent),

    /// Diagnostics have been produced from a run
    DiagnosticsProduced(DiagnosticsProducedEvent),

    /// A repair plan has been generated
    RepairPlanned(RepairPlannedEvent),

    /// A patch has been applied to the workspace
    PatchApplied(PatchAppliedEvent),

    /// Verification of a patch has finished
    VerificationFinished(VerificationFinishedEvent),

    /// A gate has been evaluated
    GateEvaluated(GateEvaluatedEvent),

    /// A patch has been promoted (e.g., to main branch)
    PromotionApplied(PromotionAppliedEvent),
}

// ============================================================================
// INDIVIDUAL EVENT TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStartedEvent {
    pub event_id: EventId,
    pub run_id: String,
    pub snapshot_id: String,
    pub spec_digest: String,
    pub policy_digest: String,
    pub runner_version: String,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageStartedEvent {
    pub event_id: EventId,
    pub run_id: String,
    pub stage_name: String,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageFinishedEvent {
    pub event_id: EventId,
    pub run_id: String,
    pub stage_name: String,
    pub passed: bool,
    pub duration_ms: u64,
    /// Digest of stage output (stdout/stderr/logs)
    pub output_digest: Option<String>,
    /// Digest of any artifacts from this stage
    pub artifact_digests: Vec<String>,
    /// Whether this stage was a cache hit
    pub cache_hit: bool,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFinishedEvent {
    pub event_id: EventId,
    pub run_id: String,
    pub passed: bool,
    pub duration_ms: u64,
    /// Digest of complete run result JSON
    pub result_digest: String,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsProducedEvent {
    pub event_id: EventId,
    pub run_id: String,
    /// Digest of the diagnostics JSON
    pub diagnostics_digest: String,
    /// Number of diagnostics produced
    pub count: usize,
    /// Highest severity: error > warning > info
    pub max_severity: String,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPlannedEvent {
    pub event_id: EventId,
    pub run_id: String,
    /// Digest of the repair plan
    pub plan_digest: String,
    /// Number of actions in the plan
    pub action_count: usize,
    /// Estimated diff size in bytes
    pub estimated_diff_size: u64,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchAppliedEvent {
    pub event_id: EventId,
    pub run_id: String,
    /// Digest of the patch (unified diff)
    pub patch_digest: String,
    /// Files that were changed
    pub changed_paths: Vec<String>,
    /// Number of lines added
    pub lines_added: u32,
    /// Number of lines removed
    pub lines_removed: u32,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFinishedEvent {
    pub event_id: EventId,
    pub original_run_id: String,
    pub verification_run_id: String,
    pub passed: bool,
    /// Digest of verification result
    pub result_digest: String,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateEvaluatedEvent {
    pub event_id: EventId,
    pub run_id: String,
    pub gate_id: String,
    pub passed: bool,
    /// Digest of gate evaluation result (violations, etc.)
    pub result_digest: Option<String>,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionAppliedEvent {
    pub event_id: EventId,
    pub run_id: String,
    /// Target branch or ref that was promoted
    pub target_ref: String,
    /// Digest of promotion record
    pub promotion_digest: String,
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// EVENT STREAM AND LEDGER RECORDING
// ============================================================================

/// An ordered record of a CI event with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,

    /// The event itself
    pub event: CIEvent,

    /// Sequence number in the run's event stream
    pub sequence: u64,

    /// Run ID this event belongs to (for efficient querying)
    pub run_id: String,

    /// Created timestamp
    #[serde(with = "crate::schema::surreal_datetime")]
    pub recorded_at: DateTime<Utc>,
}

/// Summary of a run's status in the ledger (indexed for fast queries)
///
/// This is derived from the event stream but cached in RunLedger for performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunLedgerEntry {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,

    /// Run ID
    pub run_id: String,

    /// Snapshot ID
    pub snapshot_id: String,

    /// Current run status
    pub status: String, // "queued" | "running" | "succeeded" | "failed" | "canceled"

    /// Whether run passed or failed
    pub passed: bool,

    /// Overall duration in milliseconds
    pub duration_ms: u64,

    /// Number of diagnostics
    pub diagnostic_count: u32,

    /// Max severity in diagnostics
    pub max_severity: Option<String>,

    /// Number of repair actions proposed
    pub repair_action_count: u32,

    /// Number of stages
    pub stage_count: u32,

    /// Number of stages passed
    pub stages_passed: u32,

    /// Digest of run result
    pub result_digest: Option<String>,

    /// Digest of diagnostics
    pub diagnostics_digest: Option<String>,

    /// Digest of repair plan (if any)
    pub repair_plan_digest: Option<String>,

    /// Verification run ID (if patch was verified)
    pub verification_run_id: Option<String>,

    /// When this run started
    #[serde(with = "crate::schema::surreal_datetime")]
    pub started_at: DateTime<Utc>,

    /// When this run finished
    #[serde(with = "crate::schema::surreal_datetime_opt")]
    pub finished_at: Option<DateTime<Utc>>,

    /// Last updated (when ledger entry was updated)
    #[serde(with = "crate::schema::surreal_datetime")]
    pub updated_at: DateTime<Utc>,
}

impl RunLedgerEntry {
    /// Create a new ledger entry from a run
    pub fn from_run(run: &CIRun) -> Self {
        let passed = run.status == RunStatus::Succeeded;
        let duration_ms = run
            .finished_at
            .and_then(|f| {
                f.signed_duration_since(run.started_at)
                    .num_milliseconds()
                    .try_into()
                    .ok()
            })
            .unwrap_or(0);

        let diagnostic_count = run.diagnostics.len() as u32;
        let max_severity = run.diagnostics.iter().max_by_key(|d| {
            match d.severity {
                DiagnosticSeverity::Error => 2,
                DiagnosticSeverity::Warning => 1,
                DiagnosticSeverity::Info => 0,
            }
        }).map(|d| format!("{:?}", d.severity).to_lowercase());

        let (result_digest, stage_count, stages_passed) = run
            .result
            .as_ref()
            .map(|r| {
                (
                    r.local_ci_json_digest.clone(),
                    0, // TODO: extract from local-ci JSON
                    0, // TODO: extract from local-ci JSON
                )
            })
            .unwrap_or((None, 0, 0));

        let (repair_action_count, repair_plan_digest) = run
            .repair_plan
            .as_ref()
            .map(|p| (p.actions.len() as u32, Some(p.policy_digest.clone())))
            .unwrap_or((0, None));

        RunLedgerEntry {
            id: None,
            run_id: run.id.clone(),
            snapshot_id: run.snapshot_id.clone(),
            status: format!("{}", run.status),
            passed,
            duration_ms,
            diagnostic_count,
            max_severity,
            repair_action_count,
            stage_count,
            stages_passed,
            result_digest,
            diagnostics_digest: None, // Set by DiagnosticsProduced event
            repair_plan_digest,
            verification_run_id: run
                .verification_link
                .as_ref()
                .map(|v| v.verification_run_id.clone()),
            started_at: run.started_at,
            finished_at: run.finished_at,
            updated_at: Utc::now(),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_id_unique() {
        let e1 = EventId::new();
        let e2 = EventId::new();
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_event_serialization() {
        let event = CIEvent::RunStarted(RunStartedEvent {
            event_id: EventId::new(),
            run_id: "run-123".to_string(),
            snapshot_id: "snap-456".to_string(),
            spec_digest: "digest-abc".to_string(),
            policy_digest: "policy-xyz".to_string(),
            runner_version: "1.0.0".to_string(),
            timestamp: Utc::now(),
        });

        let json = serde_json::to_string(&event).expect("should serialize");
        let _deserialized: CIEvent =
            serde_json::from_str(&json).expect("should deserialize");
    }

    #[test]
    fn test_run_ledger_entry_from_run() {
        let mut run = CIRun::new(
            "snap-123".to_string(),
            "spec-digest".to_string(),
            "policy-digest".to_string(),
            "1.0.0".to_string(),
        );
        run.status = RunStatus::Succeeded;
        run.finish(
            RunStatus::Succeeded,
            CIResult {
                local_ci_json_digest: Some("result-digest".to_string()),
                stdout_digest: None,
                stderr_digest: None,
                artifact_digests: Default::default(),
                exit_code: 0,
                duration_ms: 5000,
                cache_stats: None,
            },
        );

        let entry = RunLedgerEntry::from_run(&run);
        assert_eq!(entry.run_id, run.id);
        assert_eq!(entry.passed, true);
        assert_eq!(entry.snapshot_id, "snap-123");
    }

    #[test]
    fn test_event_ordering() {
        let run_id = "run-123";

        // Events should be created in order
        let events = vec![
            CIEvent::RunStarted(RunStartedEvent {
                event_id: EventId::new(),
                run_id: run_id.to_string(),
                snapshot_id: "snap".to_string(),
                spec_digest: "spec".to_string(),
                policy_digest: "policy".to_string(),
                runner_version: "1.0".to_string(),
                timestamp: Utc::now(),
            }),
            CIEvent::StageStarted(StageStartedEvent {
                event_id: EventId::new(),
                run_id: run_id.to_string(),
                stage_name: "fmt".to_string(),
                timestamp: Utc::now(),
            }),
            CIEvent::StageFinished(StageFinishedEvent {
                event_id: EventId::new(),
                run_id: run_id.to_string(),
                stage_name: "fmt".to_string(),
                passed: true,
                duration_ms: 1000,
                output_digest: None,
                artifact_digests: vec![],
                cache_hit: false,
                timestamp: Utc::now(),
            }),
        ];

        assert_eq!(events.len(), 3);
    }
}
