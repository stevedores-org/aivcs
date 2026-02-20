//! Error types for CI domain operations

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CIDomainError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid run status transition: {current} -> {requested}")]
    InvalidStatusTransition { current: String, requested: String },

    #[error("Policy violation: {0}")]
    PolicyViolation(String),

    #[error("Digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },

    #[error("Invalid stage: {0}")]
    InvalidStage(String),

    #[error("Repair plan error: {0}")]
    RepairPlanError(String),

    #[error("Verification link error: {0}")]
    VerificationError(String),
}

/// Result type for CI domain operations
pub type Result<T> = std::result::Result<T, CIDomainError>;
