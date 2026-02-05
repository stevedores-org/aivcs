//! AIVCS Core Library
//!
//! Re-exports core components for programmatic access to AIVCS functionality.

pub mod parallel;

pub use oxidized_state::{
    CommitId, CommitRecord, BranchRecord, SnapshotRecord, MemoryRecord, SurrealHandle,
};

pub use nix_env_manager::{
    NixHash, HashSource, FlakeMetadata,
    generate_environment_hash, generate_logic_hash,
    AtticClient, AtticConfig,
    is_nix_available, is_attic_available,
};

pub use semantic_rag_merge::{
    VectorStoreDelta, MemoryConflict, AutoResolvedValue, MergeResult,
    diff_memory_vectors, resolve_conflict_state, synthesize_memory, semantic_merge,
};

pub use parallel::{
    fork_agent_parallel, ForkResult, BranchStatus, ParallelConfig, ParallelManager,
};

/// AIVCS version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
