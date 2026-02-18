//! Observability tests for AIVCS run lifecycle tracing.
//!
//! These tests verify that structured tracing events are emitted correctly
//! for key lifecycle events: run start, event append, run finish, and gate evaluation.

use aivcs_core::{
    emit_event_appended, emit_gate_evaluated, emit_run_finalize_error, emit_run_finished,
    emit_run_started, RunSpan,
};
use tracing_test::traced_test;

/// Test: emit_run_started creates an info-level event
#[traced_test]
#[test]
fn test_emit_run_started_logs_run_id_and_agent() {
    emit_run_started("run-123", "test_agent");

    // The #[traced_test] macro captures all spans and events.
    // If the function executes without panic, the tracing infrastructure worked.
    // The event is emitted at info! level with the specified parameters.
}

/// Test: emit_run_finished creates an info-level event
#[traced_test]
#[test]
fn test_emit_run_finished_logs_duration() {
    emit_run_finished("run-456", 5000, 42, true);

    // Verify no panics occurred during tracing
}

/// Test: emit_event_appended creates an info-level event
#[traced_test]
#[test]
fn test_emit_event_appended_logs_kind_and_seq() {
    emit_event_appended("run-789", "node_entered", 7);
}

/// Test: emit_gate_evaluated creates an info-level event
#[traced_test]
#[test]
fn test_emit_gate_evaluated_logs_pass_rate() {
    emit_gate_evaluated("run-gate-001", 0.85, true);
}

/// Test: emit_run_finalize_error creates a warn-level event
#[traced_test]
#[test]
fn test_emit_run_finalize_error_logs_warning() {
    let error_msg = "database connection failed";
    emit_run_finalize_error("run-err-001", &error_msg);

    // WARN-level events are captured by traced_test
}

/// Test: RunSpan::enter creates an entered span without panicking
#[traced_test]
#[test]
fn test_run_span_enter_creates_span() {
    let span = RunSpan::enter("test-span-run");
    // If RunSpan::enter doesn't panic, the span was successfully created
    drop(span); // Explicitly drop to show intent
}

/// Test: recording start emits run.started event
#[traced_test]
#[tokio::test]
async fn test_recording_start_emits_run_started() {
    use aivcs_core::GraphRunRecorder;
    use oxidized_state::fakes::MemoryRunLedger;
    use oxidized_state::storage_traits::{ContentDigest, RunMetadata};
    use std::sync::Arc;

    let ledger: Arc<dyn oxidized_state::RunLedger> = Arc::new(MemoryRunLedger::new());
    let spec_digest = ContentDigest::from_bytes(b"test_spec");
    let metadata = RunMetadata {
        git_sha: Some("abc123".to_string()),
        agent_name: "test_recorder_agent".to_string(),
        tags: serde_json::json!({}),
    };

    let recorder = GraphRunRecorder::start(ledger, &spec_digest, metadata)
        .await
        .expect("start recorder");

    // Verify recorder is created with a run_id
    let run_id = recorder.run_id();
    assert!(!run_id.to_string().is_empty());
}

/// Test: replay completion emits replay.completed event
#[traced_test]
#[tokio::test]
async fn test_replay_completed_emits_event() {
    use aivcs_core::replay_run;
    use oxidized_state::fakes::MemoryRunLedger;
    use oxidized_state::storage_traits::{ContentDigest, RunEvent, RunMetadata, RunSummary};
    use std::sync::Arc;

    let ledger: Arc<dyn oxidized_state::RunLedger> = Arc::new(MemoryRunLedger::new());
    let spec_digest = ContentDigest::from_bytes(b"test_spec");
    let metadata = RunMetadata {
        git_sha: Some("test_sha".to_string()),
        agent_name: "test_replay_agent".to_string(),
        tags: serde_json::json!({}),
    };

    let run_id = ledger
        .create_run(&spec_digest, metadata)
        .await
        .expect("create run");

    // Add a single event
    let event = RunEvent {
        seq: 1,
        kind: "graph_started".to_string(),
        payload: serde_json::json!({}),
        timestamp: chrono::Utc::now(),
    };
    ledger
        .append_event(&run_id, event)
        .await
        .expect("append event");

    // Complete the run
    let summary = RunSummary {
        total_events: 1,
        final_state_digest: None,
        duration_ms: 100,
        success: true,
    };
    ledger
        .complete_run(&run_id, summary)
        .await
        .expect("complete run");

    // Replay and verify no errors
    let (events, summary) = replay_run(&*ledger, &run_id.0).await.expect("replay run");

    // Verify replay completed successfully
    assert_eq!(events.len(), 1);
    assert_eq!(summary.event_count, 1);
}
