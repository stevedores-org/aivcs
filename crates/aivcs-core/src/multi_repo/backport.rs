//! Backport / cherry-pick policy automation.
//!
//! [`BackportPolicy`] declares which commits must be applied to which target
//! branches. [`BackportExecutor`] resolves and applies the tasks, recording
//! each operation in the `RunLedger` for full provenance.

use std::sync::Arc;

use oxidized_state::storage_traits::{ContentDigest, RunEvent, RunLedger, RunMetadata, RunSummary};
use serde::{Deserialize, Serialize};

use crate::multi_repo::error::{MultiRepoError, MultiRepoResult};
use crate::recording::GraphRunRecorder;

/// Policy declaring which commits from a source branch must be backported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackportPolicy {
    /// Source branch name or simple glob (`*` matches any single path segment).
    pub source_branch: String,
    /// Target branches commits are backported to.
    pub target_branches: Vec<String>,
    /// Optional explicit commit filter. When `Some`, only commits in this list
    /// are backported; when `None`, all provided commits are backported.
    pub commit_filter: Option<Vec<String>>,
    /// When `true`, the first failure stops further execution.
    pub fail_fast: bool,
}

impl BackportPolicy {
    /// Check whether `branch` matches the policy's source branch pattern.
    pub fn matches_source_branch(&self, branch: &str) -> bool {
        glob_match(&self.source_branch, branch)
    }
}

/// A single resolved backport task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackportTask {
    pub commit_sha: String,
    pub source_branch: String,
    pub target_branch: String,
}

/// Result of applying one backport task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackportOutcome {
    pub task: BackportTask,
    pub success: bool,
    pub conflict_files: Vec<String>,
    pub applied_commit_sha: Option<String>,
    pub error: Option<String>,
}

/// Applies backport tasks, recording each operation in a `RunLedger`.
pub struct BackportExecutor {
    ledger: Arc<dyn RunLedger>,
}

impl BackportExecutor {
    pub fn new(ledger: Arc<dyn RunLedger>) -> Self {
        Self { ledger }
    }

    /// Resolve all backport tasks implied by `policy` and `commits`.
    ///
    /// When `policy.commit_filter` is set, only commits in the filter are
    /// included. The result is the cross-product of included commits ×
    /// target_branches.
    pub fn resolve_tasks(&self, policy: &BackportPolicy, commits: &[String]) -> Vec<BackportTask> {
        let filtered: Vec<&String> = commits
            .iter()
            .filter(|c| policy.commit_filter.as_ref().is_none_or(|f| f.contains(c)))
            .collect();

        let mut tasks = Vec::new();
        for commit in filtered {
            for target in &policy.target_branches {
                tasks.push(BackportTask {
                    commit_sha: commit.clone(),
                    source_branch: policy.source_branch.clone(),
                    target_branch: target.clone(),
                });
            }
        }
        tasks
    }

    /// Execute `tasks` using the provided `cherry_pick_fn` backend.
    ///
    /// `cherry_pick_fn(commit_sha, target_branch)` returns
    /// `(success, conflict_files, applied_sha)`.
    ///
    /// Each task is recorded as `ToolCalled` / `ToolReturned` / `ToolFailed`
    /// events in the ledger. When `policy.fail_fast` is `true`, the first
    /// failure halts execution.
    pub async fn execute<F>(
        &self,
        tasks: Vec<BackportTask>,
        policy: &BackportPolicy,
        originating_run_id: &str,
        cherry_pick_fn: F,
    ) -> MultiRepoResult<Vec<BackportOutcome>>
    where
        F: Fn(&str, &str) -> (bool, Vec<String>, Option<String>) + Send + Sync,
    {
        let spec = ContentDigest::from_bytes(format!("backport:{}", originating_run_id).as_bytes());
        let metadata = RunMetadata {
            git_sha: None,
            agent_name: "backport-executor".to_string(),
            tags: serde_json::json!({ "originating_run_id": originating_run_id }),
        };

        let recorder = GraphRunRecorder::start(Arc::clone(&self.ledger), &spec, metadata)
            .await
            .map_err(|e| MultiRepoError::Storage(e.to_string()))?;

        let mut outcomes = Vec::new();
        let mut seq: u64 = 1;

        for task in tasks {
            // Record ToolCalled event.
            let call_event = RunEvent {
                seq,
                kind: "ToolCalled".to_string(),
                payload: serde_json::json!({
                    "tool_name": "cherry_pick",
                    "commit_sha": task.commit_sha,
                    "target_branch": task.target_branch,
                }),
                timestamp: chrono::Utc::now(),
            };
            self.ledger
                .append_event(recorder.run_id(), call_event)
                .await
                .map_err(|e| MultiRepoError::Storage(e.to_string()))?;
            seq += 1;

            let (success, conflict_files, applied_sha) =
                cherry_pick_fn(&task.commit_sha, &task.target_branch);

            let outcome = BackportOutcome {
                task: task.clone(),
                success,
                conflict_files: conflict_files.clone(),
                applied_commit_sha: applied_sha.clone(),
                error: if success {
                    None
                } else {
                    Some(format!(
                        "cherry-pick failed; conflicts in: {:?}",
                        conflict_files
                    ))
                },
            };

            // Record ToolReturned / ToolFailed.
            let result_event = RunEvent {
                seq,
                kind: if success {
                    "ToolReturned".to_string()
                } else {
                    "ToolFailed".to_string()
                },
                payload: serde_json::json!({
                    "commit_sha": task.commit_sha,
                    "applied_sha": applied_sha,
                    "conflict_files": conflict_files,
                }),
                timestamp: chrono::Utc::now(),
            };
            self.ledger
                .append_event(recorder.run_id(), result_event)
                .await
                .map_err(|e| MultiRepoError::Storage(e.to_string()))?;
            seq += 1;

            let failed = !outcome.success;
            outcomes.push(outcome);

            if failed && policy.fail_fast {
                break;
            }
        }

        let total_events = seq - 1;
        let overall_success = outcomes.iter().all(|o| o.success);

        if overall_success {
            recorder
                .finish_ok(RunSummary {
                    total_events,
                    final_state_digest: None,
                    duration_ms: 0,
                    success: true,
                })
                .await
                .map_err(|e| MultiRepoError::Storage(e.to_string()))?;
        } else {
            recorder
                .finish_err(RunSummary {
                    total_events,
                    final_state_digest: None,
                    duration_ms: 0,
                    success: false,
                })
                .await
                .map_err(|e| MultiRepoError::Storage(e.to_string()))?;
        }

        Ok(outcomes)
    }
}

/// Simple glob matcher: `*` matches any sequence of non-slash characters.
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    // Split on `*` and check that each segment appears in order.
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut remaining = value;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match remaining.find(part) {
            None => return false,
            Some(pos) => {
                // First part must be a prefix.
                if i == 0 && pos != 0 {
                    return false;
                }
                remaining = &remaining[pos + part.len()..];
            }
        }
    }
    // Last part must be a suffix (no trailing characters).
    if let Some(last) = parts.last() {
        if !last.is_empty() && !value.ends_with(last) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidized_state::fakes::MemoryRunLedger;

    fn policy(targets: &[&str], filter: Option<Vec<String>>) -> BackportPolicy {
        BackportPolicy {
            source_branch: "main".to_string(),
            target_branches: targets.iter().map(|s| s.to_string()).collect(),
            commit_filter: filter,
            fail_fast: false,
        }
    }

    #[test]
    fn test_resolve_tasks_produces_correct_cross_product() {
        let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
        let exec = BackportExecutor::new(ledger);
        let p = policy(&["release/1.0", "release/1.1"], None);
        let commits: Vec<String> = vec!["sha1", "sha2", "sha3"]
            .into_iter()
            .map(String::from)
            .collect();
        let tasks = exec.resolve_tasks(&p, &commits);
        // 3 commits × 2 branches = 6 tasks.
        assert_eq!(tasks.len(), 6);
    }

    #[test]
    fn test_resolve_tasks_filters_by_commit_filter() {
        let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
        let exec = BackportExecutor::new(ledger);
        let p = policy(
            &["release/1.0"],
            Some(vec!["sha1".to_string(), "sha3".to_string()]),
        );
        let commits: Vec<String> = vec!["sha1", "sha2", "sha3"]
            .into_iter()
            .map(String::from)
            .collect();
        let tasks = exec.resolve_tasks(&p, &commits);
        // sha2 filtered out → 2 commits × 1 branch = 2 tasks.
        assert_eq!(tasks.len(), 2);
        assert!(!tasks.iter().any(|t| t.commit_sha == "sha2"));
    }

    #[tokio::test]
    async fn test_execute_records_events_per_task() {
        let ledger = Arc::new(MemoryRunLedger::new());
        let exec = BackportExecutor::new(Arc::clone(&ledger) as Arc<dyn RunLedger>);

        let p = policy(&["release/1.0"], None);
        let tasks = exec.resolve_tasks(&p, &["abc123".to_string()]);

        let outcomes = exec
            .execute(tasks, &p, "run-000", |_sha, _branch| {
                (true, vec![], Some("new_sha".to_string()))
            })
            .await
            .unwrap();

        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].success);
        assert_eq!(outcomes[0].applied_commit_sha.as_deref(), Some("new_sha"));
    }

    #[tokio::test]
    async fn test_execute_fail_fast_stops_on_first_failure() {
        let ledger: Arc<dyn RunLedger> = Arc::new(MemoryRunLedger::new());
        let exec = BackportExecutor::new(Arc::clone(&ledger));

        let mut p = policy(&["release/1.0"], None);
        p.fail_fast = true;
        let commits: Vec<String> = vec!["sha1", "sha2", "sha3"]
            .into_iter()
            .map(String::from)
            .collect();
        let tasks = exec.resolve_tasks(&p, &commits);

        let outcomes = exec
            .execute(tasks, &p, "run-001", |sha, _branch| {
                if sha == "sha1" {
                    (false, vec!["conflict.rs".to_string()], None)
                } else {
                    (true, vec![], Some("ok".to_string()))
                }
            })
            .await
            .unwrap();

        // Stopped after the first failure.
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].success);
    }

    #[test]
    fn test_glob_match_wildcard() {
        assert!(glob_match("release/*", "release/1.0"));
        assert!(glob_match("release/*", "release/main"));
        assert!(!glob_match("release/*", "main"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn test_policy_source_branch_matching() {
        let p = policy(&["release/1.0"], None);
        assert!(p.matches_source_branch("main"));
        assert!(!p.matches_source_branch("develop"));
    }
}
