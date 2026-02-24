use oxidized_state::{BranchRecord, CommitId, CommitRecord, SurrealHandle};
use semantic_rag_merge::semantic_merge;

#[tokio::test]
async fn test_merge_creates_snapshot() {
    let handle = SurrealHandle::setup_db().await.unwrap();

    // 1. Initialize
    let initial_state = serde_json::json!({"step": 0});
    let init_id = CommitId::from_state(serde_json::to_vec(&initial_state).unwrap().as_slice());
    handle.save_snapshot(&init_id, initial_state).await.unwrap();
    let init_commit = CommitRecord::new(init_id.clone(), vec![], "Initial", "test");
    handle.save_commit(&init_commit).await.unwrap();
    handle
        .save_branch(&BranchRecord::new("main", &init_id.hash, true))
        .await
        .unwrap();

    // 2. Create branch A
    let state_a = serde_json::json!({"step": 1, "branch": "A"});
    let id_a = CommitId::from_state(serde_json::to_vec(&state_a).unwrap().as_slice());
    handle.save_snapshot(&id_a, state_a).await.unwrap();
    let commit_a = CommitRecord::new(id_a.clone(), vec![init_id.hash.clone()], "A", "test");
    handle.save_commit(&commit_a).await.unwrap();

    // 3. Create branch B
    let state_b = serde_json::json!({"step": 1, "branch": "B"});
    let id_b = CommitId::from_state(serde_json::to_vec(&state_b).unwrap().as_slice());
    handle.save_snapshot(&id_b, state_b).await.unwrap();
    let commit_b = CommitRecord::new(id_b.clone(), vec![init_id.hash.clone()], "B", "test");
    handle.save_commit(&commit_b).await.unwrap();

    // 4. Merge A and B
    let merge_result = semantic_merge(&handle, &id_a.hash, &id_b.hash, "Merge A and B", "test")
        .await
        .unwrap();

    // 5. Try to load snapshot of merge commit
    let snapshot = handle
        .load_snapshot(&merge_result.merge_commit_id.hash)
        .await;
    assert!(
        snapshot.is_ok(),
        "Snapshot should be created for merge commit. Error: {:?}",
        snapshot.err()
    );
}
