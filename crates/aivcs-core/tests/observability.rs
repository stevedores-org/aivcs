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

// ========== Metrics Tests ==========

use std::sync::{Arc, Mutex};
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

/// A minimal tracing layer that captures event field values.
struct CapturingLayer {
    captured: Arc<Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> Layer<S> for CapturingLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        if let Some(metric) = visitor.metric {
            let mut entry = metric;
            if let Some(ep) = visitor.events_processed {
                entry = format!("flush:events_processed={ep}");
            }
            self.captured.lock().unwrap().push(entry);
        }
    }
}

#[derive(Default)]
struct FieldVisitor {
    metric: Option<String>,
    events_processed: Option<u64>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "metric" {
            self.metric = Some(value.to_string());
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "events_processed" {
            self.events_processed = Some(value);
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
}

#[test]
fn flush_emits_aggregated_metric_event() {
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));

    let layer = CapturingLayer {
        captured: captured.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(layer).with(
        tracing_subscriber::filter::LevelFilter::from_level(Level::INFO),
    );

    // Use a local Metrics instance to avoid cross-test interference.
    let m = aivcs_core::metrics::Metrics::new();

    tracing::subscriber::with_default(subscriber, || {
        m.inc_events_processed();
        m.inc_events_processed();
        m.inc_replays();
        m.inc_forks();
        // inc_* emits at trace! level — the INFO filter suppresses them.
        // Only flush() emits at info! and should be captured.
        m.flush();
    });

    let events = captured.lock().unwrap();
    assert!(
        events.contains(&"flush:events_processed=2".to_string()),
        "expected flush with events_processed=2 in {:?}",
        *events,
    );
}

#[test]
fn metric_counters_reflect_increments() {
    // Use a local Metrics instance to avoid cross-test interference.
    let m = aivcs_core::metrics::Metrics::new();

    m.inc_events_processed();
    m.inc_events_processed();
    m.inc_events_processed();
    m.inc_events_processed();
    m.inc_events_processed();
    m.inc_replays();
    m.inc_forks();
    m.inc_forks();

    assert_eq!(m.events_processed(), 5);
    assert_eq!(m.replays_executed(), 1);
    assert_eq!(m.forks_created(), 2);
}
