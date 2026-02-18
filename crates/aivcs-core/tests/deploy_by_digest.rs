use std::sync::Arc;

use aivcs_core::deploy::deploy_by_digest;
use aivcs_core::domain::agent_spec::AgentSpec;
use aivcs_core::domain::error::AivcsError;
use oxidized_state::fakes::{MemoryReleaseRegistry, MemoryRunLedger};
use oxidized_state::storage_traits::{
    ContentDigest, ReleaseMetadata, ReleaseRegistry, RunLedger, RunStatus,
};

fn make_spec(seed: &str) -> AgentSpec {
    AgentSpec::new(
        "abc123def456abc123def456abc123def456abc1".to_string(),
        format!("graph-{}", seed),
        format!("prompts-{}", seed),
        format!("tools-{}", seed),
        format!("config-{}", seed),
    )
    .expect("make_spec")
}

fn fixed_timestamp() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .expect("parse timestamp")
        .with_timezone(&chrono::Utc)
}

/// Promote a spec into a fresh registry, returning the registry.
async fn setup_registry(agent_name: &str, seed: &str) -> MemoryReleaseRegistry {
    let registry = MemoryReleaseRegistry::new();
    let spec = make_spec(seed);
    let digest = ContentDigest::try_from(spec.spec_digest.clone()).expect("valid digest");
    let metadata = ReleaseMetadata {
        version_label: None,
        promoted_by: "ci".to_string(),
        notes: None,
    };
    registry
        .promote(agent_name, &digest, metadata)
        .await
        .expect("promote");
    registry
}

// -----------------------------------------------------------------------
// 1. Two identical deploys produce identical replay_digest (golden eq)
// -----------------------------------------------------------------------
#[tokio::test]
async fn deployed_digest_matches_replay_golden() {
    let registry = setup_registry("agent-a", "v1").await;
    let ledger_a: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
    let ledger_b: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
    let ts = Some(fixed_timestamp());
    let inputs = serde_json::json!({"prompt": "hello"});

    let result_a = deploy_by_digest(&registry, ledger_a, "agent-a", inputs.clone(), ts)
        .await
        .expect("deploy_a");
    let result_b = deploy_by_digest(&registry, ledger_b, "agent-a", inputs, ts)
        .await
        .expect("deploy_b");

    assert_eq!(
        result_a.summary.replay_digest,
        result_b.summary.replay_digest
    );
}

// -----------------------------------------------------------------------
// 2. spec_digest matches the promoted release
// -----------------------------------------------------------------------
#[tokio::test]
async fn deploy_returns_correct_spec_digest() {
    let spec = make_spec("v1");
    let registry = setup_registry("agent-b", "v1").await;
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let result = deploy_by_digest(
        &registry,
        ledger,
        "agent-b",
        serde_json::json!({}),
        Some(fixed_timestamp()),
    )
    .await
    .expect("deploy");

    assert_eq!(result.spec_digest, spec.spec_digest);
}

// -----------------------------------------------------------------------
// 3. Run status is Completed
// -----------------------------------------------------------------------
#[tokio::test]
async fn deploy_run_status_completed() {
    let registry = setup_registry("agent-c", "v1").await;
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let result = deploy_by_digest(
        &registry,
        ledger,
        "agent-c",
        serde_json::json!({}),
        Some(fixed_timestamp()),
    )
    .await
    .expect("deploy");

    assert_eq!(result.summary.status, RunStatus::Completed);
}

// -----------------------------------------------------------------------
// 4. Exactly two events emitted
// -----------------------------------------------------------------------
#[tokio::test]
async fn deploy_emits_two_events() {
    let registry = setup_registry("agent-d", "v1").await;
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let result = deploy_by_digest(
        &registry,
        ledger,
        "agent-d",
        serde_json::json!({}),
        Some(fixed_timestamp()),
    )
    .await
    .expect("deploy");

    assert_eq!(result.summary.event_count, 2);
}

// -----------------------------------------------------------------------
// 5. Unknown agent returns error
// -----------------------------------------------------------------------
#[tokio::test]
async fn deploy_no_release_returns_error() {
    let registry = MemoryReleaseRegistry::new(); // no releases
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let err = deploy_by_digest(
        &registry,
        ledger,
        "nonexistent-agent",
        serde_json::json!({}),
        None,
    )
    .await
    .unwrap_err();

    match err {
        AivcsError::ReleaseConflict(msg) => {
            assert!(msg.contains("nonexistent-agent"));
        }
        other => panic!("Expected ReleaseConflict, got {:?}", other),
    }
}

// -----------------------------------------------------------------------
// 6. graph_started event payload contains inputs
// -----------------------------------------------------------------------
#[tokio::test]
async fn deploy_inputs_in_event_payload() {
    let registry = setup_registry("agent-f", "v1").await;
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
    let inputs = serde_json::json!({"prompt": "test input", "temperature": 0.7});

    let result = deploy_by_digest(
        &registry,
        Arc::clone(&ledger),
        "agent-f",
        inputs.clone(),
        Some(fixed_timestamp()),
    )
    .await
    .expect("deploy");

    // Fetch events from the ledger and check the first event's payload
    let events = ledger.get_events(&result.run_id).await.expect("get_events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, "graph_started");
    assert_eq!(events[0].payload["inputs"], inputs);
    assert_eq!(events[1].kind, "graph_completed");
}
