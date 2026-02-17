//! AIVCS Core Library
//!
//! Re-exports core components for programmatic access to AIVCS functionality.

pub mod cas;
pub mod diff;
pub mod domain;
pub mod event_adapter;
pub mod git;
pub mod parallel;
pub mod recording;
pub mod replay;

pub use diff::{diff_tool_calls, DiffSummary, ParamChange, ToolCallChange, ToolCallEntry};

pub use domain::{
    AgentSpec, AgentSpecFields, AivcsError, EvalSuite, EvalTestCase, EvalThresholds, Event,
    EventKind, Release, ReleaseEnvironment, ReleasePointer, Result, Run, RunStatus, ScorerConfig,
    ScorerType, SnapshotMeta,
};

pub use event_adapter::{subscribe_ledger_to_bus, LedgerHandler};

pub use git::{capture_head_sha, is_git_repo};

pub use oxidized_state::{
    BranchRecord, CommitId, CommitRecord, MemoryRecord, SnapshotRecord, SurrealHandle,
};

pub use nix_env_manager::{
    generate_environment_hash, generate_logic_hash, is_attic_available, is_nix_available,
    AtticClient, AtticConfig, FlakeMetadata, HashSource, NixHash,
};

pub use semantic_rag_merge::{
    diff_memory_vectors, resolve_conflict_state, semantic_merge, synthesize_memory,
    AutoResolvedValue, MemoryConflict, MergeResult, VectorStoreDelta,
};

pub use cas::fs::FsCasStore;
pub use cas::{CasError, CasStore, Digest};

pub use parallel::{
    fork_agent_parallel, BranchStatus, ForkResult, ParallelConfig, ParallelManager,
};

pub use diff::node_paths::{
    diff_node_paths, extract_node_path, NodeDivergence, NodePathDiff, NodeStep,
};
pub use diff::tool_calls::{diff_tool_calls, ParamDelta, ToolCall, ToolCallChange, ToolCallDiff};
pub use recording::GraphRunRecorder;
pub use replay::{replay_run, ReplaySummary};

/// AIVCS version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
