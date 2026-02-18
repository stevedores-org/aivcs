use aivcs_core::{replay_run, DeployByDigestRunner};
use chrono::{DateTime, Utc};
use oxidized_state::fakes::MemoryRunLedger;
use oxidized_state::{ContentDigest, RunEvent, RunLedger};

#[tokio::test]
async fn deploy_by_digest_matches_replay_golden() {
    let ledger = MemoryRunLedger::new();
    let digest = ContentDigest::from_bytes(b"spec-deploy-golden-v1");
    let ts = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .expect("parse timestamp")
        .with_timezone(&Utc);

    let output = DeployByDigestRunner::run_at(&ledger, &digest, "agent-golden", ts)
        .await
        .expect("run by digest");

    let (events, summary) = replay_run(&ledger, &output.run_id.0).await.expect("replay");

    let expected_events = vec![
        RunEvent {
            seq: 1,
            kind: "deploy_started".to_string(),
            payload: serde_json::json!({
                "spec_digest": digest.as_str(),
            }),
            timestamp: ts,
        },
        RunEvent {
            seq: 2,
            kind: "agent_executed".to_string(),
            payload: serde_json::json!({
                "agent_name": "agent-golden",
                "spec_digest": digest.as_str(),
            }),
            timestamp: ts,
        },
        RunEvent {
            seq: 3,
            kind: "deploy_completed".to_string(),
            payload: serde_json::json!({
                "success": true,
            }),
            timestamp: ts,
        },
    ];

    let expected_digest =
        ContentDigest::from_bytes(&serde_json::to_vec(&expected_events).expect("serialize events"));

    assert_eq!(events.len(), expected_events.len());
    for (actual, expected) in events.iter().zip(expected_events.iter()) {
        assert_eq!(actual.seq, expected.seq);
        assert_eq!(actual.kind, expected.kind);
        assert_eq!(actual.payload, expected.payload);
        assert_eq!(actual.timestamp, expected.timestamp);
    }
    assert_eq!(summary.event_count, 3);
    assert_eq!(summary.replay_digest, expected_digest.as_str());

    let run = ledger.get_run(&output.run_id).await.expect("get run");
    let run_summary = run.summary.expect("run summary");
    assert!(run_summary.final_state_digest.is_none());
}
