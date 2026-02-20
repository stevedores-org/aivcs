//! CI repair planner with bounded retry policies.
//!
//! Given diagnostics and a policy, produces a [`RepairOutcome`] that is
//! either a bounded [`RepairPlan`] or a terminal state (exhausted, skipped, etc.).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::ci::diagnostic::Diagnostic;
use crate::domain::ci::repair::{RepairPlan, RepairStrategy};

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Policy governing automated repair attempts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepairPolicy {
    /// Maximum number of repair attempts before giving up.
    pub max_attempts: u32,

    /// Repair strategy to use.
    pub strategy: RepairStrategy,

    /// Whether to continue repairing after the first successful fix.
    pub fix_all: bool,

    /// Maximum total patches across all attempts.
    pub max_total_patches: u32,

    /// File globs that may be patched (empty = all allowed).
    pub allowed_globs: Vec<String>,

    /// File globs that must never be patched.
    pub forbidden_globs: Vec<String>,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            strategy: RepairStrategy::AutoFix,
            fix_all: true,
            max_total_patches: 10,
            allowed_globs: Vec::new(),
            forbidden_globs: vec![
                ".github/**".to_string(),
                "scripts/**".to_string(),
                "*.lock".to_string(),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// Outcome of a repair planning attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepairOutcome {
    /// A repair plan was generated.
    Planned { plan: RepairPlan },

    /// No actionable diagnostics found.
    NothingToRepair,

    /// Max attempts exceeded.
    ExhaustedAttempts { attempts: u32 },

    /// Strategy is Skip — no repair attempted.
    Skipped,
}

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

/// Create a repair plan from diagnostics and policy.
///
/// Returns the appropriate [`RepairOutcome`] based on policy constraints.
/// Actual patch generation (LLM-assisted or rule-based) is deferred to
/// a follow-up integration PR; this provides the decision logic.
pub fn plan_repair(
    run_id: Uuid,
    diagnostics: &[Diagnostic],
    policy: &RepairPolicy,
    current_attempt: u32,
) -> RepairOutcome {
    if policy.strategy == RepairStrategy::Skip {
        return RepairOutcome::Skipped;
    }

    if current_attempt >= policy.max_attempts {
        return RepairOutcome::ExhaustedAttempts {
            attempts: current_attempt,
        };
    }

    if diagnostics.is_empty() {
        return RepairOutcome::NothingToRepair;
    }

    // Placeholder: real implementation will generate patches from diagnostics
    RepairOutcome::Planned {
        plan: RepairPlan::new(run_id, policy.strategy, policy.max_attempts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ci::diagnostic::{DiagnosticSource, Severity};

    fn sample_diagnostics() -> Vec<Diagnostic> {
        vec![
            Diagnostic::new(
                Severity::Error,
                "unused variable `x`".to_string(),
                DiagnosticSource::Clippy,
            ),
            Diagnostic::new(
                Severity::Warning,
                "formatting differs".to_string(),
                DiagnosticSource::Fmt,
            ),
        ]
    }

    #[test]
    fn test_plan_repair_skipped() {
        let policy = RepairPolicy {
            strategy: RepairStrategy::Skip,
            ..Default::default()
        };
        let outcome = plan_repair(Uuid::new_v4(), &sample_diagnostics(), &policy, 0);
        assert_eq!(outcome, RepairOutcome::Skipped);
    }

    #[test]
    fn test_plan_repair_exhausted() {
        let policy = RepairPolicy {
            max_attempts: 3,
            ..Default::default()
        };
        let outcome = plan_repair(Uuid::new_v4(), &sample_diagnostics(), &policy, 3);
        assert_eq!(outcome, RepairOutcome::ExhaustedAttempts { attempts: 3 });
    }

    #[test]
    fn test_plan_repair_nothing_to_repair() {
        let policy = RepairPolicy::default();
        let outcome = plan_repair(Uuid::new_v4(), &[], &policy, 0);
        assert_eq!(outcome, RepairOutcome::NothingToRepair);
    }

    #[test]
    fn test_plan_repair_produces_plan() {
        let run_id = Uuid::new_v4();
        let policy = RepairPolicy::default();
        let outcome = plan_repair(run_id, &sample_diagnostics(), &policy, 0);

        match outcome {
            RepairOutcome::Planned { plan } => {
                assert_eq!(plan.run_id, run_id);
                assert_eq!(plan.strategy, RepairStrategy::AutoFix);
                assert_eq!(plan.max_attempts, 3);
                assert_eq!(plan.current_attempt, 0);
            }
            other => panic!("expected Planned, got {:?}", other),
        }
    }

    #[test]
    fn test_repair_policy_default() {
        let policy = RepairPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.strategy, RepairStrategy::AutoFix);
        assert!(policy.fix_all);
        assert_eq!(policy.max_total_patches, 10);
        assert!(policy.allowed_globs.is_empty());
        assert!(!policy.forbidden_globs.is_empty());
    }

    #[test]
    fn test_repair_policy_serde_roundtrip() {
        let policy = RepairPolicy::default();
        let json = serde_json::to_string(&policy).expect("serialize");
        let deserialized: RepairPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(policy, deserialized);
    }

    #[test]
    fn test_repair_outcome_serde_roundtrip() {
        let outcomes = [
            RepairOutcome::Skipped,
            RepairOutcome::NothingToRepair,
            RepairOutcome::ExhaustedAttempts { attempts: 5 },
            RepairOutcome::Planned {
                plan: RepairPlan::new(Uuid::new_v4(), RepairStrategy::Suggest, 2),
            },
        ];
        for outcome in &outcomes {
            let json = serde_json::to_string(outcome).expect("serialize");
            let deserialized: RepairOutcome = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*outcome, deserialized);
        }
    }
}
