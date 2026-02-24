//! AIVCS Core Library
//!
//! Re-exports core components for programmatic access to AIVCS functionality.

pub mod cas;
pub mod compat;
pub mod deploy;
pub mod deploy_runner;
pub mod diff;
pub mod domain;
pub mod event_adapter;
pub mod gate;
pub mod git;
pub mod metrics;
pub mod obs;
pub mod orchestration;
pub mod parallel;
pub mod publish_gate;
pub mod recording;
pub mod release_registry;
pub mod replay;
pub mod reporting;
pub mod role_orchestration;
pub mod telemetry;
pub mod trace_artifact;

pub use domain::{
    validate_run_event, AgentSpec, AgentSpecFields, AivcsError, DeterministicEvalRunner,
    EvalCaseResult, EvalRunReport, EvalSuite, EvalTestCase, EvalThresholds, Event, EventKind,
    Release, ReleaseEnvironment, ReleasePointer, Result, Run, RunStatus, ScorerConfig, ScorerType,
    SnapshotMeta, ValidationError,
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
pub use compat::{
    evaluate_compat, CompatRule, CompatRuleSet, CompatVerdict, CompatViolation, PromoteContext,
};
pub use deploy::{deploy_by_digest, DeployResult};
pub use deploy_runner::{DeployByDigestRunner, DeployRunOutput};
pub use diff::lcs_diff::{
    diff_tool_calls as diff_tool_calls_lcs, DiffSummary, ParamChange,
    ToolCallChange as LcsToolCallChange, ToolCallEntry,
};
pub use diff::state_diff::{
    diff_run_states, diff_scoped_state, extract_last_checkpoint, ScopedStateDiff, StateDelta,
    CHECKPOINT_SAVED_KIND,
};
pub use orchestration::{
    default_role_templates, deterministic_role_order, merge_role_outputs, validate_handoff,
    validate_parallel_roles, AgentRole, HandoffValidationError, MergeConflict,
    MergeConflictStrategy, MergeOutcome, ParallelPlanError, RoleHandoff, RoleOutput, RoleTemplate,
};
pub use parallel::{
    fork_agent_parallel, BranchStatus, ForkResult, ParallelConfig, ParallelManager,
};

pub use diff::node_paths::{
    diff_node_paths, extract_node_path, NodeDivergence, NodePathDiff, NodeStep,
};
pub use diff::tool_calls::{diff_tool_calls, ParamDelta, ToolCall, ToolCallChange, ToolCallDiff};
pub use gate::{
    evaluate_gate, CaseResult, EvalReport, GateRule, GateRuleSet, GateVerdict, Violation,
};
pub use recording::GraphRunRecorder;
pub use release_registry::ReleaseRegistryApi;
pub use replay::{find_resume_point, replay_run, verify_spec_digest, ReplaySummary, ResumePoint};
pub use reporting::{
    render_diff_summary_md, write_diff_summary_md, write_eval_results_json, DiffSummaryArtifact,
    EvalCaseResultArtifact, EvalResultsArtifact, EvalSummaryArtifact,
};

pub use trace_artifact::{
    read_trace_artifact, write_trace_artifact, RetentionPolicy, RunTraceArtifact,
};

pub use role_orchestration::error::{RoleError, RoleResult};
pub use role_orchestration::executor::{
    execute_roles_parallel, token_from_result, ParallelRoleConfig, RoleRunResult,
};
pub use role_orchestration::merge::{merge_parallel_outputs, MergedRoleOutput, RoleConflict};
pub use role_orchestration::roles::{AgentRole, HandoffToken, RoleOutput, RoleTemplate};
pub use role_orchestration::router::{
    build_execution_plan, validate_handoff_sequence, ExecutionPlan, RoleStep,
};

pub use metrics::METRICS;
pub use obs::{
    emit_event_appended, emit_gate_evaluated, emit_run_finalize_error, emit_run_finished,
    emit_run_started, RunSpan,
};
pub use telemetry::init_tracing;

/// AIVCS version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
