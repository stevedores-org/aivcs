//! CI diagnostics parser for local-ci output.
//!
//! Normalizes raw local-ci stage output into structured [`Diagnostic`] entries.
//! This module provides the type contract and source inference logic;
//! full regex-based parsing is added in a follow-up.

use crate::ci_runner::LocalCIStageResult;
use crate::domain::ci::diagnostic::{Diagnostic, DiagnosticSource, Severity};

/// Configuration for the diagnostics parser.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticsParserConfig {
    /// Maximum number of diagnostics to retain per stage.
    pub max_per_stage: usize,

    /// Minimum severity to include.
    pub min_severity: Severity,
}

impl Default for DiagnosticsParserConfig {
    fn default() -> Self {
        Self {
            max_per_stage: 100,
            min_severity: Severity::Warning,
        }
    }
}

/// Infer the [`DiagnosticSource`] from a stage name.
pub fn infer_source(stage_name: &str) -> DiagnosticSource {
    let lower = stage_name.to_ascii_lowercase();
    if lower.contains("clippy") {
        DiagnosticSource::Clippy
    } else if lower.contains("fmt") || lower.contains("taplo") {
        DiagnosticSource::Fmt
    } else if lower.contains("test") {
        DiagnosticSource::Test
    } else if lower.contains("rustc") || lower.contains("build") || lower.contains("check") {
        DiagnosticSource::Rustc
    } else {
        DiagnosticSource::Custom
    }
}

/// Parse a local-ci stage result into normalized diagnostics.
///
/// When a stage fails, produces at least one diagnostic from the error/output.
/// This is the baseline parser; richer line-level parsing will be added later.
pub fn parse_stage_diagnostics(
    stage: &LocalCIStageResult,
    config: &DiagnosticsParserConfig,
) -> Vec<Diagnostic> {
    if stage.status == "pass" {
        return Vec::new();
    }

    let source = infer_source(&stage.name);
    let mut diagnostics = Vec::new();

    // Create a diagnostic from the error message if present
    if !stage.error.is_empty() {
        let diag = Diagnostic::new(Severity::Error, stage.error.clone(), source);
        if diag.severity >= config.min_severity {
            diagnostics.push(diag);
        }
    }

    // If no error but stage failed, create a generic failure diagnostic
    if diagnostics.is_empty() && !stage.output.is_empty() {
        let message = stage
            .output
            .lines()
            .find(|l| l.contains("error") || l.contains("warning") || l.contains("FAILED"))
            .unwrap_or("stage failed")
            .to_string();

        let diag = Diagnostic::new(Severity::Error, message, source);
        if diag.severity >= config.min_severity {
            diagnostics.push(diag);
        }
    }

    // If still empty, create a minimal failure record
    if diagnostics.is_empty() {
        diagnostics.push(Diagnostic::new(
            Severity::Error,
            format!("stage '{}' failed", stage.name),
            source,
        ));
    }

    diagnostics.truncate(config.max_per_stage);
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_source_clippy() {
        assert_eq!(infer_source("clippy"), DiagnosticSource::Clippy);
        assert_eq!(infer_source("CLIPPY"), DiagnosticSource::Clippy);
    }

    #[test]
    fn test_infer_source_fmt() {
        assert_eq!(infer_source("fmt"), DiagnosticSource::Fmt);
        assert_eq!(infer_source("taplo"), DiagnosticSource::Fmt);
    }

    #[test]
    fn test_infer_source_test() {
        assert_eq!(infer_source("test"), DiagnosticSource::Test);
    }

    #[test]
    fn test_infer_source_rustc() {
        assert_eq!(infer_source("build"), DiagnosticSource::Rustc);
        assert_eq!(infer_source("check"), DiagnosticSource::Rustc);
    }

    #[test]
    fn test_infer_source_custom() {
        assert_eq!(infer_source("audit"), DiagnosticSource::Custom);
        assert_eq!(infer_source("deny"), DiagnosticSource::Custom);
    }

    #[test]
    fn test_parse_passing_stage_returns_empty() {
        let stage = LocalCIStageResult {
            name: "fmt".to_string(),
            command: "cargo fmt".to_string(),
            status: "pass".to_string(),
            duration_ms: 100,
            cache_hit: false,
            output: String::new(),
            error: String::new(),
        };
        let config = DiagnosticsParserConfig::default();
        let diags = parse_stage_diagnostics(&stage, &config);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_failing_stage_with_error() {
        let stage = LocalCIStageResult {
            name: "clippy".to_string(),
            command: "cargo clippy".to_string(),
            status: "fail".to_string(),
            duration_ms: 500,
            cache_hit: false,
            output: String::new(),
            error: "clippy found 3 warnings".to_string(),
        };
        let config = DiagnosticsParserConfig::default();
        let diags = parse_stage_diagnostics(&stage, &config);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].source, DiagnosticSource::Clippy);
        assert!(diags[0].message.contains("clippy found"));
    }

    #[test]
    fn test_parse_failing_stage_minimal() {
        let stage = LocalCIStageResult {
            name: "test".to_string(),
            command: "cargo test".to_string(),
            status: "fail".to_string(),
            duration_ms: 2000,
            cache_hit: false,
            output: String::new(),
            error: String::new(),
        };
        let config = DiagnosticsParserConfig::default();
        let diags = parse_stage_diagnostics(&stage, &config);

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("stage 'test' failed"));
    }

    #[test]
    fn test_parse_respects_max_per_stage() {
        let config = DiagnosticsParserConfig {
            max_per_stage: 0,
            min_severity: Severity::Hint,
        };
        let stage = LocalCIStageResult {
            name: "clippy".to_string(),
            command: "cargo clippy".to_string(),
            status: "fail".to_string(),
            duration_ms: 100,
            cache_hit: false,
            output: String::new(),
            error: "error".to_string(),
        };
        let diags = parse_stage_diagnostics(&stage, &config);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parser_config_default() {
        let config = DiagnosticsParserConfig::default();
        assert_eq!(config.max_per_stage, 100);
        assert_eq!(config.min_severity, Severity::Warning);
    }
}
