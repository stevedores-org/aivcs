//! CI runner trait and local-ci wrapper types.
//!
//! Defines the `CIRunner` async trait for executing CI runs, plus
//! types matching the `local-ci --json` output schema.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::ci::result::CIResult;
use crate::domain::ci::run_spec::CIRunSpec;
use crate::domain::error::AivcsError;

// ---------------------------------------------------------------------------
// Runner trait
// ---------------------------------------------------------------------------

/// Trait for CI runner backends (local-ci, embedded, etc.).
#[async_trait]
pub trait CIRunner: Send + Sync {
    /// Execute a CI run and return the result.
    async fn run(&self, spec: &CIRunSpec) -> std::result::Result<CIResult, AivcsError>;
}

// ---------------------------------------------------------------------------
// Local-CI configuration
// ---------------------------------------------------------------------------

/// Configuration for the local-ci runner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalCIConfig {
    /// Path to the local-ci binary.
    pub binary_path: String,

    /// Working directory for CI execution.
    pub work_dir: String,

    /// Whether to emit JSON output (--json flag).
    pub json_output: bool,

    /// Whether to use caching (negation of --no-cache).
    pub cache_enabled: bool,

    /// Whether to stop on first failure (--fail-fast flag).
    pub fail_fast: bool,

    /// Whether to auto-fix (--fix flag).
    pub fix_mode: bool,
}

impl Default for LocalCIConfig {
    fn default() -> Self {
        Self {
            binary_path: "local-ci".to_string(),
            work_dir: ".".to_string(),
            json_output: true,
            cache_enabled: true,
            fail_fast: false,
            fix_mode: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Local-CI JSON output types (matching local-ci v0.2.0 schema)
// ---------------------------------------------------------------------------

/// Top-level JSON output from `local-ci --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalCIPipelineReport {
    /// local-ci version string.
    pub version: String,

    /// Total wall-clock duration in milliseconds.
    pub duration_ms: u64,

    /// Number of stages that passed.
    pub passed: u32,

    /// Number of stages that failed.
    pub failed: u32,

    /// Per-stage results.
    pub results: Vec<LocalCIStageResult>,
}

/// Per-stage result from `local-ci --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalCIStageResult {
    /// Stage name (e.g. "fmt", "clippy", "test").
    pub name: String,

    /// Full command string.
    pub command: String,

    /// "pass" or "fail".
    pub status: String,

    /// Stage execution duration in milliseconds.
    pub duration_ms: u64,

    /// Whether the result came from cache.
    pub cache_hit: bool,

    /// Combined stdout+stderr (may be empty).
    #[serde(default)]
    pub output: String,

    /// Error message if the stage failed (may be empty).
    #[serde(default)]
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_ci_config_default() {
        let config = LocalCIConfig::default();
        assert_eq!(config.binary_path, "local-ci");
        assert!(config.json_output);
        assert!(config.cache_enabled);
        assert!(!config.fail_fast);
        assert!(!config.fix_mode);
    }

    #[test]
    fn test_local_ci_config_serde_roundtrip() {
        let config = LocalCIConfig {
            binary_path: "/usr/local/bin/local-ci".to_string(),
            work_dir: "/home/user/project".to_string(),
            json_output: true,
            cache_enabled: false,
            fail_fast: true,
            fix_mode: false,
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: LocalCIConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_local_ci_pipeline_report_serde_roundtrip() {
        let report = LocalCIPipelineReport {
            version: "0.2.0".to_string(),
            duration_ms: 12345,
            passed: 3,
            failed: 0,
            results: vec![
                LocalCIStageResult {
                    name: "fmt".to_string(),
                    command: "cargo fmt --all -- --check".to_string(),
                    status: "pass".to_string(),
                    duration_ms: 423,
                    cache_hit: false,
                    output: String::new(),
                    error: String::new(),
                },
                LocalCIStageResult {
                    name: "test".to_string(),
                    command: "cargo test --workspace".to_string(),
                    status: "pass".to_string(),
                    duration_ms: 8000,
                    cache_hit: true,
                    output: String::new(),
                    error: String::new(),
                },
            ],
        };

        let json = serde_json::to_string(&report).expect("serialize");
        let deserialized: LocalCIPipelineReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, deserialized);
    }

    #[test]
    fn test_local_ci_stage_result_with_output() {
        let result = LocalCIStageResult {
            name: "clippy".to_string(),
            command: "cargo clippy --workspace".to_string(),
            status: "fail".to_string(),
            duration_ms: 4500,
            cache_hit: false,
            output: "warning: unused variable `x`\n".to_string(),
            error: "clippy found warnings".to_string(),
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let deserialized: LocalCIStageResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, deserialized);
    }

    #[test]
    fn test_local_ci_stage_result_missing_optional_fields() {
        // Simulate minimal JSON from local-ci (output/error may be omitted)
        let json = r#"{"name":"fmt","command":"cargo fmt","status":"pass","duration_ms":100,"cache_hit":true}"#;
        let result: LocalCIStageResult = serde_json::from_str(json).expect("deserialize");
        assert_eq!(result.output, "");
        assert_eq!(result.error, "");
    }
}
