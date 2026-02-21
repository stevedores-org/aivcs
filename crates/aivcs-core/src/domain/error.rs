//! Domain-level error taxonomy for AIVCS.

/// AIVCS domain errors.
#[derive(Debug, thiserror::Error)]
pub enum AivcsError {
    #[error("invalid agent spec: {0}")]
    InvalidAgentSpec(String),

    #[error("invalid CI run spec: {0}")]
    InvalidCIRunSpec(String),

    #[error("run not found: {0}")]
    RunNotFound(uuid::Uuid),

    #[error("eval suite not found: {0}")]
    EvalSuiteNotFound(uuid::Uuid),

    #[error("release conflict: {0}")]
    ReleaseConflict(String),

    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },

    #[error("git error: {0}")]
    GitError(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("storage error: {0}")]
    StorageError(String),
}

/// Result type for AIVCS domain operations.
pub type Result<T> = std::result::Result<T, AivcsError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aivcs_error_display() {
        let err = AivcsError::InvalidAgentSpec("missing git_sha".to_string());
        assert!(err.to_string().contains("invalid agent spec"));

        let err = AivcsError::InvalidCIRunSpec("stages cannot be empty".to_string());
        assert!(err.to_string().contains("invalid CI run spec"));

        let id = uuid::Uuid::new_v4();
        let err = AivcsError::RunNotFound(id);
        assert!(err.to_string().contains("run not found"));
    }

    #[test]
    fn test_digest_mismatch_error() {
        let err = AivcsError::DigestMismatch {
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("abc123"));
        assert!(msg.contains("def456"));
    }

    #[test]
    fn test_storage_error() {
        let err = AivcsError::StorageError("database connection failed".to_string());
        assert!(err.to_string().contains("storage error"));
        assert!(err.to_string().contains("database connection failed"));
    }
}
