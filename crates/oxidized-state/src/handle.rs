//! SurrealDB Handle - Connection and Operations
//!
//! Manages connection and provides methods for:
//! - save_snapshot / load_snapshot
//! - save_commit_graph_edge
//! - get_branch_head
//! - CRUD for commits, branches, agents, memories

use crate::error::StateError;
use crate::schema::{
    AgentRecord, BranchRecord, CommitId, CommitRecord, GraphEdge, MemoryRecord, SnapshotRecord,
};
use crate::Result;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::sql::Datetime as SurrealDatetime;
use surrealdb::Surreal;
use tracing::{debug, info, instrument};

/// SurrealDB connection handle for AIVCS
pub struct SurrealHandle {
    db: Surreal<Db>,
}

impl SurrealHandle {
    /// Connect to SurrealDB and set up schema
    ///
    /// # TDD: test_surreal_connection_and_schema_creation
    #[instrument(skip_all)]
    pub async fn setup_db() -> Result<Self> {
        info!("Connecting to SurrealDB (in-memory)");

        let db = Surreal::new::<Mem>(())
            .await
            .map_err(|e| StateError::Connection(e.to_string()))?;

        // Select namespace and database
        db.use_ns("aivcs")
            .use_db("main")
            .await
            .map_err(|e| StateError::Connection(e.to_string()))?;

        let handle = SurrealHandle { db };
        handle.init_schema().await?;

        info!("SurrealDB connected and schema initialized");
        Ok(handle)
    }

    /// Initialize the database schema
    async fn init_schema(&self) -> Result<()> {
        debug!("Initializing AIVCS schema");

        // Define tables with schema
        let schema = r#"
            -- Commits table (Document Layer)
            DEFINE TABLE commits SCHEMAFULL;
            DEFINE FIELD commit_id ON commits TYPE object;
            DEFINE FIELD commit_id.hash ON commits TYPE string;
            DEFINE FIELD commit_id.logic_hash ON commits TYPE option<string>;
            DEFINE FIELD commit_id.state_hash ON commits TYPE string;
            DEFINE FIELD commit_id.env_hash ON commits TYPE option<string>;
            DEFINE FIELD parent_id ON commits TYPE option<string>;
            DEFINE FIELD message ON commits TYPE string;
            DEFINE FIELD author ON commits TYPE string;
            DEFINE FIELD created_at ON commits TYPE datetime;
            DEFINE FIELD branch ON commits TYPE option<string>;
            DEFINE INDEX idx_commit_hash ON commits FIELDS commit_id.hash UNIQUE;

            -- Snapshots table (State Data)
            DEFINE TABLE snapshots SCHEMAFULL;
            DEFINE FIELD commit_id ON snapshots TYPE string;
            DEFINE FIELD state ON snapshots FLEXIBLE TYPE object;
            DEFINE FIELD size_bytes ON snapshots TYPE int;
            DEFINE FIELD created_at ON snapshots TYPE datetime;
            DEFINE INDEX idx_snapshot_commit ON snapshots FIELDS commit_id UNIQUE;

            -- Branches table
            DEFINE TABLE branches SCHEMAFULL;
            DEFINE FIELD name ON branches TYPE string;
            DEFINE FIELD head_commit_id ON branches TYPE string;
            DEFINE FIELD is_default ON branches TYPE bool;
            DEFINE FIELD created_at ON branches TYPE datetime;
            DEFINE FIELD updated_at ON branches TYPE datetime;
            DEFINE INDEX idx_branch_name ON branches FIELDS name UNIQUE;

            -- Agents table
            DEFINE TABLE agents SCHEMAFULL;
            DEFINE FIELD agent_id ON agents TYPE string;
            DEFINE FIELD name ON agents TYPE string;
            DEFINE FIELD agent_type ON agents TYPE string;
            DEFINE FIELD config ON agents FLEXIBLE TYPE object;
            DEFINE FIELD created_at ON agents TYPE datetime;
            DEFINE INDEX idx_agent_id ON agents FIELDS agent_id UNIQUE;

            -- Memories table (for RAG)
            DEFINE TABLE memories SCHEMAFULL;
            DEFINE FIELD commit_id ON memories TYPE string;
            DEFINE FIELD key ON memories TYPE string;
            DEFINE FIELD content ON memories TYPE string;
            DEFINE FIELD embedding ON memories TYPE option<array>;
            DEFINE FIELD metadata ON memories FLEXIBLE TYPE object;
            DEFINE FIELD created_at ON memories TYPE datetime;
            DEFINE INDEX idx_memory_commit ON memories FIELDS commit_id;

            -- Graph edges table (for commit relationships)
            DEFINE TABLE graph_edges SCHEMAFULL;
            DEFINE FIELD child_id ON graph_edges TYPE string;
            DEFINE FIELD parent_id ON graph_edges TYPE string;
            DEFINE FIELD edge_type ON graph_edges TYPE string;
            DEFINE FIELD created_at ON graph_edges TYPE datetime;
            DEFINE INDEX idx_edge_child ON graph_edges FIELDS child_id;
            DEFINE INDEX idx_edge_parent ON graph_edges FIELDS parent_id;
        "#;

        self.db
            .query(schema)
            .await
            .map_err(|e| StateError::SchemaSetup(e.to_string()))?;

        debug!("Schema initialized successfully");
        Ok(())
    }

    // ========== Commit Operations ==========

    /// Save a new commit record
    #[instrument(skip(self, record), fields(commit_id = %record.commit_id))]
    pub async fn save_commit(&self, record: &CommitRecord) -> Result<CommitRecord> {
        debug!("Saving commit");

        // Clone to owned value to satisfy SurrealDB lifetime requirements
        let record_owned = record.clone();

        let created: Option<CommitRecord> = self
            .db
            .create("commits")
            .content(record_owned)
            .await?;

        created.ok_or_else(|| StateError::Transaction("Failed to create commit".to_string()))
    }

    /// Get a commit by its hash
    #[instrument(skip(self))]
    pub async fn get_commit(&self, commit_hash: &str) -> Result<Option<CommitRecord>> {
        debug!("Getting commit");

        let hash_owned = commit_hash.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM commits WHERE commit_id.hash = $hash")
            .bind(("hash", hash_owned))
            .await?;

        let commits: Vec<CommitRecord> = result.take(0)?;
        Ok(commits.into_iter().next())
    }

    // ========== Snapshot Operations ==========

    /// Save a snapshot (agent state)
    ///
    /// # TDD: test_snapshot_is_atomic_and_retrievable
    #[instrument(skip(self, commit_id, state))]
    pub async fn save_snapshot(&self, commit_id: &CommitId, state: serde_json::Value) -> Result<()> {
        debug!("Saving snapshot for commit {}", commit_id.short());

        let record = SnapshotRecord::new(&commit_id.hash, state);

        let _created: Option<SnapshotRecord> = self
            .db
            .create("snapshots")
            .content(record.clone())
            .await?;

        info!("Snapshot saved: {} ({} bytes)", commit_id.short(), record.size_bytes);
        Ok(())
    }

    /// Load a snapshot by commit ID
    #[instrument(skip(self))]
    pub async fn load_snapshot(&self, commit_id: &str) -> Result<SnapshotRecord> {
        debug!("Loading snapshot");

        let id_owned = commit_id.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM snapshots WHERE commit_id = $id")
            .bind(("id", id_owned))
            .await?;

        let snapshots: Vec<SnapshotRecord> = result.take(0)?;
        snapshots
            .into_iter()
            .next()
            .ok_or_else(|| StateError::CommitNotFound(commit_id.to_string()))
    }

    // ========== Graph Edge Operations ==========

    /// Save a commit graph edge (parent -> child relationship)
    ///
    /// # TDD: test_parent_child_edge_is_created
    #[instrument(skip(self))]
    pub async fn save_commit_graph_edge(&self, child_id: &str, parent_id: &str) -> Result<()> {
        debug!("Saving graph edge: {} -> {}", parent_id, child_id);

        let edge = GraphEdge::new(child_id, parent_id);

        let _created: Option<GraphEdge> = self
            .db
            .create("graph_edges")
            .content(edge)
            .await?;

        info!("Graph edge saved: {} -> {}", parent_id, child_id);
        Ok(())
    }

    /// Get parent commit ID for a given commit
    #[instrument(skip(self))]
    pub async fn get_parent(&self, child_id: &str) -> Result<Option<String>> {
        let id_owned = child_id.to_string();

        let mut result = self
            .db
            .query("SELECT parent_id FROM graph_edges WHERE child_id = $id")
            .bind(("id", id_owned))
            .await?;

        #[derive(serde::Deserialize)]
        struct ParentResult {
            parent_id: String,
        }

        let parents: Vec<ParentResult> = result.take(0)?;
        Ok(parents.into_iter().next().map(|p| p.parent_id))
    }

    /// Get all children of a commit (for branch visualization)
    #[instrument(skip(self))]
    pub async fn get_children(&self, parent_id: &str) -> Result<Vec<String>> {
        let id_owned = parent_id.to_string();

        let mut result = self
            .db
            .query("SELECT child_id FROM graph_edges WHERE parent_id = $id")
            .bind(("id", id_owned))
            .await?;

        #[derive(serde::Deserialize)]
        struct ChildResult {
            child_id: String,
        }

        let children: Vec<ChildResult> = result.take(0)?;
        Ok(children.into_iter().map(|c| c.child_id).collect())
    }

    // ========== Branch Operations ==========

    /// Create or update a branch
    #[instrument(skip(self))]
    pub async fn save_branch(&self, record: &BranchRecord) -> Result<BranchRecord> {
        debug!("Saving branch: {}", record.name);

        // Check if branch exists
        let existing = self.get_branch(&record.name).await?;

        if existing.is_some() {
            // Update existing branch
            let head = record.head_commit_id.clone();
            let now = SurrealDatetime::from(chrono::Utc::now());
            let name = record.name.clone();

            let mut result = self
                .db
                .query("UPDATE branches SET head_commit_id = $head, updated_at = $now WHERE name = $name")
                .bind(("head", head))
                .bind(("now", now))
                .bind(("name", name))
                .await?;

            let updated: Vec<BranchRecord> = result.take(0)?;
            updated
                .into_iter()
                .next()
                .ok_or_else(|| StateError::Transaction("Failed to update branch".to_string()))
        } else {
            // Create new branch - clone to owned
            let record_owned = record.clone();

            let created: Option<BranchRecord> = self
                .db
                .create("branches")
                .content(record_owned)
                .await?;

            created.ok_or_else(|| StateError::Transaction("Failed to create branch".to_string()))
        }
    }

    /// Get a branch by name
    #[instrument(skip(self))]
    pub async fn get_branch(&self, name: &str) -> Result<Option<BranchRecord>> {
        let name_owned = name.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM branches WHERE name = $name")
            .bind(("name", name_owned))
            .await?;

        let branches: Vec<BranchRecord> = result.take(0)?;
        Ok(branches.into_iter().next())
    }

    /// Get branch head commit ID
    #[instrument(skip(self))]
    pub async fn get_branch_head(&self, branch_name: &str) -> Result<String> {
        let branch = self
            .get_branch(branch_name)
            .await?
            .ok_or_else(|| StateError::BranchNotFound(branch_name.to_string()))?;

        Ok(branch.head_commit_id)
    }

    /// List all branches
    #[instrument(skip(self))]
    pub async fn list_branches(&self) -> Result<Vec<BranchRecord>> {
        let mut result = self
            .db
            .query("SELECT * FROM branches ORDER BY name")
            .await?;

        let branches: Vec<BranchRecord> = result.take(0)?;
        Ok(branches)
    }

    // ========== Agent Operations ==========

    /// Register an agent
    #[instrument(skip(self, record), fields(agent_name = %record.name))]
    pub async fn register_agent(&self, record: &AgentRecord) -> Result<AgentRecord> {
        debug!("Registering agent");

        let record_owned = record.clone();

        let created: Option<AgentRecord> = self
            .db
            .create("agents")
            .content(record_owned)
            .await?;

        created.ok_or_else(|| StateError::Transaction("Failed to register agent".to_string()))
    }

    /// Get agent by ID
    #[instrument(skip(self))]
    pub async fn get_agent(&self, agent_id: &str) -> Result<Option<AgentRecord>> {
        let id_owned = agent_id.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM agents WHERE agent_id = $id")
            .bind(("id", id_owned))
            .await?;

        let agents: Vec<AgentRecord> = result.take(0)?;
        Ok(agents.into_iter().next())
    }

    // ========== Memory Operations ==========

    /// Save a memory record
    #[instrument(skip(self, record), fields(key = %record.key))]
    pub async fn save_memory(&self, record: &MemoryRecord) -> Result<MemoryRecord> {
        debug!("Saving memory");

        let record_owned = record.clone();

        let created: Option<MemoryRecord> = self
            .db
            .create("memories")
            .content(record_owned)
            .await?;

        created.ok_or_else(|| StateError::Transaction("Failed to save memory".to_string()))
    }

    /// Get all memories for a commit
    #[instrument(skip(self))]
    pub async fn get_memories(&self, commit_id: &str) -> Result<Vec<MemoryRecord>> {
        let id_owned = commit_id.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM memories WHERE commit_id = $id ORDER BY created_at")
            .bind(("id", id_owned))
            .await?;

        let memories: Vec<MemoryRecord> = result.take(0)?;
        Ok(memories)
    }

    // ========== History Operations ==========

    /// Get commit history (walk back from a commit)
    #[instrument(skip(self))]
    pub async fn get_commit_history(&self, start_commit: &str, limit: usize) -> Result<Vec<CommitRecord>> {
        let mut history = Vec::new();
        let mut current = Some(start_commit.to_string());

        while let Some(commit_hash) = current {
            if history.len() >= limit {
                break;
            }

            if let Some(commit) = self.get_commit(&commit_hash).await? {
                current = commit.parent_id.clone();
                history.push(commit);
            } else {
                break;
            }
        }

        Ok(history)
    }

    /// Get the reasoning trace (CoT) for time-travel debugging
    ///
    /// # TDD: test_get_trace_for_commit_id_returns_correct_CoT
    #[instrument(skip(self))]
    pub async fn get_reasoning_trace(&self, commit_id: &str) -> Result<Vec<SnapshotRecord>> {
        // Get commit history
        let history = self.get_commit_history(commit_id, 100).await?;

        // Load snapshots for each commit
        let mut trace = Vec::new();
        for commit in history {
            if let Ok(snapshot) = self.load_snapshot(&commit.commit_id.hash).await {
                trace.push(snapshot);
            }
        }

        Ok(trace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_surreal_connection_and_schema_creation() {
        let handle = SurrealHandle::setup_db().await;
        assert!(handle.is_ok(), "Failed to connect: {:?}", handle.err());
    }

    #[tokio::test]
    async fn test_snapshot_is_atomic_and_retrievable() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let state = serde_json::json!({
            "agent_name": "test-agent",
            "step": 1,
            "variables": {"x": 42, "y": "hello"}
        });

        let commit_id = CommitId::from_state(serde_json::to_vec(&state).unwrap().as_slice());

        // Save snapshot
        handle.save_snapshot(&commit_id, state.clone()).await.unwrap();

        // Retrieve snapshot
        let loaded = handle.load_snapshot(&commit_id.hash).await.unwrap();

        assert_eq!(loaded.commit_id, commit_id.hash);
        assert_eq!(loaded.state, state);
    }

    #[tokio::test]
    async fn test_parent_child_edge_is_created() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let parent_id = "parent-commit-hash";
        let child_id = "child-commit-hash";

        // Save edge
        handle.save_commit_graph_edge(child_id, parent_id).await.unwrap();

        // Verify parent can be retrieved
        let parent = handle.get_parent(child_id).await.unwrap();
        assert_eq!(parent, Some(parent_id.to_string()));

        // Verify children can be retrieved
        let children = handle.get_children(parent_id).await.unwrap();
        assert!(children.contains(&child_id.to_string()));
    }

    #[tokio::test]
    async fn test_branch_operations() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let branch = BranchRecord::new("main", "commit-abc123", true);
        handle.save_branch(&branch).await.unwrap();

        // Get branch
        let loaded = handle.get_branch("main").await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().head_commit_id, "commit-abc123");

        // Get branch head
        let head = handle.get_branch_head("main").await.unwrap();
        assert_eq!(head, "commit-abc123");

        // Update branch head
        let updated_branch = BranchRecord::new("main", "commit-def456", true);
        handle.save_branch(&updated_branch).await.unwrap();

        let new_head = handle.get_branch_head("main").await.unwrap();
        assert_eq!(new_head, "commit-def456");
    }

    #[tokio::test]
    async fn test_commit_record_operations() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let commit_id = CommitId::from_state(b"test state");
        let commit = CommitRecord::new(commit_id.clone(), None, "Initial commit", "test-agent");

        // Save commit
        let saved = handle.save_commit(&commit).await.unwrap();
        assert_eq!(saved.commit_id.hash, commit_id.hash);

        // Get commit
        let loaded = handle.get_commit(&commit_id.hash).await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().message, "Initial commit");
    }
}
