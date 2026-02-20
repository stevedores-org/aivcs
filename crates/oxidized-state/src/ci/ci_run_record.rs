//! CI run record for SurrealDB persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::surreal_dt;
use super::surreal_dt_opt;

/// CI run record stored in SurrealDB.
///
/// Mirrors the domain `CIRunSpec` + `CIResult` but uses string/JSON
/// types suitable for SurrealDB storage (Layer 0 cannot depend on Layer 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRunRecord {
    /// SurrealDB record ID.
    pub id: Option<surrealdb::sql::Thing>,

    /// Unique CI run ID (UUID string).
    pub ci_run_id: String,

    /// SHA256 hex digest of the run spec.
    pub spec_digest: String,

    /// Git commit SHA for this run.
    pub git_sha: String,

    /// What triggered this run: "manual", "pre_merge", "post_commit", "scheduled".
    pub trigger: String,

    /// JSON array of stage names.
    pub stages: serde_json::Value,

    /// Run status: "pending", "running", "passed", "failed", "cancelled".
    pub status: String,

    /// Total wall-clock duration in milliseconds.
    pub total_duration_ms: u64,

    /// Total number of diagnostics produced.
    pub diagnostics_count: u32,

    /// Number of stages that passed.
    pub passed: u32,

    /// Number of stages that failed.
    pub failed: u32,

    /// When the run was created.
    #[serde(with = "surreal_dt")]
    pub created_at: DateTime<Utc>,

    /// When the run finished (None if still running).
    #[serde(default, with = "surreal_dt_opt")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl CIRunRecord {
    /// Create a new CI run record in "pending" state.
    pub fn new(
        ci_run_id: String,
        spec_digest: String,
        git_sha: String,
        trigger: String,
        stages: Vec<String>,
    ) -> Self {
        Self {
            id: None,
            ci_run_id,
            spec_digest,
            git_sha,
            trigger,
            stages: serde_json::json!(stages),
            status: "pending".to_string(),
            total_duration_ms: 0,
            diagnostics_count: 0,
            passed: 0,
            failed: 0,
            created_at: Utc::now(),
            finished_at: None,
        }
    }

    /// Mark run as passed.
    pub fn pass(mut self, total_duration_ms: u64, passed: u32) -> Self {
        self.status = "passed".to_string();
        self.total_duration_ms = total_duration_ms;
        self.passed = passed;
        self.finished_at = Some(Utc::now());
        self
    }

    /// Mark run as failed.
    pub fn fail(mut self, total_duration_ms: u64, passed: u32, failed: u32) -> Self {
        self.status = "failed".to_string();
        self.total_duration_ms = total_duration_ms;
        self.passed = passed;
        self.failed = failed;
        self.finished_at = Some(Utc::now());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ci_run_record_new() {
        let record = CIRunRecord::new(
            "run-123".to_string(),
            "spec-abc".to_string(),
            "sha-def".to_string(),
            "manual".to_string(),
            vec!["fmt".to_string(), "test".to_string()],
        );

        assert_eq!(record.ci_run_id, "run-123");
        assert_eq!(record.status, "pending");
        assert_eq!(record.total_duration_ms, 0);
        assert_eq!(record.diagnostics_count, 0);
        assert!(record.finished_at.is_none());
    }

    #[test]
    fn test_ci_run_record_pass() {
        let record = CIRunRecord::new(
            "run-123".to_string(),
            "spec-abc".to_string(),
            "sha-def".to_string(),
            "pre_merge".to_string(),
            vec!["fmt".to_string()],
        )
        .pass(5000, 3);

        assert_eq!(record.status, "passed");
        assert_eq!(record.total_duration_ms, 5000);
        assert_eq!(record.passed, 3);
        assert!(record.finished_at.is_some());
    }

    #[test]
    fn test_ci_run_record_fail() {
        let record = CIRunRecord::new(
            "run-456".to_string(),
            "spec-xyz".to_string(),
            "sha-789".to_string(),
            "post_commit".to_string(),
            vec!["clippy".to_string()],
        )
        .fail(3000, 2, 1);

        assert_eq!(record.status, "failed");
        assert_eq!(record.passed, 2);
        assert_eq!(record.failed, 1);
        assert!(record.finished_at.is_some());
    }
}
