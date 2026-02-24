//! Multi-repo and CI/CD orchestration (EPIC9).
//!
//! Provides:
//! - [`graph::RepoDependencyGraph`] / [`graph::RepoExecutionPlan`] — topological repo ordering
//! - [`sequencer::ReleaseSequencer`] — cross-repo coordinated release sequencing
//! - [`aggregator::CiAggregator`] / [`aggregator::CiHealthReport`] — unified CI signal collection
//! - [`backport::BackportPolicy`] / [`backport::BackportExecutor`] — automated backport application
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use aivcs_core::multi_repo::{
//!     graph::{RepoDependencyGraph, RepoNode},
//!     aggregator::{CiAggregator, CiRunFetcher},
//!     sequencer::{ReleaseSequencer, RepoReleaser},
//!     backport::{BackportExecutor, BackportPolicy},
//! };
//! ```

pub mod aggregator;
pub mod backport;
pub mod error;
pub mod graph;
pub mod sequencer;

pub use aggregator::{CiAggregator, CiHealthReport, CiRunFetcher, RepoHealth, RepoHealthStatus};
pub use backport::{BackportExecutor, BackportOutcome, BackportPolicy, BackportTask};
pub use error::{MultiRepoError, MultiRepoResult};
pub use graph::{RepoDependencyGraph, RepoExecutionPlan, RepoNode, RepoStep};
pub use sequencer::{
    ReleaseSequencer, RepoReleaseStatus, RepoReleaser, SequenceItem, SequenceOutcome, SequencePlan,
};
