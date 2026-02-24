use aivcs_core::enterprise::{
    AuditEvent, AuditLog, AuditOutcome, AuthzDecision, CostBudget, CostCharge, CostTracker,
    Permission, Principal, RbacPolicy, RedactionRule, SecretRef, SecretsPolicy, SliMeasurement,
    Slo, SloTracker, TenantId,
};
use chrono::{Duration, Utc};

fn tenant_a() -> TenantId {
    TenantId("tenant-a".to_string())
}

fn tenant_b() -> TenantId {
    TenantId("tenant-b".to_string())
}

fn role_developer() -> aivcs_core::enterprise::Role {
    aivcs_core::enterprise::Role {
        name: "developer".to_string(),
        permissions: vec![Permission::RunRead, Permission::RunWrite],
    }
}

fn role_admin() -> aivcs_core::enterprise::Role {
    aivcs_core::enterprise::Role {
        name: "admin".to_string(),
        permissions: vec![Permission::AdminFull],
    }
}

// ---- RBAC ----

#[test]
fn rbac_allows_authorized_principal() {
    let mut policy = RbacPolicy::new();
    policy.add_principal(Principal {
        id: "user-1".to_string(),
        tenant_id: tenant_a(),
        roles: vec![role_developer()],
    });

    let decision = policy.authorize("user-1", &tenant_a(), Permission::RunRead);
    assert!(decision.is_allowed());
}

#[test]
fn rbac_denies_missing_permission() {
    let mut policy = RbacPolicy::new();
    policy.add_principal(Principal {
        id: "user-1".to_string(),
        tenant_id: tenant_a(),
        roles: vec![role_developer()],
    });

    let decision = policy.authorize("user-1", &tenant_a(), Permission::AgentDeploy);
    assert!(!decision.is_allowed());
    if let AuthzDecision::Denied { reason } = decision {
        assert!(reason.contains("missing permission"));
    }
}

#[test]
fn rbac_enforces_tenant_boundary() {
    let mut policy = RbacPolicy::new();
    policy.add_principal(Principal {
        id: "user-1".to_string(),
        tenant_id: tenant_a(),
        roles: vec![role_admin()],
    });

    // Admin in tenant-a cannot access tenant-b
    let decision = policy.authorize("user-1", &tenant_b(), Permission::RunRead);
    assert!(!decision.is_allowed());
    if let AuthzDecision::Denied { reason } = decision {
        assert!(reason.contains("tenant boundary"));
    }
}

#[test]
fn rbac_admin_implies_all_permissions() {
    let mut policy = RbacPolicy::new();
    policy.add_principal(Principal {
        id: "admin-1".to_string(),
        tenant_id: tenant_a(),
        roles: vec![role_admin()],
    });

    for perm in [
        Permission::RunRead,
        Permission::RunWrite,
        Permission::AgentDeploy,
        Permission::SecretWrite,
        Permission::AuditExport,
    ] {
        assert!(
            policy.authorize("admin-1", &tenant_a(), perm).is_allowed(),
            "admin should have {:?}",
            perm
        );
    }
}

#[test]
fn rbac_denies_unknown_principal() {
    let policy = RbacPolicy::new();
    let decision = policy.authorize("ghost", &tenant_a(), Permission::RunRead);
    assert!(!decision.is_allowed());
}

// ---- Secrets Governance ----

#[test]
fn secrets_redaction_removes_sensitive_values() {
    let mut policy = SecretsPolicy::new();
    policy.add_redaction_rule(RedactionRule::env_var("API_KEY"));
    policy.add_redaction_rule(RedactionRule::bearer_token());

    let text = "Setting API_KEY=sk-secret-123 and using Bearer eyJhbGciOi for auth";
    let result = policy.redact(text);

    assert!(!result.text.contains("sk-secret-123"));
    assert!(!result.text.contains("eyJhbGciOi"));
    assert!(result.text.contains("[REDACTED]"));
    assert_eq!(result.redactions_applied, 2);
}

#[test]
fn secrets_rotation_detection() {
    let now = Utc::now();
    let mut policy = SecretsPolicy::new();
    policy.add_secret(SecretRef {
        name: "db-password".to_string(),
        provider: "vault".to_string(),
        last_rotated: Some(now - Duration::days(100)),
        rotation_interval_days: Some(90),
    });
    policy.add_secret(SecretRef {
        name: "api-key".to_string(),
        provider: "vault".to_string(),
        last_rotated: Some(now - Duration::days(10)),
        rotation_interval_days: Some(90),
    });

    let stale = policy.secrets_needing_rotation(now);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].name, "db-password");
}

// ---- Audit Log ----

#[test]
fn audit_log_records_and_queries_by_tenant() {
    let mut log = AuditLog::new();
    let now = Utc::now();

    log.record(AuditEvent {
        event_id: "e1".to_string(),
        timestamp: now,
        tenant_id: "tenant-a".to_string(),
        principal_id: "user-1".to_string(),
        action: "run.create".to_string(),
        resource: "run-123".to_string(),
        outcome: AuditOutcome::Success,
        metadata: serde_json::json!({}),
    });
    log.record(AuditEvent {
        event_id: "e2".to_string(),
        timestamp: now,
        tenant_id: "tenant-b".to_string(),
        principal_id: "user-2".to_string(),
        action: "agent.deploy".to_string(),
        resource: "agent-abc".to_string(),
        outcome: AuditOutcome::Denied,
        metadata: serde_json::json!({}),
    });

    let tenant_a_events = log.query("tenant-a", None, None);
    assert_eq!(tenant_a_events.len(), 1);
    assert_eq!(tenant_a_events[0].action, "run.create");

    let denied = log.query_by_outcome(AuditOutcome::Denied);
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].tenant_id, "tenant-b");
}

#[test]
fn audit_export_produces_valid_json() {
    let mut log = AuditLog::new();
    log.record(AuditEvent {
        event_id: "e1".to_string(),
        timestamp: Utc::now(),
        tenant_id: "tenant-a".to_string(),
        principal_id: "user-1".to_string(),
        action: "run.create".to_string(),
        resource: "run-123".to_string(),
        outcome: AuditOutcome::Success,
        metadata: serde_json::json!({}),
    });

    let json = log.export_json("tenant-a").expect("export");
    let parsed: Vec<serde_json::Value> = serde_json::from_slice(&json).expect("parse");
    assert_eq!(parsed.len(), 1);
}

#[test]
fn audit_export_write_and_verify_integrity() {
    let mut log = AuditLog::new();
    log.record(AuditEvent {
        event_id: "e1".to_string(),
        timestamp: Utc::now(),
        tenant_id: "tenant-a".to_string(),
        principal_id: "user-1".to_string(),
        action: "run.create".to_string(),
        resource: "run-123".to_string(),
        outcome: AuditOutcome::Success,
        metadata: serde_json::json!({}),
    });

    let json = log.export_json("tenant-a").expect("export");
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt =
        aivcs_core::enterprise::write_audit_export("tenant-a", &json, dir.path()).expect("write");

    assert!(receipt.path.exists());
    assert_eq!(receipt.event_count, 1);

    let verified = aivcs_core::enterprise::verify_audit_export(&receipt.path).expect("verify");
    assert!(verified);
}

// ---- SLO / Error Budget ----

#[test]
fn slo_tracker_computes_error_budget() {
    let slo = Slo {
        name: "run-success-rate".to_string(),
        target_ratio: 0.95,
        window_seconds: 3600,
    };
    let mut tracker = SloTracker::new(slo);
    let now = Utc::now();

    // 90 good, 10 bad = 90% success rate (below 95% target)
    for i in 0..90 {
        tracker.record(SliMeasurement {
            timestamp: now - Duration::seconds(3600 - i),
            good: true,
        });
    }
    for i in 0..10 {
        tracker.record(SliMeasurement {
            timestamp: now - Duration::seconds(100 - i),
            good: false,
        });
    }

    let status = tracker.status(now);
    assert_eq!(status.total_measurements, 100);
    assert_eq!(status.good_measurements, 90);
    assert!((status.current_ratio - 0.9).abs() < 0.01);
    assert!(status.budget_exhausted);
}

#[test]
fn slo_tracker_healthy_when_within_target() {
    let slo = Slo {
        name: "run-success-rate".to_string(),
        target_ratio: 0.95,
        window_seconds: 3600,
    };
    let mut tracker = SloTracker::new(slo);
    let now = Utc::now();

    // 99 good, 1 bad = 99% (above 95%)
    for i in 0..99 {
        tracker.record(SliMeasurement {
            timestamp: now - Duration::seconds(3600 - i),
            good: true,
        });
    }
    tracker.record(SliMeasurement {
        timestamp: now - Duration::seconds(1),
        good: false,
    });

    let status = tracker.status(now);
    assert!(!status.budget_exhausted);
    assert!(status.error_budget_remaining > 0.0);
}

#[test]
fn slo_empty_window_returns_healthy() {
    let slo = Slo {
        name: "test".to_string(),
        target_ratio: 0.99,
        window_seconds: 3600,
    };
    let tracker = SloTracker::new(slo);
    let status = tracker.status(Utc::now());
    assert!(!status.budget_exhausted);
    assert_eq!(status.total_measurements, 0);
}

// ---- Cost Controls ----

#[test]
fn cost_tracker_enforces_budget_limit() {
    let budget = CostBudget {
        name: "monthly-compute".to_string(),
        limit: 100.0,
        period: "monthly".to_string(),
    };
    let mut tracker = CostTracker::new(budget);

    let exceeded = tracker.charge(CostCharge {
        timestamp: Utc::now(),
        amount: 60.0,
        category: "compute".to_string(),
        description: "GPU hours".to_string(),
    });
    assert!(!exceeded);
    assert!(!tracker.is_exceeded());
    assert!((tracker.remaining() - 40.0).abs() < 0.01);

    let exceeded = tracker.charge(CostCharge {
        timestamp: Utc::now(),
        amount: 50.0,
        category: "storage".to_string(),
        description: "Artifact storage".to_string(),
    });
    assert!(exceeded);
    assert!(tracker.is_exceeded());
    assert!((tracker.remaining() - 0.0).abs() < 0.01);
}

#[test]
fn cost_tracker_reports_by_category() {
    let budget = CostBudget {
        name: "test".to_string(),
        limit: 1000.0,
        period: "monthly".to_string(),
    };
    let mut tracker = CostTracker::new(budget);

    tracker.charge(CostCharge {
        timestamp: Utc::now(),
        amount: 50.0,
        category: "compute".to_string(),
        description: "run-1".to_string(),
    });
    tracker.charge(CostCharge {
        timestamp: Utc::now(),
        amount: 30.0,
        category: "compute".to_string(),
        description: "run-2".to_string(),
    });
    tracker.charge(CostCharge {
        timestamp: Utc::now(),
        amount: 20.0,
        category: "storage".to_string(),
        description: "artifacts".to_string(),
    });

    let breakdown = tracker.by_category();
    assert!((breakdown["compute"] - 80.0).abs() < 0.01);
    assert!((breakdown["storage"] - 20.0).abs() < 0.01);
}
