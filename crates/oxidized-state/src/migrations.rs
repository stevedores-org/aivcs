//! SurrealDB schema migrations and initialization
//!
//! This module provides initialization functions to set up all tables
//! with proper constraints, indexes, and ACID guarantees.

use crate::Result;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::{debug, info};

/// Initialize all AIVCS tables in SurrealDB
///
/// This should be called once on first connection to set up the schema.
/// Safe to call multiple times (idempotent).
pub async fn init_schema(db: &Surreal<Any>) -> Result<()> {
    info!("Initializing AIVCS SurrealDB schema");

    // Initialize run ledger tables
    init_runs_table(db).await?;
    init_run_events_table(db).await?;
    init_releases_table(db).await?;

    // Initialize existing tables if needed
    init_commits_table(db).await?;
    init_branches_table(db).await?;

    info!("AIVCS schema initialization complete");
    Ok(())
}

/// Initialize `runs` table with constraints and indexes
///
/// Schema:
/// ```text
/// TABLE runs {
///   run_id:              STRING (primary key, unique)
///   spec_digest:         STRING (indexed)
///   git_sha:             STRING? (optional, indexed)
///   agent_name:          STRING (indexed)
///   tags:                OBJECT
///   status:              STRING (enum: running | completed | failed)
///   total_events:        INT
///   final_state_digest:  STRING?
///   duration_ms:         INT
///   success:             BOOL
///   created_at:          DATETIME (indexed)
///   completed_at:        DATETIME?
/// }
/// ```
///
/// Constraints:
/// - `run_id` is unique (prevents duplicate runs)
/// - `status` must be one of: "running", "completed", "failed"
/// - `status` transitions: running → completed | failed (enforced via app logic)
/// - Completed runs are immutable (enforced via app logic)
async fn init_runs_table(db: &Surreal<Any>) -> Result<()> {
    debug!("Initializing runs table");

    // Create table with constraints
    let sql = r#"
        DEFINE TABLE runs AS
            SCHEMALESS
            PERMISSIONS
                FOR create FULL
                FOR read FULL
                FOR update FULL
                FOR delete NONE;

        -- Ensure run_id is unique
        DEFINE INDEX idx_run_id ON TABLE runs COLUMNS run_id UNIQUE;

        -- Index spec_digest for listing runs by agent version
        DEFINE INDEX idx_spec_digest ON TABLE runs COLUMNS spec_digest;

        -- Index agent_name for finding runs by agent
        DEFINE INDEX idx_agent_name ON TABLE runs COLUMNS agent_name;

        -- Index git_sha for correlating runs with git commits
        DEFINE INDEX idx_git_sha ON TABLE runs COLUMNS git_sha;

        -- Index created_at for time-range queries
        DEFINE INDEX idx_created_at ON TABLE runs COLUMNS created_at DESC;

        -- Composite index (spec_digest, created_at) for fast agent version history
        DEFINE INDEX idx_spec_digest_created_at ON TABLE runs COLUMNS spec_digest, created_at DESC;

        -- Composite index (run_id, status) for state queries
        DEFINE INDEX idx_run_id_status ON TABLE runs COLUMNS run_id, status;
    "#;

    db.query(sql).await?;
    info!("✓ runs table initialized");
    Ok(())
}

/// Initialize `run_events` table with constraints and indexes
///
/// Schema:
/// ```text
/// TABLE run_events {
///   run_id:     STRING (foreign key to runs.run_id)
///   seq:        INT (monotonic sequence within run)
///   kind:       STRING (event type)
///   payload:    OBJECT (event data)
///   timestamp:  DATETIME
/// }
/// ```
///
/// Constraints:
/// - `(run_id, seq)` is unique and clustered (prevents duplicate seq)
/// - `seq` is 1-indexed and monotonically increasing within a run
/// - Enforced via application logic during append_event()
async fn init_run_events_table(db: &Surreal<Any>) -> Result<()> {
    debug!("Initializing run_events table");

    let sql = r#"
        DEFINE TABLE run_events AS
            SCHEMALESS
            PERMISSIONS
                FOR create FULL
                FOR read FULL
                FOR update NONE
                FOR delete NONE;

        -- Composite unique index: (run_id, seq) ensures no duplicate sequences per run
        -- This is the most critical constraint for event ordering
        DEFINE INDEX idx_run_id_seq ON TABLE run_events COLUMNS run_id, seq UNIQUE;

        -- Index run_id for fast event retrieval by run
        DEFINE INDEX idx_run_id ON TABLE run_events COLUMNS run_id;

        -- Index (run_id, timestamp) for time-ordered queries
        DEFINE INDEX idx_run_id_timestamp ON TABLE run_events COLUMNS run_id, timestamp;

        -- Index event kind for filtering by event type
        DEFINE INDEX idx_kind ON TABLE run_events COLUMNS kind;

        -- Composite index (run_id, seq, timestamp) for sorted event retrieval
        DEFINE INDEX idx_run_id_seq_timestamp ON TABLE run_events COLUMNS run_id, seq, timestamp;
    "#;

    db.query(sql).await?;
    info!("✓ run_events table initialized");
    Ok(())
}

/// Initialize `releases` table with constraints and indexes
///
/// Schema:
/// ```text
/// TABLE releases {
///   agent_name:     STRING (part of uniqueness constraint)
///   spec_digest:    STRING
///   version_label:  STRING? (optional semantic version)
///   promoted_by:    STRING (who promoted this release)
///   notes:          STRING? (release notes)
///   created_at:     DATETIME (unique per agent+time)
/// }
/// ```
///
/// Semantics:
/// - Release history is append-only (new release entry for rollback)
/// - Most recent release (by created_at) is "current"
/// - Uniqueness enforced at application layer (can have same spec_digest multiple times)
async fn init_releases_table(db: &Surreal<Any>) -> Result<()> {
    debug!("Initializing releases table");

    let sql = r#"
        DEFINE TABLE releases AS
            SCHEMALESS
            PERMISSIONS
                FOR create FULL
                FOR read FULL
                FOR update NONE
                FOR delete NONE;

        -- Index agent_name for finding releases by agent
        DEFINE INDEX idx_agent_name ON TABLE releases COLUMNS agent_name;

        -- Composite index (agent_name, created_at DESC) for fast history retrieval
        -- Newer releases first
        DEFINE INDEX idx_agent_name_created_at ON TABLE releases COLUMNS agent_name, created_at DESC;

        -- Index spec_digest for reverse lookup (which agents have this version)
        DEFINE INDEX idx_spec_digest ON TABLE releases COLUMNS spec_digest;

        -- Index version_label for version-based lookups
        DEFINE INDEX idx_version_label ON TABLE releases COLUMNS version_label;
    "#;

    db.query(sql).await?;
    info!("✓ releases table initialized");
    Ok(())
}

/// Initialize `commits` table (existing, for reference)
async fn init_commits_table(db: &Surreal<Any>) -> Result<()> {
    debug!("Initializing commits table");

    let sql = r#"
        DEFINE TABLE commits AS
            SCHEMALESS
            PERMISSIONS
                FOR create FULL
                FOR read FULL
                FOR update FULL
                FOR delete NONE;

        DEFINE INDEX idx_commit_id ON TABLE commits COLUMNS commit_id UNIQUE;
        DEFINE INDEX idx_parent_id ON TABLE commits COLUMNS parent_id;
        DEFINE INDEX idx_author ON TABLE commits COLUMNS author;
        DEFINE INDEX idx_branch ON TABLE commits COLUMNS branch;
    "#;

    db.query(sql).await?;
    info!("✓ commits table initialized");
    Ok(())
}

/// Initialize `branches` table (existing, for reference)
async fn init_branches_table(db: &Surreal<Any>) -> Result<()> {
    debug!("Initializing branches table");

    let sql = r#"
        DEFINE TABLE branches AS
            SCHEMALESS
            PERMISSIONS
                FOR create FULL
                FOR read FULL
                FOR update FULL
                FOR delete FULL;

        DEFINE INDEX idx_branch_name ON TABLE branches COLUMNS name UNIQUE;
        DEFINE INDEX idx_head_commit_id ON TABLE branches COLUMNS head_commit_id;
        DEFINE INDEX idx_is_default ON TABLE branches COLUMNS is_default;
    "#;

    db.query(sql).await?;
    info!("✓ branches table initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    // Note: Full integration tests for migrations are in oxidized-state/tests/
    // These tests verify actual schema creation and constraints
}
