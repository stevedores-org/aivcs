//! CI gate rules engine for promotion policies.
//!
//! Evaluates a [`CIResult`] against a [`CIGateRuleSet`] to produce a
//! [`CIGateVerdict`] — the pass/fail decision that blocks or allows
//! promotion of a CI run. Supports stage-pass checks, duration limits,
//! diagnostics thresholds, and fail-fast.

use serde::{Deserialize, Serialize};

use crate::domain::ci::result::{CIResult, CIStatus};

// ---------------------------------------------------------------------------
// Gate rules
// ---------------------------------------------------------------------------

/// A single CI gate rule that can block promotion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CIGateRule {
    /// All stages must pass.
    AllStagesPass,
    /// A specific named stage must pass.
    RequireStage { stage: String },
    /// Total run duration must not exceed a threshold (milliseconds).
    MaxDuration { max_ms: u64 },
    /// Total diagnostics count must not exceed a threshold.
    MaxDiagnostics { max_count: u32 },
}

/// A set of CI gate rules with a fail-fast option.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CIGateRuleSet {
    pub rules: Vec<CIGateRule>,
    pub fail_fast: bool,
}

impl CIGateRuleSet {
    /// Create a standard rule set: all stages must pass.
    pub fn standard() -> Self {
        Self {
            rules: vec![CIGateRule::AllStagesPass],
            fail_fast: false,
        }
    }

    /// Add a rule.
    pub fn with_rule(mut self, rule: CIGateRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Set fail-fast mode.
    pub fn with_fail_fast(mut self, fail_fast: bool) -> Self {
        self.fail_fast = fail_fast;
        self
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// A single CI gate rule violation.
#[derive(Debug, Clone, PartialEq)]
pub struct CIGateViolation {
    /// Which rule was violated.
    pub rule: CIGateRule,
    /// Human-readable explanation.
    pub reason: String,
}

/// The outcome of evaluating a CI gate rule set against a CI result.
#[derive(Debug, Clone, PartialEq)]
pub struct CIGateVerdict {
    /// Whether the gate passed (no violations).
    pub passed: bool,
    /// Violations found (empty when passed).
    pub violations: Vec<CIGateViolation>,
}

impl CIGateVerdict {
    fn pass() -> Self {
        Self {
            passed: true,
            violations: Vec::new(),
        }
    }

    fn fail(violations: Vec<CIGateViolation>) -> Self {
        Self {
            passed: false,
            violations,
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Evaluate a [`CIResult`] against a [`CIGateRuleSet`], returning a [`CIGateVerdict`].
///
/// When `fail_fast` is true, evaluation stops at the first violation.
pub fn evaluate_ci_gate(rule_set: &CIGateRuleSet, result: &CIResult) -> CIGateVerdict {
    let mut violations = Vec::new();

    for rule in &rule_set.rules {
        if let Some(v) = check_rule(rule, result) {
            violations.push(v);
            if rule_set.fail_fast {
                return CIGateVerdict::fail(violations);
            }
        }
    }

    if violations.is_empty() {
        CIGateVerdict::pass()
    } else {
        CIGateVerdict::fail(violations)
    }
}

fn check_rule(rule: &CIGateRule, result: &CIResult) -> Option<CIGateViolation> {
    match rule {
        CIGateRule::AllStagesPass => {
            let failed: Vec<&str> = result
                .stages
                .iter()
                .filter(|s| s.status == CIStatus::Failed)
                .map(|s| s.stage.as_str())
                .collect();

            if failed.is_empty() {
                None
            } else {
                Some(CIGateViolation {
                    rule: rule.clone(),
                    reason: format!("{} stage(s) failed: [{}]", failed.len(), failed.join(", ")),
                })
            }
        }
        CIGateRule::RequireStage { stage } => {
            let found = result.stages.iter().find(|s| &s.stage == stage);
            match found {
                Some(s) if s.status == CIStatus::Passed => None,
                Some(s) => Some(CIGateViolation {
                    rule: rule.clone(),
                    reason: format!("required stage '{}' has status {:?}", stage, s.status),
                }),
                None => Some(CIGateViolation {
                    rule: rule.clone(),
                    reason: format!("required stage '{}' not found in results", stage),
                }),
            }
        }
        CIGateRule::MaxDuration { max_ms } => {
            if result.total_duration_ms > *max_ms {
                Some(CIGateViolation {
                    rule: rule.clone(),
                    reason: format!(
                        "total duration {}ms > max allowed {}ms",
                        result.total_duration_ms, max_ms,
                    ),
                })
            } else {
                None
            }
        }
        CIGateRule::MaxDiagnostics { max_count } => {
            let total: u32 = result.stages.iter().map(|s| s.diagnostics_count).sum();
            if total > *max_count {
                Some(CIGateViolation {
                    rule: rule.clone(),
                    reason: format!("total diagnostics {} > max allowed {}", total, max_count,),
                })
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ci::result::{CIResult, CIStageResult, CIStatus};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_passing_result() -> CIResult {
        let run_id = Uuid::new_v4();
        let stages = vec![
            CIStageResult::new("fmt".into(), "cargo fmt".into(), CIStatus::Passed, 100),
            CIStageResult::new(
                "clippy".into(),
                "cargo clippy".into(),
                CIStatus::Passed,
                500,
            ),
            CIStageResult::new("test".into(), "cargo test".into(), CIStatus::Passed, 2000),
        ];
        CIResult::new(run_id, stages, Utc::now())
    }

    fn make_failing_result() -> CIResult {
        let run_id = Uuid::new_v4();
        let stages = vec![
            CIStageResult::new("fmt".into(), "cargo fmt".into(), CIStatus::Passed, 100),
            CIStageResult::new(
                "clippy".into(),
                "cargo clippy".into(),
                CIStatus::Failed,
                500,
            ),
            CIStageResult::new("test".into(), "cargo test".into(), CIStatus::Passed, 2000),
        ];
        CIResult::new(run_id, stages, Utc::now())
    }

    #[test]
    fn test_all_stages_pass_passes() {
        let rules = CIGateRuleSet::standard();
        let result = make_passing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(verdict.passed);
        assert!(verdict.violations.is_empty());
    }

    #[test]
    fn test_all_stages_pass_fails() {
        let rules = CIGateRuleSet::standard();
        let result = make_failing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(!verdict.passed);
        assert_eq!(verdict.violations.len(), 1);
        assert!(verdict.violations[0].reason.contains("clippy"));
    }

    #[test]
    fn test_require_stage_passes() {
        let rules = CIGateRuleSet {
            rules: vec![CIGateRule::RequireStage {
                stage: "test".to_string(),
            }],
            fail_fast: false,
        };
        let result = make_passing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(verdict.passed);
    }

    #[test]
    fn test_require_stage_fails_when_missing() {
        let rules = CIGateRuleSet {
            rules: vec![CIGateRule::RequireStage {
                stage: "audit".to_string(),
            }],
            fail_fast: false,
        };
        let result = make_passing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(!verdict.passed);
        assert!(verdict.violations[0].reason.contains("not found"));
    }

    #[test]
    fn test_max_duration_passes() {
        let rules = CIGateRuleSet {
            rules: vec![CIGateRule::MaxDuration { max_ms: 10000 }],
            fail_fast: false,
        };
        let result = make_passing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(verdict.passed);
    }

    #[test]
    fn test_max_duration_fails() {
        let rules = CIGateRuleSet {
            rules: vec![CIGateRule::MaxDuration { max_ms: 100 }],
            fail_fast: false,
        };
        let result = make_passing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(!verdict.passed);
        assert!(verdict.violations[0].reason.contains("duration"));
    }

    #[test]
    fn test_max_diagnostics_passes() {
        let rules = CIGateRuleSet {
            rules: vec![CIGateRule::MaxDiagnostics { max_count: 100 }],
            fail_fast: false,
        };
        let result = make_passing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(verdict.passed);
    }

    #[test]
    fn test_fail_fast_stops_early() {
        let rules = CIGateRuleSet {
            rules: vec![
                CIGateRule::AllStagesPass,
                CIGateRule::MaxDuration { max_ms: 1 },
            ],
            fail_fast: true,
        };
        let result = make_failing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(!verdict.passed);
        assert_eq!(
            verdict.violations.len(),
            1,
            "fail_fast should stop at first violation"
        );
    }

    #[test]
    fn test_multiple_violations_without_fail_fast() {
        let rules = CIGateRuleSet {
            rules: vec![
                CIGateRule::AllStagesPass,
                CIGateRule::MaxDuration { max_ms: 1 },
            ],
            fail_fast: false,
        };
        let result = make_failing_result();
        let verdict = evaluate_ci_gate(&rules, &result);

        assert!(!verdict.passed);
        assert_eq!(verdict.violations.len(), 2);
    }

    #[test]
    fn test_ci_gate_rule_serde() {
        let rules = [
            CIGateRule::AllStagesPass,
            CIGateRule::RequireStage {
                stage: "test".to_string(),
            },
            CIGateRule::MaxDuration { max_ms: 5000 },
            CIGateRule::MaxDiagnostics { max_count: 10 },
        ];
        for rule in &rules {
            let json = serde_json::to_string(rule).expect("serialize");
            let deserialized: CIGateRule = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*rule, deserialized);
        }
    }
}
