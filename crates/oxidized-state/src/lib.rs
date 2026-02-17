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
pub mod fakes;
mod handle;
mod schema;
pub mod storage_traits;
pub mod surreal_ledger;

pub use error::{StateError, StorageError};
pub use handle::{CloudConfig, SurrealHandle};
pub use schema::{
    AgentRecord, BranchRecord, CommitId, CommitRecord, GraphEdge, MemoryRecord, SnapshotRecord,
};
pub use storage_traits::{
    CasStore, ContentDigest, ReleaseMetadata, ReleaseRecord, ReleaseRegistry, RunEvent, RunId,
    RunLedger, RunMetadata, RunRecord, RunStatus, RunSummary, StorageResult,
};
pub use surreal_ledger::SurrealRunLedger;

/// Result type for oxidized-state operations
pub type Result<T> = std::result::Result<T, StateError>;
