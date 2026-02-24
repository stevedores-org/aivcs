//! Verification and self-healing orchestration primitives.
//!
//! This module provides:
//! - failure taxonomy classification
//! - bounded auto-repair loop decisions
//! - auditable recovery artifacts with digest verification

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::{AivcsError, Result};
use oxidized_state::storage_traits::ContentDigest;

/// Coarse failure taxonomy used by the recovery planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Build,
    Test,
    Runtime,
    Integration,
    Unknown,
}

/// Structured failure signal from verification/runtime stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureSignal {
    pub stage: String,
    pub message: String,
    pub exit_code: Option<i32>,
    pub flaky_hint: bool,
}

impl FailureSignal {
    pub fn new(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            message: message.into(),
            exit_code: None,
            flaky_hint: false,
        }
    }
}

/// Recovery action selected per attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry,
    PatchForward,
    Rollback,
    Escalate,
}

/// Recovery loop final state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryOutcome {
    Recovered,
    Failed,
}

/// Bounded recovery policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    pub max_attempts: u32,
    pub max_flaky_retries: u32,
    pub allow_patch_forward: bool,
    pub allow_rollback: bool,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            max_flaky_retries: 1,
            allow_patch_forward: true,
            allow_rollback: true,
        }
    }
}

/// One auditable decision in the recovery timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryDecision {
    pub attempt: u32,
    pub failure_class: FailureClass,
    pub action: RecoveryAction,
    pub rationale: String,
}

/// Result from one repair attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAttemptResult {
    pub success: bool,
    pub next_failure: Option<FailureSignal>,
}

/// Full recovery log for artifacts/audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryLog {
    pub run_id: String,
    pub policy: RecoveryPolicy,
    pub initial_failure: FailureSignal,
    pub decisions: Vec<RecoveryDecision>,
    pub outcome: RecoveryOutcome,
    pub attempts_used: u32,
    pub final_failure: Option<FailureSignal>,
    pub evaluated_at: DateTime<Utc>,
}

/// Classify a failure into a coarse category.
pub fn classify_failure(signal: &FailureSignal) -> FailureClass {
    let stage = signal.stage.to_lowercase();
    let msg = signal.message.to_lowercase();

    if stage.contains("build")
        || stage.contains("compile")
        || msg.contains("compil")
        || msg.contains("linker error")
    {
        return FailureClass::Build;
    }
    if stage.contains("test")
        || msg.contains("assertion")
        || msg.contains("test failed")
        || msg.contains("snapshot mismatch")
    {
        return FailureClass::Test;
    }
    if stage.contains("runtime")
        || msg.contains("panic")
        || msg.contains("segmentation fault")
        || msg.contains("null pointer")
    {
        return FailureClass::Runtime;
    }
    if stage.contains("integration")
        || msg.contains("contract")
        || msg.contains("handshake")
        || msg.contains("dependency unavailable")
    {
        return FailureClass::Integration;
    }

    FailureClass::Unknown
}

fn decide_action(
    class: FailureClass,
    signal: &FailureSignal,
    policy: &RecoveryPolicy,
    flaky_retries_used: u32,
) -> (RecoveryAction, String) {
    if class == FailureClass::Test
        && signal.flaky_hint
        && flaky_retries_used < policy.max_flaky_retries
    {
        return (
            RecoveryAction::Retry,
            "flaky signal detected; bounded retry permitted".to_string(),
        );
    }

    if policy.allow_patch_forward && matches!(class, FailureClass::Build | FailureClass::Test) {
        return (
            RecoveryAction::PatchForward,
            "build/test failure; patch-forward is enabled".to_string(),
        );
    }

    if policy.allow_rollback && matches!(class, FailureClass::Runtime | FailureClass::Integration) {
        return (
            RecoveryAction::Rollback,
            "runtime/integration failure; rollback is enabled".to_string(),
        );
    }

    (
        RecoveryAction::Escalate,
        "no safe automated action available under policy".to_string(),
    )
}

/// Run a bounded, policy-controlled recovery loop.
pub fn execute_recovery_loop<F>(
    run_id: &str,
    initial_failure: FailureSignal,
    policy: RecoveryPolicy,
    mut apply_action: F,
) -> RecoveryLog
where
    F: FnMut(u32, RecoveryAction, &FailureSignal) -> RecoveryAttemptResult,
{
    let mut current = initial_failure.clone();
    let mut decisions = Vec::new();
    let mut flaky_retries_used = 0u32;
    let mut attempts_used = 0u32;

    for attempt in 1..=policy.max_attempts {
        attempts_used = attempt;
        let class = classify_failure(&current);
        let (action, rationale) = decide_action(class, &current, &policy, flaky_retries_used);
        if action == RecoveryAction::Retry && current.flaky_hint {
            flaky_retries_used += 1;
        }

        decisions.push(RecoveryDecision {
            attempt,
            failure_class: class,
            action,
            rationale,
        });

        if action == RecoveryAction::Escalate {
            return RecoveryLog {
                run_id: run_id.to_string(),
                policy,
                initial_failure,
                decisions,
                outcome: RecoveryOutcome::Failed,
                attempts_used,
                final_failure: Some(current),
                evaluated_at: Utc::now(),
            };
        }

        let attempt_result = apply_action(attempt, action, &current);
        if attempt_result.success {
            return RecoveryLog {
                run_id: run_id.to_string(),
                policy,
                initial_failure,
                decisions,
                outcome: RecoveryOutcome::Recovered,
                attempts_used,
                final_failure: None,
                evaluated_at: Utc::now(),
            };
        }

        if let Some(next) = attempt_result.next_failure {
            current = next;
        }
    }

    RecoveryLog {
        run_id: run_id.to_string(),
        policy,
        initial_failure,
        decisions,
        outcome: RecoveryOutcome::Failed,
        attempts_used,
        final_failure: Some(current),
        evaluated_at: Utc::now(),
    }
}

/// Persist `<dir>/<run_id>/recovery.json` and `<dir>/<run_id>/recovery.digest`.
pub fn write_recovery_artifact(log: &RecoveryLog, dir: &Path) -> Result<PathBuf> {
    let run_dir = dir.join(&log.run_id);
    std::fs::create_dir_all(&run_dir)?;

    let artifact_path = run_dir.join("recovery.json");
    let digest_path = run_dir.join("recovery.digest");
    let json = serde_json::to_vec_pretty(log)?;
    let digest = ContentDigest::from_bytes(&json).as_str().to_string();

    std::fs::write(&artifact_path, &json)?;
    std::fs::write(&digest_path, digest.as_bytes())?;

    Ok(artifact_path)
}

/// Regression check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionCheck {
    /// Whether this failure has been seen before.
    pub is_regression: bool,
    /// Previous recovery outcome if this is a regression.
    pub prior_outcome: Option<RecoveryOutcome>,
    /// The prior run that experienced this failure class/stage.
    pub prior_run_id: Option<String>,
    /// Recommendation based on regression analysis.
    pub recommendation: RegressionRecommendation,
}

/// Recommendation from regression analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionRecommendation {
    /// New failure, proceed with normal recovery.
    ProceedNormally,
    /// Same failure recurred after prior recovery — try a different action.
    TryAlternateAction,
    /// Same failure recurred multiple times — escalate immediately.
    EscalateImmediately,
}

/// Check whether a failure is a regression of a previously recovered issue.
///
/// Compares the current failure against a list of prior recovery logs.
/// If the same stage+class was previously recovered but has recurred,
/// recommends alternate action or escalation.
pub fn check_regression(signal: &FailureSignal, prior_logs: &[RecoveryLog]) -> RegressionCheck {
    let current_class = classify_failure(signal);

    let mut matching_logs: Vec<&RecoveryLog> = prior_logs
        .iter()
        .filter(|log| {
            let prior_class = classify_failure(&log.initial_failure);
            prior_class == current_class && log.initial_failure.stage == signal.stage
        })
        .collect();

    // Sort by most recent first
    matching_logs.sort_by(|a, b| b.evaluated_at.cmp(&a.evaluated_at));

    match matching_logs.len() {
        0 => RegressionCheck {
            is_regression: false,
            prior_outcome: None,
            prior_run_id: None,
            recommendation: RegressionRecommendation::ProceedNormally,
        },
        1 => {
            let prior = matching_logs[0];
            RegressionCheck {
                is_regression: true,
                prior_outcome: Some(prior.outcome),
                prior_run_id: Some(prior.run_id.clone()),
                recommendation: if prior.outcome == RecoveryOutcome::Recovered {
                    RegressionRecommendation::TryAlternateAction
                } else {
                    RegressionRecommendation::EscalateImmediately
                },
            }
        }
        _ => {
            let prior = matching_logs[0];
            RegressionCheck {
                is_regression: true,
                prior_outcome: Some(prior.outcome),
                prior_run_id: Some(prior.run_id.clone()),
                recommendation: RegressionRecommendation::EscalateImmediately,
            }
        }
    }
}

/// Convert a `RecoveryLog` into a format suitable for memory indexing.
///
/// Returns `(summary, tags, token_estimate)` for constructing a `MemoryEntry`.
pub fn recovery_log_to_memory_fields(log: &RecoveryLog) -> (String, Vec<String>, usize) {
    let summary = format!(
        "{} recovery for {} failure in stage '{}': {} in {} attempt(s)",
        match log.outcome {
            RecoveryOutcome::Recovered => "Successful",
            RecoveryOutcome::Failed => "Failed",
        },
        classify_failure(&log.initial_failure),
        log.initial_failure.stage,
        log.decisions
            .last()
            .map(|d| format!("{:?}", d.action))
            .unwrap_or_else(|| "none".into()),
        log.attempts_used,
    );

    let class = classify_failure(&log.initial_failure);
    let mut tags = vec![
        format!("recovery:{:?}", log.outcome).to_lowercase(),
        format!("failure:{class}").to_lowercase(),
        format!("stage:{}", log.initial_failure.stage),
        format!("run:{}", log.run_id),
    ];
    if log.initial_failure.flaky_hint {
        tags.push("flaky:true".into());
    }

    let token_estimate = (summary.len() / 4).max(1);

    (summary, tags, token_estimate)
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Build => write!(f, "build"),
            Self::Test => write!(f, "test"),
            Self::Runtime => write!(f, "runtime"),
            Self::Integration => write!(f, "integration"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Read and verify `<dir>/<run_id>/recovery.json` integrity.
pub fn read_recovery_artifact(run_id: &str, dir: &Path) -> Result<RecoveryLog> {
    let run_dir = dir.join(run_id);
    let artifact_path = run_dir.join("recovery.json");
    let digest_path = run_dir.join("recovery.digest");

    let json = std::fs::read(&artifact_path)?;
    let digest = std::fs::read_to_string(&digest_path)?;
    let actual = ContentDigest::from_bytes(&json).as_str().to_string();
    if digest.trim() != actual {
        return Err(AivcsError::DigestMismatch {
            expected: digest.trim().to_string(),
            actual,
        });
    }

    Ok(serde_json::from_slice(&json)?)
}
