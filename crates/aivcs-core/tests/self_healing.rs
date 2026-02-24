use aivcs_core::{
    check_regression, classify_failure, execute_recovery_loop, read_recovery_artifact,
    recovery_log_to_memory_fields, write_recovery_artifact, FailureClass, FailureSignal,
    RecoveryAction, RecoveryAttemptResult, RecoveryOutcome, RecoveryPolicy,
    RegressionRecommendation,
};
use tempfile::tempdir;

#[test]
fn failure_taxonomy_maps_common_failures() {
    let build = FailureSignal::new("build", "compilation failed: unresolved import");
    let test = FailureSignal::new("test", "assertion failed at suite");
    let runtime = FailureSignal::new("runtime", "thread panicked at src/main.rs:1");
    let integration = FailureSignal::new("integration", "contract handshake failed");
    let unknown = FailureSignal::new("misc", "unexpected condition");

    assert_eq!(classify_failure(&build), FailureClass::Build);
    assert_eq!(classify_failure(&test), FailureClass::Test);
    assert_eq!(classify_failure(&runtime), FailureClass::Runtime);
    assert_eq!(classify_failure(&integration), FailureClass::Integration);
    assert_eq!(classify_failure(&unknown), FailureClass::Unknown);
}

#[test]
fn bounded_loop_auto_remediates_common_build_failure() {
    let failure = FailureSignal::new("build", "compile error");
    let policy = RecoveryPolicy::default();

    let log = execute_recovery_loop("run-build", failure, policy, |attempt, action, _| {
        if attempt == 1 {
            assert_eq!(action, RecoveryAction::PatchForward);
            return RecoveryAttemptResult {
                success: true,
                next_failure: None,
            };
        }
        panic!("should recover on first patch-forward");
    });

    assert_eq!(log.outcome, RecoveryOutcome::Recovered);
    assert_eq!(log.attempts_used, 1);
    assert_eq!(log.decisions.len(), 1);
}

#[test]
fn flaky_retry_obeys_safety_bounds_then_escalates() {
    let mut flaky = FailureSignal::new("test", "intermittent timeout");
    flaky.flaky_hint = true;

    let policy = RecoveryPolicy {
        max_attempts: 3,
        max_flaky_retries: 1,
        allow_patch_forward: false,
        allow_rollback: false,
    };

    let log = execute_recovery_loop("run-flaky", flaky.clone(), policy, |_attempt, action, _| {
        assert_eq!(action, RecoveryAction::Retry);
        RecoveryAttemptResult {
            success: false,
            next_failure: Some(flaky.clone()),
        }
    });

    assert_eq!(log.outcome, RecoveryOutcome::Failed);
    assert_eq!(log.attempts_used, 2);
    assert_eq!(log.decisions[0].action, RecoveryAction::Retry);
    assert_eq!(log.decisions[1].action, RecoveryAction::Escalate);
}

#[test]
fn max_attempt_bound_is_enforced() {
    let failure = FailureSignal::new("build", "compile error");
    let policy = RecoveryPolicy {
        max_attempts: 2,
        max_flaky_retries: 0,
        allow_patch_forward: true,
        allow_rollback: false,
    };

    let mut calls = 0u32;
    let log = execute_recovery_loop(
        "run-bounded",
        failure.clone(),
        policy,
        |_attempt, action, _| {
            calls += 1;
            assert_eq!(action, RecoveryAction::PatchForward);
            RecoveryAttemptResult {
                success: false,
                next_failure: Some(failure.clone()),
            }
        },
    );

    assert_eq!(calls, 2);
    assert_eq!(log.outcome, RecoveryOutcome::Failed);
    assert_eq!(log.attempts_used, 2);
}

#[test]
fn recovery_artifact_is_auditable_and_digest_verified() {
    let failure = FailureSignal::new("build", "compile error");
    let log = execute_recovery_loop(
        "run-artifact",
        failure,
        RecoveryPolicy::default(),
        |_attempt, _action, _| RecoveryAttemptResult {
            success: true,
            next_failure: None,
        },
    );

    let dir = tempdir().expect("tempdir");
    let path = write_recovery_artifact(&log, dir.path()).expect("write artifact");
    assert!(path.exists());

    let loaded = read_recovery_artifact("run-artifact", dir.path()).expect("read artifact");
    assert_eq!(loaded.outcome, RecoveryOutcome::Recovered);
    assert_eq!(loaded.decisions.len(), 1);
}

// ---------------------------------------------------------------------------
// Rollback path
// ---------------------------------------------------------------------------

#[test]
fn runtime_failure_triggers_rollback() {
    let failure = FailureSignal::new("runtime", "thread panicked");
    let policy = RecoveryPolicy::default();

    let log = execute_recovery_loop("run-rollback", failure, policy, |attempt, action, _| {
        if attempt == 1 {
            assert_eq!(action, RecoveryAction::Rollback);
            return RecoveryAttemptResult {
                success: true,
                next_failure: None,
            };
        }
        panic!("should recover on rollback");
    });

    assert_eq!(log.outcome, RecoveryOutcome::Recovered);
    assert_eq!(log.decisions[0].action, RecoveryAction::Rollback);
    assert_eq!(log.decisions[0].failure_class, FailureClass::Runtime);
}

#[test]
fn integration_failure_triggers_rollback() {
    let failure = FailureSignal::new("integration", "dependency unavailable");
    let policy = RecoveryPolicy::default();

    let log = execute_recovery_loop("run-int-rb", failure, policy, |attempt, action, _| {
        if attempt == 1 {
            assert_eq!(action, RecoveryAction::Rollback);
            return RecoveryAttemptResult {
                success: true,
                next_failure: None,
            };
        }
        panic!("should recover on rollback");
    });

    assert_eq!(log.outcome, RecoveryOutcome::Recovered);
    assert_eq!(log.decisions[0].failure_class, FailureClass::Integration);
}

// ---------------------------------------------------------------------------
// Unknown failure escalation
// ---------------------------------------------------------------------------

#[test]
fn unknown_failure_escalates_immediately() {
    let failure = FailureSignal::new("misc", "something went wrong");
    let policy = RecoveryPolicy {
        max_attempts: 3,
        max_flaky_retries: 1,
        allow_patch_forward: false,
        allow_rollback: false,
    };

    let log = execute_recovery_loop("run-unknown", failure, policy, |_, _, _| {
        panic!("should not attempt action on escalate");
    });

    assert_eq!(log.outcome, RecoveryOutcome::Failed);
    assert_eq!(log.attempts_used, 1);
    assert_eq!(log.decisions[0].action, RecoveryAction::Escalate);
}

// ---------------------------------------------------------------------------
// Regression detection
// ---------------------------------------------------------------------------

#[test]
fn no_prior_logs_means_no_regression() {
    let signal = FailureSignal::new("build", "compile error");
    let check = check_regression(&signal, &[]);

    assert!(!check.is_regression);
    assert_eq!(
        check.recommendation,
        RegressionRecommendation::ProceedNormally
    );
}

#[test]
fn single_prior_recovered_recommends_alternate() {
    let failure = FailureSignal::new("build", "compile error");
    let prior = execute_recovery_loop(
        "run-prior",
        failure.clone(),
        RecoveryPolicy::default(),
        |_, _, _| RecoveryAttemptResult {
            success: true,
            next_failure: None,
        },
    );

    let check = check_regression(&failure, &[prior]);
    assert!(check.is_regression);
    assert_eq!(check.prior_outcome, Some(RecoveryOutcome::Recovered));
    assert_eq!(
        check.recommendation,
        RegressionRecommendation::TryAlternateAction
    );
}

#[test]
fn prior_failure_recommends_escalation() {
    let failure = FailureSignal::new("build", "compile error");
    let policy = RecoveryPolicy {
        max_attempts: 1,
        max_flaky_retries: 0,
        allow_patch_forward: true,
        allow_rollback: false,
    };
    let prior = execute_recovery_loop("run-prior-fail", failure.clone(), policy, |_, _, _| {
        RecoveryAttemptResult {
            success: false,
            next_failure: Some(failure.clone()),
        }
    });

    let check = check_regression(&failure, &[prior]);
    assert!(check.is_regression);
    assert_eq!(
        check.recommendation,
        RegressionRecommendation::EscalateImmediately
    );
}

#[test]
fn multiple_priors_escalate_immediately() {
    let failure = FailureSignal::new("test", "assertion failed");
    let mk_log = |id: &str| {
        execute_recovery_loop(id, failure.clone(), RecoveryPolicy::default(), |_, _, _| {
            RecoveryAttemptResult {
                success: true,
                next_failure: None,
            }
        })
    };

    let check = check_regression(&failure, &[mk_log("r1"), mk_log("r2")]);
    assert!(check.is_regression);
    assert_eq!(
        check.recommendation,
        RegressionRecommendation::EscalateImmediately
    );
}

#[test]
fn different_stage_is_not_regression() {
    let build_fail = FailureSignal::new("build", "compile error");
    let test_fail = FailureSignal::new("test", "assertion failed");
    let prior = execute_recovery_loop(
        "run-build",
        build_fail,
        RecoveryPolicy::default(),
        |_, _, _| RecoveryAttemptResult {
            success: true,
            next_failure: None,
        },
    );

    let check = check_regression(&test_fail, &[prior]);
    assert!(!check.is_regression);
}

// ---------------------------------------------------------------------------
// Recovery-to-memory bridge
// ---------------------------------------------------------------------------

#[test]
fn recovery_log_to_memory_fields_success() {
    let failure = FailureSignal::new("build", "compile error");
    let log = execute_recovery_loop("run-mem", failure, RecoveryPolicy::default(), |_, _, _| {
        RecoveryAttemptResult {
            success: true,
            next_failure: None,
        }
    });

    let (summary, tags, tokens) = recovery_log_to_memory_fields(&log);
    assert!(summary.contains("Successful"));
    assert!(summary.contains("build"));
    assert!(tags.contains(&"recovery:recovered".to_string()));
    assert!(tags.contains(&"failure:build".to_string()));
    assert!(tags.contains(&"stage:build".to_string()));
    assert!(tags.contains(&"run:run-mem".to_string()));
    assert!(tokens > 0);
}

#[test]
fn recovery_log_to_memory_fields_failed_with_flaky() {
    let mut failure = FailureSignal::new("test", "intermittent timeout");
    failure.flaky_hint = true;
    let log = execute_recovery_loop(
        "run-flaky-mem",
        failure.clone(),
        RecoveryPolicy {
            max_attempts: 1,
            max_flaky_retries: 1,
            allow_patch_forward: false,
            allow_rollback: false,
        },
        |_, _, _| RecoveryAttemptResult {
            success: false,
            next_failure: Some(failure.clone()),
        },
    );

    let (summary, tags, _) = recovery_log_to_memory_fields(&log);
    assert!(summary.contains("Failed") || summary.contains("test"));
    assert!(tags.contains(&"flaky:true".to_string()));
}

// ---------------------------------------------------------------------------
// Serde roundtrips
// ---------------------------------------------------------------------------

#[test]
fn recovery_log_serde_roundtrip() {
    let failure = FailureSignal::new("build", "compile error");
    let log = execute_recovery_loop(
        "run-serde",
        failure,
        RecoveryPolicy::default(),
        |_, _, _| RecoveryAttemptResult {
            success: true,
            next_failure: None,
        },
    );

    let json = serde_json::to_string_pretty(&log).unwrap();
    let back: aivcs_core::RecoveryLog = serde_json::from_str(&json).unwrap();
    assert_eq!(log.outcome, back.outcome);
    assert_eq!(log.decisions.len(), back.decisions.len());
}

#[test]
fn regression_check_serde_roundtrip() {
    let check = aivcs_core::RegressionCheck {
        is_regression: true,
        prior_outcome: Some(RecoveryOutcome::Recovered),
        prior_run_id: Some("run-1".into()),
        recommendation: RegressionRecommendation::TryAlternateAction,
    };
    let json = serde_json::to_string(&check).unwrap();
    let back: aivcs_core::RegressionCheck = serde_json::from_str(&json).unwrap();
    assert_eq!(check, back);
}
