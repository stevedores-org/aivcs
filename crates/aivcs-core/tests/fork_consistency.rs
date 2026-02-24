use aivcs_core::parallel::fork_agent_parallel;
use oxidized_state::{CommitId, CommitRecord, SurrealHandle};
use std::sync::Arc;

#[tokio::test]
async fn test_fork_consistency_and_uniqueness() {
    aivcs_core::init_tracing(false, tracing::Level::DEBUG);
    let handle = Arc::new(SurrealHandle::setup_db().await.unwrap());

    // 1. Create a parent commit
    let parent_state = serde_json::json!({"step": 0});
    let state_bytes = serde_json::to_vec(&parent_state).unwrap();
    let parent_id = CommitId::from_state(&state_bytes);
    handle
        .save_snapshot(&parent_id, parent_state)
        .await
        .unwrap();
    let parent_commit = CommitRecord::new(parent_id.clone(), vec![], "Initial", "test");
    handle.save_commit(&parent_commit).await.unwrap();

    // 2. Fork
    let result = fork_agent_parallel(Arc::clone(&handle), &parent_id.hash, 2, "fork")
        .await
        .unwrap();

    // 3. Verify each fork has a unique ID
    assert_ne!(result.commit_ids[0].hash, result.commit_ids[1].hash);

    // 4. Verify each fork has a consistent ID (id reflects stored state)
    for commit_id in result.commit_ids {
        let snapshot = handle.load_snapshot(&commit_id.hash).await.unwrap();
        let expected_id = CommitId::from_json(&snapshot.state);

        if commit_id.hash != expected_id.hash {
            println!("Mismatch for commit {}", commit_id.hash);
            println!(
                "Stored state: {}",
                serde_json::to_string(&snapshot.state).unwrap()
            );
            println!("Expected hash: {}", expected_id.hash);
            println!("Actual hash:   {}", commit_id.hash);
        }

        assert_eq!(
            commit_id.hash, expected_id.hash,
            "Commit ID must match canonical hash of stored state"
        );
    }
}

#[tokio::test]
async fn test_fork_with_non_object_state() {
    let handle = Arc::new(SurrealHandle::setup_db().await.unwrap());

    // 1. Create a parent commit with an array state
    let parent_state = serde_json::json!([1, 2, 3]);
    let parent_id = CommitId::from_json(&parent_state);
    handle
        .save_snapshot(&parent_id, parent_state)
        .await
        .unwrap();
    let parent_commit = CommitRecord::new(parent_id.clone(), vec![], "Initial", "test");
    handle.save_commit(&parent_commit).await.unwrap();

    // 2. Fork
    let result = fork_agent_parallel(Arc::clone(&handle), &parent_id.hash, 1, "fork")
        .await
        .unwrap();

    // 3. Verify it worked and injected metadata
    let commit_id = &result.commit_ids[0];
    let snapshot = handle.load_snapshot(&commit_id.hash).await.unwrap();

    assert!(snapshot.state.is_object());
    assert_eq!(snapshot.state["inner"], serde_json::json!([1, 2, 3]));
    assert_eq!(snapshot.state["_aivcs_fork"]["branch"], "fork-0");
}
