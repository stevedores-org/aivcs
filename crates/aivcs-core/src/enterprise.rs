//! Enterprise readiness primitives for EPIC10.
//!
//! Provides RBAC with tenant isolation, secrets governance, compliance-grade
//! audit export, and SLO/error-budget tracking.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::Result;
use oxidized_state::storage_traits::ContentDigest;

// ---------------------------------------------------------------------------
// RBAC — Role-Based Access Control with Tenant Isolation
// ---------------------------------------------------------------------------

/// Tenant identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

/// Principal (user or service) within a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub tenant_id: TenantId,
    pub roles: Vec<Role>,
}

/// Role with associated permissions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub permissions: Vec<Permission>,
}

/// Fine-grained permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    RunRead,
    RunWrite,
    RunDelete,
    AgentDeploy,
    AgentPromote,
    AgentRollback,
    SecretRead,
    SecretWrite,
    AuditExport,
    AdminFull,
}

impl Permission {
    /// Admin implies all permissions.
    fn is_admin(self) -> bool {
        matches!(self, Self::AdminFull)
    }
}

/// RBAC policy engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RbacPolicy {
    principals: Vec<Principal>,
}

impl RbacPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_principal(&mut self, principal: Principal) {
        self.principals.push(principal);
    }

    /// Check if a principal has a specific permission.
    pub fn authorize(
        &self,
        principal_id: &str,
        tenant_id: &TenantId,
        permission: Permission,
    ) -> AuthzDecision {
        let principal = self.principals.iter().find(|p| p.id == principal_id);

        let Some(principal) = principal else {
            return AuthzDecision::Denied {
                reason: "principal not found".to_string(),
            };
        };

        // Tenant boundary enforcement
        if &principal.tenant_id != tenant_id {
            return AuthzDecision::Denied {
                reason: "tenant boundary violation".to_string(),
            };
        }

        let has_permission = principal.roles.iter().any(|role| {
            role.permissions
                .iter()
                .any(|p| *p == permission || p.is_admin())
        });

        if has_permission {
            AuthzDecision::Allowed
        } else {
            AuthzDecision::Denied {
                reason: format!("missing permission: {:?}", permission),
            }
        }
    }

    /// List all principals for a tenant.
    pub fn principals_for_tenant(&self, tenant_id: &TenantId) -> Vec<&Principal> {
        self.principals
            .iter()
            .filter(|p| &p.tenant_id == tenant_id)
            .collect()
    }
}

/// Authorization decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthzDecision {
    Allowed,
    Denied { reason: String },
}

impl AuthzDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

// ---------------------------------------------------------------------------
// Secrets Governance — Redaction and Rotation Tracking
// ---------------------------------------------------------------------------

/// Secret reference (never stores the actual value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    pub name: String,
    pub provider: String,
    pub last_rotated: Option<DateTime<Utc>>,
    pub rotation_interval_days: Option<u64>,
}

/// Redaction pattern for preventing secret leakage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionRule {
    pub pattern_name: String,
    pub regex_pattern: String,
    pub replacement: String,
}

impl RedactionRule {
    pub fn env_var(name: &str) -> Self {
        Self {
            pattern_name: name.to_string(),
            regex_pattern: format!(r"(?i){}=\S+", regex::escape(name)),
            replacement: format!("{}=[REDACTED]", name),
        }
    }

    pub fn bearer_token() -> Self {
        Self {
            pattern_name: "bearer_token".to_string(),
            regex_pattern: r"(?i)bearer\s+[a-zA-Z0-9\-._~+/]+=*".to_string(),
            replacement: "Bearer [REDACTED]".to_string(),
        }
    }
}

/// Secrets governance policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretsPolicy {
    pub secrets: Vec<SecretRef>,
    pub redaction_rules: Vec<RedactionRule>,
}

impl SecretsPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_secret(&mut self, secret: SecretRef) {
        self.secrets.push(secret);
    }

    pub fn add_redaction_rule(&mut self, rule: RedactionRule) {
        self.redaction_rules.push(rule);
    }

    /// Check for secrets needing rotation.
    pub fn secrets_needing_rotation(&self, now: DateTime<Utc>) -> Vec<&SecretRef> {
        self.secrets
            .iter()
            .filter(|s| {
                if let (Some(last_rotated), Some(interval)) =
                    (s.last_rotated, s.rotation_interval_days)
                {
                    let age_days = (now - last_rotated).num_days();
                    age_days >= interval as i64
                } else {
                    false
                }
            })
            .collect()
    }

    /// Apply redaction rules to a string, returning (redacted_text, redaction_count).
    pub fn redact(&self, text: &str) -> RedactionResult {
        let mut result = text.to_string();
        let mut count = 0;
        let mut applied_rules = Vec::new();

        for rule in &self.redaction_rules {
            if let Ok(re) = regex::Regex::new(&rule.regex_pattern) {
                let matches = re.find_iter(&result).count();
                if matches > 0 {
                    result = re
                        .replace_all(&result, rule.replacement.as_str())
                        .to_string();
                    count += matches;
                    applied_rules.push(rule.pattern_name.clone());
                }
            }
        }

        RedactionResult {
            text: result,
            redactions_applied: count,
            rules_matched: applied_rules,
        }
    }
}

/// Result of applying redaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionResult {
    pub text: String,
    pub redactions_applied: usize,
    pub rules_matched: Vec<String>,
}

// ---------------------------------------------------------------------------
// Compliance Audit Export
// ---------------------------------------------------------------------------

/// Audit event for compliance records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub tenant_id: String,
    pub principal_id: String,
    pub action: String,
    pub resource: String,
    pub outcome: AuditOutcome,
    pub metadata: serde_json::Value,
}

/// Outcome of an audited action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Denied,
    Error,
}

/// Audit log with retention and export capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog {
    events: Vec<AuditEvent>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, event: AuditEvent) {
        self.events.push(event);
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Query events by tenant within a time range.
    pub fn query(
        &self,
        tenant_id: &str,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| {
                e.tenant_id == tenant_id
                    && from.is_none_or(|f| e.timestamp >= f)
                    && to.is_none_or(|t| e.timestamp <= t)
            })
            .collect()
    }

    /// Query events by outcome (e.g., all denied actions for security review).
    pub fn query_by_outcome(&self, outcome: AuditOutcome) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| e.outcome == outcome)
            .collect()
    }

    /// Export audit events as compliance-ready JSON.
    pub fn export_json(&self, tenant_id: &str) -> Result<Vec<u8>> {
        let tenant_events: Vec<&AuditEvent> = self
            .events
            .iter()
            .filter(|e| e.tenant_id == tenant_id)
            .collect();
        let json = serde_json::to_vec_pretty(&tenant_events)?;
        Ok(json)
    }
}

/// Write a compliance audit export with digest for tamper detection.
pub fn write_audit_export(
    tenant_id: &str,
    events: &[u8],
    dir: &Path,
) -> Result<AuditExportReceipt> {
    let export_dir = dir.join(tenant_id);
    std::fs::create_dir_all(&export_dir)?;

    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let filename = format!("audit-export-{}.json", timestamp);
    let path = export_dir.join(&filename);
    let digest_path = export_dir.join(format!("{}.digest", filename));

    let digest = ContentDigest::from_bytes(events).as_str().to_string();
    std::fs::write(&path, events)?;
    std::fs::write(&digest_path, digest.as_bytes())?;

    Ok(AuditExportReceipt {
        path,
        digest,
        event_count: serde_json::from_slice::<Vec<serde_json::Value>>(events)
            .map(|v| v.len())
            .unwrap_or(0),
        exported_at: Utc::now(),
    })
}

/// Verify an audit export's integrity.
pub fn verify_audit_export(path: &Path) -> Result<bool> {
    let digest_path = path.with_extension(format!(
        "{}.digest",
        path.extension().unwrap_or_default().to_str().unwrap_or("")
    ));
    if !digest_path.exists() {
        return Ok(false);
    }
    let data = std::fs::read(path)?;
    let expected = std::fs::read_to_string(&digest_path)?;
    let actual = ContentDigest::from_bytes(&data).as_str().to_string();
    Ok(expected.trim() == actual)
}

/// Receipt for a completed audit export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditExportReceipt {
    pub path: PathBuf,
    pub digest: String,
    pub event_count: usize,
    pub exported_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// SLO / Error Budget Tracking
// ---------------------------------------------------------------------------

/// Service Level Objective definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slo {
    pub name: String,
    pub target_ratio: f64,
    pub window_seconds: u64,
}

/// A single SLI measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliMeasurement {
    pub timestamp: DateTime<Utc>,
    pub good: bool,
}

/// SLO tracker with error budget computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloTracker {
    pub slo: Slo,
    measurements: Vec<SliMeasurement>,
}

impl SloTracker {
    pub fn new(slo: Slo) -> Self {
        Self {
            slo,
            measurements: Vec::new(),
        }
    }

    pub fn record(&mut self, measurement: SliMeasurement) {
        self.measurements.push(measurement);
    }

    /// Compute the current SLO status within the configured window.
    pub fn status(&self, now: DateTime<Utc>) -> SloStatus {
        let window_start = now - chrono::Duration::seconds(self.slo.window_seconds as i64);
        let in_window: Vec<&SliMeasurement> = self
            .measurements
            .iter()
            .filter(|m| m.timestamp >= window_start)
            .collect();

        let total = in_window.len();
        if total == 0 {
            return SloStatus {
                slo_name: self.slo.name.clone(),
                current_ratio: 1.0,
                target_ratio: self.slo.target_ratio,
                error_budget_remaining: 1.0,
                total_measurements: 0,
                good_measurements: 0,
                budget_exhausted: false,
            };
        }

        let good = in_window.iter().filter(|m| m.good).count();
        let current_ratio = good as f64 / total as f64;
        let max_bad = ((1.0 - self.slo.target_ratio) * total as f64).floor() as usize;
        let actual_bad = total - good;
        let budget_remaining = if max_bad == 0 {
            if actual_bad == 0 {
                1.0
            } else {
                0.0
            }
        } else {
            1.0 - (actual_bad as f64 / max_bad as f64)
        };

        SloStatus {
            slo_name: self.slo.name.clone(),
            current_ratio,
            target_ratio: self.slo.target_ratio,
            error_budget_remaining: budget_remaining.max(0.0),
            total_measurements: total,
            good_measurements: good,
            budget_exhausted: budget_remaining <= 0.0,
        }
    }
}

/// Current SLO status and error budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SloStatus {
    pub slo_name: String,
    pub current_ratio: f64,
    pub target_ratio: f64,
    pub error_budget_remaining: f64,
    pub total_measurements: usize,
    pub good_measurements: usize,
    pub budget_exhausted: bool,
}

// ---------------------------------------------------------------------------
// Cost Controls
// ---------------------------------------------------------------------------

/// Cost budget for a tenant or workload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostBudget {
    pub name: String,
    pub limit: f64,
    pub period: String,
}

/// A cost charge event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostCharge {
    pub timestamp: DateTime<Utc>,
    pub amount: f64,
    pub category: String,
    pub description: String,
}

/// Cost tracker with budget enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostTracker {
    pub budget: CostBudget,
    charges: Vec<CostCharge>,
}

impl CostTracker {
    pub fn new(budget: CostBudget) -> Self {
        Self {
            budget,
            charges: Vec::new(),
        }
    }

    /// Record a charge. Returns whether the budget is now exceeded.
    pub fn charge(&mut self, charge: CostCharge) -> bool {
        self.charges.push(charge);
        self.total_spent() > self.budget.limit
    }

    pub fn total_spent(&self) -> f64 {
        self.charges.iter().map(|c| c.amount).sum()
    }

    pub fn remaining(&self) -> f64 {
        (self.budget.limit - self.total_spent()).max(0.0)
    }

    pub fn is_exceeded(&self) -> bool {
        self.total_spent() > self.budget.limit
    }

    /// Breakdown by category.
    pub fn by_category(&self) -> HashMap<String, f64> {
        let mut map = HashMap::new();
        for c in &self.charges {
            *map.entry(c.category.clone()).or_insert(0.0) += c.amount;
        }
        map
    }
}
