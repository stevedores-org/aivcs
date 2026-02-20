//! CI run specification and digest computation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::digest;
use crate::domain::error::{AivcsError, Result};

/// What triggered a CI run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CITrigger {
    /// Manually triggered by user or agent.
    Manual,
    /// Triggered before merge (PR check).
    PreMerge,
    /// Triggered after a commit lands.
    PostCommit,
    /// Triggered on a schedule.
    Scheduled,
}

/// Specification for a CI run, including stages, budgets, and trigger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CIRunSpec {
    /// Unique identifier for this run spec.
    pub run_id: Uuid,

    /// SHA256 hex digest computed from the spec fields.
    pub spec_digest: String,

    /// Git commit SHA for this CI run.
    pub git_sha: String,

    /// Ordered stages to execute (e.g. ["fmt", "clippy", "test"]).
    pub stages: Vec<String>,

    /// What triggered this run.
    pub trigger: CITrigger,

    /// Per-stage timeout in milliseconds.
    pub stage_timeout_ms: u64,

    /// Maximum total duration in milliseconds.
    pub total_timeout_ms: u64,

    /// When this spec was created.
    pub created_at: DateTime<Utc>,
}

/// Input fields for computing CI run spec digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRunSpecFields {
    pub git_sha: String,
    pub stages: Vec<String>,
    pub trigger: CITrigger,
    pub stage_timeout_ms: u64,
    pub total_timeout_ms: u64,
}

impl CIRunSpec {
    /// Create a new CI run spec with computed digest.
    pub fn new(git_sha: String, stages: Vec<String>, trigger: CITrigger) -> Result<Self> {
        if git_sha.is_empty() {
            return Err(AivcsError::InvalidAgentSpec(
                "git_sha cannot be empty".to_string(),
            ));
        }
        if stages.is_empty() {
            return Err(AivcsError::InvalidAgentSpec(
                "stages cannot be empty".to_string(),
            ));
        }

        let fields = CIRunSpecFields {
            git_sha: git_sha.clone(),
            stages: stages.clone(),
            trigger,
            stage_timeout_ms: 300_000,
            total_timeout_ms: 1_200_000,
        };

        let spec_digest = Self::compute_digest(&fields)?;

        Ok(Self {
            run_id: Uuid::new_v4(),
            spec_digest,
            git_sha,
            stages,
            trigger,
            stage_timeout_ms: 300_000,
            total_timeout_ms: 1_200_000,
            created_at: Utc::now(),
        })
    }

    /// Compute stable SHA256 digest from canonical JSON (RFC 8785-compliant).
    pub fn compute_digest(fields: &CIRunSpecFields) -> Result<String> {
        let json = serde_json::to_value(fields)?;
        digest::compute_digest(&json)
    }

    /// Verify that spec_digest matches computed digest.
    pub fn verify_digest(&self) -> Result<()> {
        let fields = CIRunSpecFields {
            git_sha: self.git_sha.clone(),
            stages: self.stages.clone(),
            trigger: self.trigger,
            stage_timeout_ms: self.stage_timeout_ms,
            total_timeout_ms: self.total_timeout_ms,
        };

        let computed = Self::compute_digest(&fields)?;
        if computed != self.spec_digest {
            return Err(AivcsError::DigestMismatch {
                expected: self.spec_digest.clone(),
                actual: computed,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ci_run_spec_serde_roundtrip() {
        let spec = CIRunSpec::new(
            "abc123".to_string(),
            vec!["fmt".to_string(), "clippy".to_string(), "test".to_string()],
            CITrigger::PreMerge,
        )
        .expect("create spec");

        let json = serde_json::to_string(&spec).expect("serialize");
        let deserialized: CIRunSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(spec, deserialized);
    }

    #[test]
    fn test_ci_run_spec_digest_stable() {
        let fields1 = CIRunSpecFields {
            git_sha: "abc123".to_string(),
            stages: vec!["fmt".to_string(), "test".to_string()],
            trigger: CITrigger::Manual,
            stage_timeout_ms: 300_000,
            total_timeout_ms: 1_200_000,
        };
        let fields2 = fields1.clone();

        let d1 = CIRunSpec::compute_digest(&fields1).expect("digest 1");
        let d2 = CIRunSpec::compute_digest(&fields2).expect("digest 2");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_ci_run_spec_digest_changes_on_mutation() {
        let fields1 = CIRunSpecFields {
            git_sha: "abc123".to_string(),
            stages: vec!["fmt".to_string()],
            trigger: CITrigger::Manual,
            stage_timeout_ms: 300_000,
            total_timeout_ms: 1_200_000,
        };
        let fields2 = CIRunSpecFields {
            git_sha: "abc123".to_string(),
            stages: vec!["fmt".to_string(), "test".to_string()],
            trigger: CITrigger::Manual,
            stage_timeout_ms: 300_000,
            total_timeout_ms: 1_200_000,
        };

        let d1 = CIRunSpec::compute_digest(&fields1).expect("digest 1");
        let d2 = CIRunSpec::compute_digest(&fields2).expect("digest 2");
        assert_ne!(d1, d2);
    }

    #[test]
    fn test_ci_run_spec_verify_digest() {
        let spec = CIRunSpec::new(
            "abc123".to_string(),
            vec!["fmt".to_string()],
            CITrigger::PostCommit,
        )
        .expect("create spec");

        assert!(spec.verify_digest().is_ok());
    }

    #[test]
    fn test_ci_run_spec_rejects_empty_git_sha() {
        let result = CIRunSpec::new("".to_string(), vec!["fmt".to_string()], CITrigger::Manual);
        assert!(result.is_err());
    }

    #[test]
    fn test_ci_run_spec_rejects_empty_stages() {
        let result = CIRunSpec::new("abc123".to_string(), vec![], CITrigger::Manual);
        assert!(result.is_err());
    }

    #[test]
    fn test_ci_trigger_serde() {
        let triggers = [
            CITrigger::Manual,
            CITrigger::PreMerge,
            CITrigger::PostCommit,
            CITrigger::Scheduled,
        ];
        for trigger in &triggers {
            let json = serde_json::to_string(trigger).expect("serialize");
            let deserialized: CITrigger = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*trigger, deserialized);
        }
    }
}
