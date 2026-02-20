//! CI Run Envelope - Stable request/response contract for orchestrator
//!
//! Story 0.1: Specify CI Run Envelope schema (Request/Response)
//! Story 0.2: Define normalized diagnostics[] shape + state mapping
//!
//! The Envelope is the public API boundary between orchestrator and CI implementations.
//! Both legacy ai-agent-ci and new ath engine must conform to this contract.
//! It is backward-compatible and allows wrapping legacy without changes to its internals.

use crate::schema::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// REQUEST - What the orchestrator asks for
// ============================================================================

/// Repository specification (path or git URL)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepoSpec {
    /// Local filesystem path
    Local { path: String },
    /// Git repository URL
    Remote { url: String, ref_name: String },
}

/// CI execution options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CIOptions {
    /// Return JSON output (always true for ath)
    pub json: bool,

    /// Enable automatic fixes (fmt, clippy-fix, etc.)
    pub fix: bool,

    /// Enable stage caching
    pub cache: bool,

    /// Fail on first stage failure
    pub fail_fast: bool,

    /// Per-stage timeout in milliseconds
    pub stage_timeout_ms: Option<u64>,

    /// Total run timeout in milliseconds
    pub total_timeout_ms: Option<u64>,

    /// Maximum retry attempts per stage
    pub max_attempts: Option<u32>,

    /// Extra environment variables
    pub env: HashMap<String, String>,
}

impl CIOptions {
    /// Conservative default (no fixes, caching enabled)
    pub fn conservative() -> Self {
        CIOptions {
            json: true,
            fix: false,
            cache: true,
            fail_fast: false,
            stage_timeout_ms: Some(300_000),        // 5 min
            total_timeout_ms: Some(1_800_000),      // 30 min
            max_attempts: Some(2),
            env: HashMap::new(),
        }
    }

    /// Aggressive default (fixes enabled, no retry)
    pub fn aggressive() -> Self {
        CIOptions {
            json: true,
            fix: true,
            cache: true,
            fail_fast: true,
            stage_timeout_ms: Some(300_000),
            total_timeout_ms: Some(1_800_000),
            max_attempts: Some(1),
            env: HashMap::new(),
        }
    }
}

/// The CI Run Request - what orchestrator sends to CI engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRunRequest {
    /// Repository to run CI on
    pub repo: RepoSpec,

    /// Stages to run (e.g., ["fmt", "clippy", "test"])
    /// If empty, uses engine defaults
    pub stages: Vec<String>,

    /// Execution options
    #[serde(default)]
    pub options: CIOptions,

    /// Repair policy (constraints on what fixes are allowed)
    #[serde(default)]
    pub policy: RepairPolicy,

    /// Optional: custom run metadata/tags
    pub metadata: HashMap<String, String>,
}

impl CIRunRequest {
    /// Create a minimal request (repo only, all defaults)
    pub fn minimal(repo: RepoSpec) -> Self {
        CIRunRequest {
            repo,
            stages: vec![],
            options: CIOptions::conservative(),
            policy: RepairPolicy::conservative(),
            metadata: HashMap::new(),
        }
    }

    /// Validate request (all required fields present)
    pub fn validate(&self) -> crate::Result<()> {
        if self.stages.is_empty() {
            // Empty stages is OK - engine uses defaults
        }
        Ok(())
    }
}

// ============================================================================
// RESPONSE - What CI engine returns to orchestrator
// ============================================================================

/// Artifact reference (stored in CAS)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Artifact type (e.g., "junit", "coverage", "log")
    pub artifact_type: String,

    /// CAS digest of artifact content
    pub digest: String,

    /// Optional human-readable name
    pub name: Option<String>,

    /// Size in bytes
    pub size_bytes: u64,
}

/// Summary of run results for human consumption
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunSummary {
    /// One-line summary (e.g., "2 failures, 1 warning")
    pub title: String,

    /// Detailed text summary
    pub details: Option<String>,

    /// Top issues by category
    pub top_issues: Vec<String>,

    /// Recommendations (e.g., "Run `cargo fmt` to fix formatting")
    pub recommendations: Vec<String>,
}

/// The CI Run Response - what CI engine returns to orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRunResponse {
    /// Unique run ID
    pub run_id: String,

    /// Current state: queued/running/succeeded/failed/canceled
    pub state: String,

    /// Results (populated when state is succeeded/failed/canceled)
    pub results: Option<CIRunResults>,

    /// Human-readable summary
    pub summary: Option<RunSummary>,

    /// When this response was generated
    #[serde(with = "crate::schema::surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

impl CIRunResponse {
    /// Create a queued response
    pub fn queued(run_id: String) -> Self {
        CIRunResponse {
            run_id,
            state: "queued".to_string(),
            results: None,
            summary: None,
            timestamp: Utc::now(),
        }
    }

    /// Create a running response
    pub fn running(run_id: String) -> Self {
        CIRunResponse {
            run_id,
            state: "running".to_string(),
            results: None,
            summary: None,
            timestamp: Utc::now(),
        }
    }

    /// Create a completed response
    pub fn completed(run_id: String, state: &str, results: CIRunResults) -> Self {
        CIRunResponse {
            run_id,
            state: state.to_string(),
            results: Some(results),
            summary: None,
            timestamp: Utc::now(),
        }
    }

    /// Add summary
    pub fn with_summary(mut self, summary: RunSummary) -> Self {
        self.summary = Some(summary);
        self
    }

    /// Is the run complete?
    pub fn is_complete(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "canceled")
    }

    /// Did the run pass?
    pub fn passed(&self) -> bool {
        self.state == "succeeded"
    }
}

/// Results of a completed CI run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CIRunResults {
    /// Duration in milliseconds
    pub duration_ms: u64,

    /// Normalized diagnostics
    pub diagnostics: Vec<Diagnostic>,

    /// Repair plan (if failures were found and repair is enabled)
    pub repair_plan: Option<RepairPlan>,

    /// Patch (if repair was applied)
    pub patch: Option<PatchDetails>,

    /// Artifacts (logs, coverage, etc.)
    pub artifacts: Vec<ArtifactRef>,

    /// Raw legacy output (if wrapped legacy ai-agent-ci)
    pub raw_legacy: Option<serde_json::Value>,

    /// Raw local-ci JSON output
    pub raw_local_ci_json: Option<serde_json::Value>,
}

/// Details about an applied patch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchDetails {
    /// Unified diff
    pub unified_diff: String,

    /// CAS digest of patch
    pub digest: String,

    /// Files changed
    pub changed_files: Vec<String>,

    /// Lines added
    pub lines_added: u32,

    /// Lines removed
    pub lines_removed: u32,

    /// Verification run ID (if patch was verified)
    pub verification_run_id: Option<String>,
}

// ============================================================================
// NORMALIZED DIAGNOSTICS (Story 0.2)
// ============================================================================

/// State mapping: runner exit codes → standard envelope states
///
/// Ensures consistent state across all implementations
pub fn map_exit_code_to_state(exit_code: i32) -> String {
    match exit_code {
        0 => "succeeded".to_string(),
        124 => "canceled".to_string(), // timeout
        _ => "failed".to_string(),
    }
}

/// Normalize diagnostics for consistent API
///
/// Ensures all implementations produce compatible diagnostic format
pub fn normalize_diagnostics(diags: &[Diagnostic]) -> Vec<Diagnostic> {
    let mut normalized = diags.to_vec();

    // Sort by severity (error > warning > info) then stage
    normalized.sort_by(|a, b| {
        let severity_cmp = (match b.severity {
            DiagnosticSeverity::Error => 2,
            DiagnosticSeverity::Warning => 1,
            DiagnosticSeverity::Info => 0,
        })
        .cmp(&(match a.severity {
            DiagnosticSeverity::Error => 2,
            DiagnosticSeverity::Warning => 1,
            DiagnosticSeverity::Info => 0,
        }));

        if severity_cmp == std::cmp::Ordering::Equal {
            a.stage.cmp(&b.stage)
        } else {
            severity_cmp
        }
    });

    normalized
}

// ============================================================================
// ENVELOPE VALIDATION
// ============================================================================

/// Validate request/response envelope contract
pub trait EnvelopeValidator {
    fn validate(&self) -> crate::Result<()>;
}

impl EnvelopeValidator for CIRunRequest {
    fn validate(&self) -> crate::Result<()> {
        // Request must have a repository
        // Stages, options, policy can be empty/default
        Ok(())
    }
}

impl EnvelopeValidator for CIRunResponse {
    fn validate(&self) -> crate::Result<()> {
        // Response must always have run_id and state
        if self.run_id.is_empty() {
            return Err(crate::error::CIDomainError::PolicyViolation(
                "Response missing run_id".to_string(),
            ));
        }

        // state must be one of: queued, running, succeeded, failed, canceled
        match self.state.as_str() {
            "queued" | "running" | "succeeded" | "failed" | "canceled" => {
                // If complete, must have results
                if self.is_complete() && self.results.is_none() {
                    return Err(crate::error::CIDomainError::PolicyViolation(
                        "Complete response missing results".to_string(),
                    ));
                }
                Ok(())
            }
            _ => Err(crate::error::CIDomainError::PolicyViolation(
                format!("Invalid state: {}", self.state),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_spec_local() {
        let repo = RepoSpec::Local {
            path: "/path/to/repo".to_string(),
        };
        let json = serde_json::to_string(&repo).unwrap();
        assert!(json.contains("path"));
    }

    #[test]
    fn test_repo_spec_remote() {
        let repo = RepoSpec::Remote {
            url: "https://github.com/org/repo".to_string(),
            ref_name: "main".to_string(),
        };
        let json = serde_json::to_string(&repo).unwrap();
        assert!(json.contains("url"));
    }

    #[test]
    fn test_ci_options_conservative() {
        let opts = CIOptions::conservative();
        assert!(!opts.fix);
        assert!(opts.cache);
        assert!(opts.json);
    }

    #[test]
    fn test_ci_options_aggressive() {
        let opts = CIOptions::aggressive();
        assert!(opts.fix);
        assert!(opts.json);
        assert!(opts.fail_fast);
    }

    #[test]
    fn test_run_request_minimal() {
        let repo = RepoSpec::Local {
            path: ".".to_string(),
        };
        let req = CIRunRequest::minimal(repo);
        assert!(req.validate().is_ok());
        assert_eq!(req.stages.len(), 0);
    }

    #[test]
    fn test_run_response_queued() {
        let resp = CIRunResponse::queued("run-123".to_string());
        assert_eq!(resp.state, "queued");
        assert_eq!(resp.run_id, "run-123");
        assert!(!resp.is_complete());
    }

    #[test]
    fn test_run_response_running() {
        let resp = CIRunResponse::running("run-456".to_string());
        assert_eq!(resp.state, "running");
        assert!(!resp.is_complete());
    }

    #[test]
    fn test_run_response_completed() {
        let results = CIRunResults {
            duration_ms: 5000,
            diagnostics: vec![],
            repair_plan: None,
            patch: None,
            artifacts: vec![],
            raw_legacy: None,
            raw_local_ci_json: None,
        };
        let resp = CIRunResponse::completed("run-789".to_string(), "succeeded", results);
        assert!(resp.is_complete());
        assert!(resp.passed());
    }

    #[test]
    fn test_response_with_summary() {
        let results = CIRunResults {
            duration_ms: 5000,
            diagnostics: vec![],
            repair_plan: None,
            patch: None,
            artifacts: vec![],
            raw_legacy: None,
            raw_local_ci_json: None,
        };
        let summary = RunSummary {
            title: "All tests passed".to_string(),
            details: None,
            top_issues: vec![],
            recommendations: vec![],
        };
        let resp = CIRunResponse::completed("run-999".to_string(), "succeeded", results)
            .with_summary(summary);
        assert!(resp.summary.is_some());
    }

    #[test]
    fn test_exit_code_mapping() {
        assert_eq!(map_exit_code_to_state(0), "succeeded");
        assert_eq!(map_exit_code_to_state(1), "failed");
        assert_eq!(map_exit_code_to_state(124), "canceled");
    }

    #[test]
    fn test_normalize_diagnostics() {
        let diags = vec![
            Diagnostic {
                kind: DiagnosticKind::Lint,
                stage: "clippy".to_string(),
                severity: DiagnosticSeverity::Warning,
                message: "warning".to_string(),
                file: None,
                line: None,
                rule: None,
                evidence: None,
                command: None,
                exit_code: None,
                fix_confidence: None,
            },
            Diagnostic {
                kind: DiagnosticKind::Compilation,
                stage: "build".to_string(),
                severity: DiagnosticSeverity::Error,
                message: "error".to_string(),
                file: None,
                line: None,
                rule: None,
                evidence: None,
                command: None,
                exit_code: None,
                fix_confidence: None,
            },
        ];

        let normalized = normalize_diagnostics(&diags);
        // Errors should come before warnings
        assert_eq!(
            normalized[0].severity,
            DiagnosticSeverity::Error,
            "Errors should sort first"
        );
    }

    #[test]
    fn test_response_validation_valid() {
        let results = CIRunResults {
            duration_ms: 1000,
            diagnostics: vec![],
            repair_plan: None,
            patch: None,
            artifacts: vec![],
            raw_legacy: None,
            raw_local_ci_json: None,
        };
        let resp = CIRunResponse::completed("run-ok".to_string(), "succeeded", results);
        assert!(resp.validate().is_ok());
    }

    #[test]
    fn test_response_validation_missing_run_id() {
        let resp = CIRunResponse::queued("".to_string());
        assert!(resp.validate().is_err());
    }

    #[test]
    fn test_response_validation_invalid_state() {
        let mut resp = CIRunResponse::queued("run-bad".to_string());
        resp.state = "unknown".to_string();
        assert!(resp.validate().is_err());
    }

    #[test]
    fn test_response_validation_missing_results() {
        let mut resp = CIRunResponse::queued("run-incomplete".to_string());
        resp.state = "succeeded".to_string();
        // Response says succeeded but has no results - should error
        assert!(resp.validate().is_err());
    }

    #[test]
    fn test_request_response_roundtrip() {
        let req = CIRunRequest::minimal(RepoSpec::Local {
            path: ".".to_string(),
        });
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: CIRunRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.validate().is_ok());
    }
}
