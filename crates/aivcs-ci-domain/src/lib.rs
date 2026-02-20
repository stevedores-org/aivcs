//! AIVCS CI Domain Model
//!
//! Defines content-addressable CI objects as first-class AIVCS domain:
//! - CISnapshot: repo state + workspace hash + toolchain hash
//! - CIRunSpec: specification for a CI run (stages, budgets, runner version)
//! - CIRun: the run itself with status and lifecycle
//! - CIResult: outputs (local-ci JSON, logs, artifacts)
//! - Diagnostic: normalized failure/success information
//! - RepairPlan: bounded repair actions with policy enforcement
//! - PatchCommit: verified patch promoted to AIVCS commit
//!
//! All objects are serializable and content-addressable (SHA256).
//! Runs form a deterministic identity from (snapshot_id + env_hash + run_spec_digest + policy_id).

pub mod error;
pub mod schema;

pub use error::{CIDomainError, Result};
pub use schema::{
    CISnapshot, CIRunSpec, CIRun, CIResult, RunStatus, Diagnostic, DiagnosticKind, DiagnosticSeverity,
    RepairAction, RepairPlan, RepairPolicy, PatchCommit, VerificationLink,
    compute_snapshot_digest, compute_run_spec_digest, compute_policy_digest,
};

/// AIVCS CI domain version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
