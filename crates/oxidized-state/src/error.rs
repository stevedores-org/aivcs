//! Error types for oxidized-state

use thiserror::Error;

/// Errors that can occur in the state persistence layer
#[derive(Error, Debug)]
pub enum StateError {
    /// Database connection error
    #[error("Database connection failed: {0}")]
    Connection(String),

    /// Database query error
    #[error("Database query failed: {0}")]
    Query(String),

    /// Serialization error
    #[error("Serialization failed: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization failed: {0}")]
    Deserialization(String),

    /// Commit not found
    #[error("Commit not found: {0}")]
    CommitNotFound(String),

    /// Branch not found
    #[error("Branch not found: {0}")]
    BranchNotFound(String),

    /// Invalid commit ID format
    #[error("Invalid commit ID: {0}")]
    InvalidCommitId(String),

    /// Transaction failed
    #[error("Transaction failed: {0}")]
    Transaction(String),

    /// Schema setup error
    #[error("Schema setup failed: {0}")]
    SchemaSetup(String),
}

impl From<surrealdb::Error> for StateError {
    fn from(err: surrealdb::Error) -> Self {
        StateError::Query(err.to_string())
    }
}

impl From<serde_json::Error> for StateError {
    fn from(err: serde_json::Error) -> Self {
        StateError::Serialization(err.to_string())
    }
}
