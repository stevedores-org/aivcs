//! Oxidized-State: SurrealDB Backend for AIVCS
//!
//! This crate provides the persistence layer for the Agent Version Control System.
//! It handles all I/O with SurrealDB, providing a clean persistence layer for
//! graph states and memory.
//!
//! ## Layer 0 - Data/Persistence
//!
//! Focus: Data integrity, transactionality, and graph traversal.
//!
//! ## Key Components
//!
//! - `SurrealHandle`: Manages connection and transactions
//! - `SnapshotRecord`: Schema mapping to the Document Layer (State + Memory)
//! - `GraphEdge`: Schema mapping to the Graph Layer (Commit -> Parent)

mod error;
mod handle;
mod schema;

pub use error::StateError;
pub use handle::SurrealHandle;
pub use schema::{
    AgentRecord, BranchRecord, CommitId, CommitRecord, GraphEdge, MemoryRecord, SnapshotRecord,
};

/// Result type for oxidized-state operations
pub type Result<T> = std::result::Result<T, StateError>;
