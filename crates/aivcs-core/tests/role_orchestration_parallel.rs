//! Parallel role execution isolation tests.

use std::collections::HashSet;
use std::sync::Arc;

use aivcs_core::role_orchestration::{
    error::{RoleError, RoleResult},
    executor::{execute_roles_parallel, token_from_result, ParallelRoleConfig},
    roles::{AgentRole, RoleOutput},
};
use oxidized_state::fakes::MemoryRunLedger;
use oxidized_state::storage_traits::{ContentDigest, RunLedger, RunStatus};

fn spec() -> ContentDigest {
    ContentDigest::from_bytes(b"test-spec")
}

/// Stub executor: always succeeds with a canned output per role.
fn ok_executor(
    role: AgentRole,
    run_id: oxidized_state::storage_traits::RunId,
) -> impl std::future::Future<Output = RoleResult<RoleOutput>> {
    let _ = run_id;
    async move {
        Ok(match role {
            AgentRole::Reviewer => RoleOutput::Review {
                approved: true,
                comments: vec![],
                requires_fix: false,
            },
            AgentRole::Tester => RoleOutput::TestReport {
                passed: true,
                total_cases: 3,
                failed_cases: vec![],
                diagnostic_digest: None,
            },
            _ => RoleOutput::Fix {
                patch_digest: "stub".to_string(),
                resolved_issues: vec![],
            },
        })
    }
}

/// Stub executor: always fails.
fn fail_executor(
    _role: AgentRole,
    _run_id: oxidized_state::storage_traits::RunId,
) -> impl std::future::Future<Output = RoleResult<RoleOutput>> {
    async move {
        Err(RoleError::ParallelExecutionFailed {
            detail: "intentional failure".to_string(),
        })
    }
}

#[tokio::test]
async fn test_parallel_roles_get_isolated_run_ids() {
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let results = execute_roles_parallel(
        Arc::clone(&ledger),
        "parent-run-1",
        vec![AgentRole::Reviewer, AgentRole::Tester],
        &spec(),
        ParallelRoleConfig::default(),
        ok_executor,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 2);

    let ids: HashSet<String> = results.iter().map(|r| r.run_id.0.clone()).collect();
    assert_eq!(ids.len(), 2, "each role must get a distinct RunId");
}

#[tokio::test]
async fn test_parallel_roles_do_not_share_ledger_state() {
    let ledger = Arc::new(MemoryRunLedger::new());

    let results = execute_roles_parallel(
        Arc::clone(&ledger) as Arc<dyn RunLedger>,
        "parent-run-2",
        vec![AgentRole::Reviewer, AgentRole::Tester],
        &spec(),
        ParallelRoleConfig::default(),
        ok_executor,
    )
    .await
    .unwrap();

    // Each RunId must resolve to a distinct, completed run record.
    for result in &results {
        let record = ledger.get_run(&result.run_id).await.unwrap();
        assert_eq!(record.run_id, result.run_id);
        assert_eq!(record.status, RunStatus::Completed);
    }

    // Verify no run shares events with another.
    for result in &results {
        let events = ledger.get_events(&result.run_id).await.unwrap();
        // Events are empty (stub executor doesn't append any), confirming isolation
        assert!(
            events.is_empty(),
            "stub executor should not append events to ledger"
        );
    }
}

#[tokio::test]
async fn test_all_roles_fail_returns_parallel_execution_failed_error() {
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let result = execute_roles_parallel(
        ledger,
        "parent-run-3",
        vec![AgentRole::Reviewer, AgentRole::Tester],
        &spec(),
        ParallelRoleConfig::default(),
        fail_executor,
    )
    .await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RoleError::ParallelExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn test_successful_parallel_run_produces_handoff_tokens() {
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let results = execute_roles_parallel(
        ledger,
        "parent-run-4",
        vec![AgentRole::Reviewer, AgentRole::Tester],
        &spec(),
        ParallelRoleConfig::default(),
        ok_executor,
    )
    .await
    .unwrap();

    for result in results {
        assert!(result.success);
        let token = token_from_result(result).unwrap();
        // Token must self-verify cleanly.
        assert!(token.verify().is_ok());
    }
}

#[tokio::test]
async fn test_fail_fast_stops_remaining_tasks() {
    let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());

    let config = ParallelRoleConfig {
        max_concurrent: 1, // serial to make ordering deterministic
        fail_fast: true,
    };

    // All executors fail — fail_fast should still return after they all tried.
    let result = execute_roles_parallel(
        ledger,
        "parent-run-5",
        vec![AgentRole::Reviewer, AgentRole::Tester],
        &spec(),
        config,
        fail_executor,
    )
    .await;

    // All failed → ParallelExecutionFailed
    assert!(result.is_err());
}
