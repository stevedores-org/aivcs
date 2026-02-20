//! Diagnostics record for SurrealDB persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::surreal_dt;

/// A single diagnostic record stored in SurrealDB.
///
/// One record per diagnostic entry, linked to a CI run and stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsRecord {
    /// SurrealDB record ID.
    pub id: Option<surrealdb::sql::Thing>,

    /// CI run ID this diagnostic belongs to.
    pub ci_run_id: String,

    /// Stage that produced this diagnostic (e.g. "clippy", "test").
    pub stage: String,

    /// Severity: "hint", "warning", "error".
    pub severity: String,

    /// Diagnostic/lint code (e.g. "clippy::needless_return").
    pub code: Option<String>,

    /// Human-readable message.
    pub message: String,

    /// Source file path.
    pub file: Option<String>,

    /// Line number.
    pub line: Option<u32>,

    /// Column number.
    pub column: Option<u32>,

    /// Source tool: "rustc", "clippy", "fmt", "test", "custom".
    pub source: String,

    /// When this diagnostic was recorded.
    #[serde(with = "surreal_dt")]
    pub created_at: DateTime<Utc>,
}

impl DiagnosticsRecord {
    /// Create a new diagnostics record.
    pub fn new(
        ci_run_id: String,
        stage: String,
        severity: String,
        message: String,
        source: String,
    ) -> Self {
        Self {
            id: None,
            ci_run_id,
            stage,
            severity,
            code: None,
            message,
            file: None,
            line: None,
            column: None,
            source,
            created_at: Utc::now(),
        }
    }

    /// Set file location.
    pub fn with_location(mut self, file: String, line: u32, column: u32) -> Self {
        self.file = Some(file);
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    /// Set diagnostic code.
    pub fn with_code(mut self, code: String) -> Self {
        self.code = Some(code);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostics_record_new() {
        let record = DiagnosticsRecord::new(
            "run-123".to_string(),
            "clippy".to_string(),
            "warning".to_string(),
            "unused variable".to_string(),
            "clippy".to_string(),
        );

        assert_eq!(record.ci_run_id, "run-123");
        assert_eq!(record.stage, "clippy");
        assert_eq!(record.severity, "warning");
        assert!(record.code.is_none());
        assert!(record.file.is_none());
    }

    #[test]
    fn test_diagnostics_record_with_location() {
        let record = DiagnosticsRecord::new(
            "run-123".to_string(),
            "rustc".to_string(),
            "error".to_string(),
            "cannot find value".to_string(),
            "rustc".to_string(),
        )
        .with_location("src/main.rs".to_string(), 42, 9)
        .with_code("E0425".to_string());

        assert_eq!(record.file, Some("src/main.rs".to_string()));
        assert_eq!(record.line, Some(42));
        assert_eq!(record.column, Some(9));
        assert_eq!(record.code, Some("E0425".to_string()));
    }
}
