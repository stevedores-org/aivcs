//! CI domain schema definitions
//!
//! All domain objects are content-addressable (SHA256) and follow AIVCS patterns.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

/// Module for serializing chrono DateTime to SurrealDB format
pub mod surreal_datetime {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};
    use surrealdb::sql::Datetime as SurrealDatetime;

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let sd = SurrealDatetime::from(*date);
        serde::Serialize::serialize(&sd, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sd = SurrealDatetime::deserialize(deserializer)?;
        Ok(DateTime::from(sd))
    }
}

/// Module for serializing optional chrono DateTime to SurrealDB format
pub mod surreal_datetime_opt {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};
    use surrealdb::sql::Datetime as SurrealDatetime;

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(d) => {
                let sd = SurrealDatetime::from(*d);
                serde::Serialize::serialize(&sd, serializer)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<SurrealDatetime>::deserialize(deserializer)?;
        Ok(opt.map(|sd| DateTime::from(sd)))
    }
}

// ============================================================================
// 1. CI SNAPSHOT - repo state + environment
// ============================================================================

/// A CI snapshot captures the repo and environment at the start of a run.
///
/// Identity: SHA256(repo_sha + workspace_hash + config_hash + env_hash)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CISnapshot {
    /// Content-addressed digest (SHA256)
    pub digest: String,

    /// Repository reference (commit SHA)
    pub repo_sha: String,

    /// Workspace content hash (includes uncommitted changes)
    pub workspace_hash: String,

    /// .local-ci.toml config hash (if present)
    pub config_hash: Option<String>,

    /// Toolchain/environment hash (Nix flake metadata)
    pub env_hash: Option<String>,

    /// When this snapshot was recorded
    #[serde(with = "surreal_datetime")]
    pub recorded_at: DateTime<Utc>,
}

impl CISnapshot {
    /// Create a new snapshot and compute its digest
    pub fn new(
        repo_sha: String,
        workspace_hash: String,
        config_hash: Option<String>,
        env_hash: Option<String>,
    ) -> Self {
        let digest = compute_snapshot_digest(&repo_sha, &workspace_hash, &config_hash, &env_hash);
        CISnapshot {
            digest,
            repo_sha,
            workspace_hash,
            config_hash,
            env_hash,
            recorded_at: Utc::now(),
        }
    }

    /// Short digest (first 12 chars)
    pub fn short_digest(&self) -> &str {
        &self.digest[..12.min(self.digest.len())]
    }
}

// ============================================================================
// 2. CI RUN SPECIFICATION - what to run
// ============================================================================

/// Specification for a CI run (stages, budgets, runner config)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRunSpec {
    /// Ordered list of stages to run (e.g., ["fmt", "clippy", "test"])
    pub stages: Vec<String>,

    /// Timeout per stage (milliseconds)
    pub stage_timeout_ms: Option<u64>,

    /// Total run timeout (milliseconds)
    pub total_timeout_ms: Option<u64>,

    /// Maximum retry attempts per stage
    pub max_attempts: Option<u32>,

    /// Fail fast on first failure
    pub fail_fast: bool,

    /// Local-ci runner version
    pub runner_version: String,

    /// Extra options (cache, fix-mode, etc.)
    pub options: HashMap<String, String>,
}

impl CIRunSpec {
    /// Create a minimal run spec (common default: fmt, clippy, test)
    pub fn default_stages(runner_version: String) -> Self {
        CIRunSpec {
            stages: vec!["fmt".to_string(), "clippy".to_string(), "test".to_string()],
            stage_timeout_ms: Some(300_000),        // 5 min per stage
            total_timeout_ms: Some(1_800_000),      // 30 min total
            max_attempts: Some(2),
            fail_fast: false,
            runner_version,
            options: HashMap::new(),
        }
    }
}

// ============================================================================
// 3. RUN STATUS AND RESULT
// ============================================================================

/// Lifecycle status of a CI run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Queued => write!(f, "queued"),
            RunStatus::Running => write!(f, "running"),
            RunStatus::Succeeded => write!(f, "succeeded"),
            RunStatus::Failed => write!(f, "failed"),
            RunStatus::Canceled => write!(f, "canceled"),
        }
    }
}

/// Result of a CI run (outputs and artifacts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIResult {
    /// Digest of local-ci JSON output
    pub local_ci_json_digest: Option<String>,

    /// Digest of stdout log
    pub stdout_digest: Option<String>,

    /// Digest of stderr log
    pub stderr_digest: Option<String>,

    /// Additional artifact digests (e.g., JUnit, coverage reports)
    pub artifact_digests: HashMap<String, String>,

    /// Exit code from runner
    pub exit_code: i32,

    /// Duration in milliseconds
    pub duration_ms: u64,

    /// Cache hit stats (optional)
    pub cache_stats: Option<CacheStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: u32,
    pub misses: u32,
}

/// A CI run - the core execution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRun {
    /// Unique run ID
    pub id: String,

    /// Reference to the snapshot that was run
    pub snapshot_id: String,

    /// Specification used for this run
    pub spec_digest: String,

    /// Policy digest (enforcement rules)
    pub policy_digest: String,

    /// Current status
    pub status: RunStatus,

    /// Results (populated when status != Running/Queued)
    pub result: Option<CIResult>,

    /// Diagnostics produced from this run
    pub diagnostics: Vec<Diagnostic>,

    /// Timestamps
    #[serde(with = "surreal_datetime")]
    pub started_at: DateTime<Utc>,

    #[serde(with = "surreal_datetime_opt")]
    pub finished_at: Option<DateTime<Utc>>,

    /// Runner version used
    pub runner_version: String,

    /// Optional repair plan (if failures were diagnosed)
    pub repair_plan: Option<RepairPlan>,

    /// Verification run link (if patch was applied)
    pub verification_link: Option<VerificationLink>,
}

impl CIRun {
    /// Create a new run
    pub fn new(
        snapshot_id: String,
        spec_digest: String,
        policy_digest: String,
        runner_version: String,
    ) -> Self {
        CIRun {
            id: Uuid::new_v4().to_string(),
            snapshot_id,
            spec_digest,
            policy_digest,
            status: RunStatus::Queued,
            result: None,
            diagnostics: vec![],
            started_at: Utc::now(),
            finished_at: None,
            runner_version,
            repair_plan: None,
            verification_link: None,
        }
    }

    /// Transition to running
    pub fn start(&mut self) {
        self.status = RunStatus::Running;
        self.started_at = Utc::now();
    }

    /// Finish the run with a result
    pub fn finish(&mut self, status: RunStatus, result: CIResult) {
        self.status = status;
        self.result = Some(result);
        self.finished_at = Some(Utc::now());
    }

    /// Add a diagnostic
    pub fn add_diagnostic(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }
}

// ============================================================================
// 4. DIAGNOSTICS - normalized failure information
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticKind {
    /// Format violation (rustfmt)
    Format,
    /// Linting issue (clippy)
    Lint,
    /// Unit test failure
    UnitTest,
    /// Compilation error
    Compilation,
    /// Other issue
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// Normalized diagnostic from CI run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Kind of issue
    pub kind: DiagnosticKind,

    /// Stage that produced this (e.g., "fmt", "clippy", "test")
    pub stage: String,

    /// Severity
    pub severity: DiagnosticSeverity,

    /// Human-readable message
    pub message: String,

    /// File path (if applicable)
    pub file: Option<String>,

    /// Line number (if applicable)
    pub line: Option<u32>,

    /// Rule name (e.g., "E0425" for clippy)
    pub rule: Option<String>,

    /// Evidence/context snippet
    pub evidence: Option<String>,

    /// Command that was run
    pub command: Option<String>,

    /// Exit code (if available)
    pub exit_code: Option<i32>,

    /// Confidence (0-100) that fix is safe
    pub fix_confidence: Option<u8>,
}

// ============================================================================
// 5. REPAIR POLICY AND PLAN
// ============================================================================

/// Policy that governs repair actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPolicy {
    /// Is repair allowed at all?
    pub allow_repair: bool,

    /// Is automatic patching allowed?
    pub allow_patch: bool,

    /// File globs to allow repairs on (e.g., ["src/**", "Cargo.toml"])
    pub allowed_globs: Vec<String>,

    /// File globs strictly forbidden (e.g., [".github/**", "scripts/**"])
    pub forbidden_globs: Vec<String>,

    /// Max attempts before giving up
    pub max_attempts: u32,

    /// Time budget in milliseconds
    pub time_budget_ms: u64,

    /// Tool capabilities allowed (e.g., ["fmt", "clippy_fix"])
    pub allowed_tools: Vec<String>,

    /// Allow network during repair (default: false)
    pub allow_network: bool,
}

impl RepairPolicy {
    /// Conservative default: fmt fixes only
    pub fn conservative() -> Self {
        RepairPolicy {
            allow_repair: true,
            allow_patch: true,
            allowed_globs: vec!["src/**".to_string(), "Cargo.toml".to_string()],
            forbidden_globs: vec![".github/**".to_string(), "scripts/**".to_string()],
            max_attempts: 2,
            time_budget_ms: 600_000, // 10 min
            allowed_tools: vec!["fmt".to_string()],
            allow_network: false,
        }
    }
}

/// A proposed repair action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairAction {
    /// Kind (e.g., "fmt", "clippy_fix", "test_ignore")
    pub kind: String,

    /// Target diagnostic(s) this addresses
    pub target_diagnostic_indices: Vec<usize>,

    /// Unified diff (if applicable)
    pub patch: Option<String>,

    /// Commands to run (if applicable)
    pub commands: Vec<String>,

    /// Rationale for this action
    pub rationale: String,

    /// Risk level (low, medium, high)
    pub risk_level: String,
}

/// A plan for repair: bounded, auditable, with provenance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPlan {
    /// Policy digest that governed this plan
    pub policy_digest: String,

    /// Ordered list of actions to attempt
    pub actions: Vec<RepairAction>,

    /// Total estimated diff size (bytes)
    pub estimated_diff_size: u64,

    /// When plan was created
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,

    /// Optional: patch digest if patch was generated
    pub patch_digest: Option<String>,
}

// ============================================================================
// 6. PATCH COMMIT - verified patch as AIVCS commit
// ============================================================================

/// Link to a verification run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationLink {
    /// The verification run ID
    pub verification_run_id: String,

    /// Was it verified successfully?
    pub passed: bool,

    /// When verification completed
    #[serde(with = "surreal_datetime")]
    pub verified_at: DateTime<Utc>,
}

/// A patch commit - a verified repair promoted to AIVCS commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchCommit {
    /// AIVCS commit ID (from logic + state + env hash)
    pub commit_id: String,

    /// Parent run ID that had failures
    pub parent_run_id: String,

    /// Repair plan digest that was applied
    pub repair_plan_digest: String,

    /// Patch diff digest
    pub patch_digest: String,

    /// Verification links (may have multiple verifications)
    pub verifications: Vec<VerificationLink>,

    /// Changed paths (for auditing)
    pub changed_paths: Vec<String>,

    /// When this commit was created
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// CONTENT-ADDRESSING UTILITIES
// ============================================================================

/// Compute SHA256 digest of a snapshot
pub fn compute_snapshot_digest(
    repo_sha: &str,
    workspace_hash: &str,
    config_hash: &Option<String>,
    env_hash: &Option<String>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_sha.as_bytes());
    hasher.update(workspace_hash.as_bytes());
    if let Some(ch) = config_hash {
        hasher.update(ch.as_bytes());
    }
    if let Some(eh) = env_hash {
        hasher.update(eh.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Compute SHA256 digest of a run spec
pub fn compute_run_spec_digest(spec: &CIRunSpec) -> String {
    let json = serde_json::to_string(spec).expect("CIRunSpec is serializable");
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute SHA256 digest of a policy
pub fn compute_policy_digest(policy: &RepairPolicy) -> String {
    let json = serde_json::to_string(policy).expect("RepairPolicy is serializable");
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_digest_deterministic() {
        let s1 = CISnapshot::new(
            "abc123".to_string(),
            "ws1".to_string(),
            Some("cfg1".to_string()),
            Some("env1".to_string()),
        );
        let s2 = CISnapshot::new(
            "abc123".to_string(),
            "ws1".to_string(),
            Some("cfg1".to_string()),
            Some("env1".to_string()),
        );
        assert_eq!(s1.digest, s2.digest, "Same inputs should produce same digest");
    }

    #[test]
    fn test_snapshot_digest_different_inputs() {
        let s1 = CISnapshot::new(
            "abc123".to_string(),
            "ws1".to_string(),
            None,
            None,
        );
        let s2 = CISnapshot::new(
            "xyz789".to_string(),
            "ws1".to_string(),
            None,
            None,
        );
        assert_ne!(s1.digest, s2.digest, "Different inputs should produce different digests");
    }

    #[test]
    fn test_ci_run_spec_digest_deterministic() {
        let spec1 = CIRunSpec::default_stages("1.0.0".to_string());
        let spec2 = CIRunSpec::default_stages("1.0.0".to_string());
        let d1 = compute_run_spec_digest(&spec1);
        let d2 = compute_run_spec_digest(&spec2);
        assert_eq!(d1, d2, "Same specs should produce same digest");
    }

    #[test]
    fn test_ci_run_lifecycle() {
        let mut run = CIRun::new(
            "snapshot-1".to_string(),
            "spec-digest-1".to_string(),
            "policy-digest-1".to_string(),
            "local-ci-1.0".to_string(),
        );
        assert_eq!(run.status, RunStatus::Queued);

        run.start();
        assert_eq!(run.status, RunStatus::Running);

        let result = CIResult {
            local_ci_json_digest: Some("digest-1".to_string()),
            stdout_digest: Some("digest-2".to_string()),
            stderr_digest: None,
            artifact_digests: Default::default(),
            exit_code: 0,
            duration_ms: 5000,
            cache_stats: None,
        };
        run.finish(RunStatus::Succeeded, result);
        assert_eq!(run.status, RunStatus::Succeeded);
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn test_repair_policy_conservative() {
        let policy = RepairPolicy::conservative();
        assert!(policy.allow_repair);
        assert!(policy.allow_patch);
        assert!(!policy.allow_network);
        assert_eq!(policy.max_attempts, 2);
    }

    #[test]
    fn test_policy_digest_deterministic() {
        let p1 = RepairPolicy::conservative();
        let p2 = RepairPolicy::conservative();
        let d1 = compute_policy_digest(&p1);
        let d2 = compute_policy_digest(&p2);
        assert_eq!(d1, d2, "Same policies should produce same digest");
    }
}
