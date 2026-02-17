//! Domain models for AIVCS.
//!
//! Canonical definitions for the core entities:
//! - `AgentSpec`: Immutable specification of an agent
//! - `Run`: Execution instance of an agent
//! - `EvalSuite`: Evaluation framework for testing agents
//! - `Release`: Deployment records

pub mod agent_spec;
pub mod digest;
pub mod error;
pub mod eval;
pub mod release;
pub mod run;

// Re-export main types and errors
pub use agent_spec::{AgentSpec, AgentSpecFields};
pub use error::{AivcsError, Result};
pub use eval::{EvalSuite, EvalTestCase, EvalThresholds, ScorerConfig, ScorerType};
pub use release::{Release, ReleaseEnvironment, ReleasePointer};
pub use run::{Event, EventKind, Run, RunStatus};
