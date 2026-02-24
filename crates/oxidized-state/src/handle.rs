//! SurrealDB Handle - Connection and Operations
//!
//! Manages connection and provides methods for:
//! - save_snapshot / load_snapshot
//! - save_commit_graph_edge
//! - get_branch_head
//! - CRUD for commits, branches, agents, memories, and CI records
//!
//! Supports both local (in-memory) and cloud (WebSocket) connections.

use crate::ci::{CiPipelineSpec, CiRunRecord, CiSnapshot};
use crate::error::StateError;
use crate::schema::{
    AgentRecord, BranchRecord, CommitId, CommitRecord, DecisionRecord, GraphEdge,
    MemoryProvenanceRecord, MemoryRecord, SnapshotRecord,
};
use crate::storage_traits::{ContentDigest, ReleaseMetadata, ReleaseRecord, StorageResult};
use crate::Result;
use crate::StorageError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::{Database, Root};
use surrealdb::sql::Datetime as SurrealDatetime;
use surrealdb::Surreal;
use tracing::{debug, info, instrument};

/// Configuration for SurrealDB Cloud connection
#[derive(Debug, Clone)]
pub struct CloudConfig {
    /// WebSocket endpoint URL (e.g., "wss://xxx.aws-use1.surrealdb.cloud")
    pub endpoint: String,
    /// Database username
    pub username: String,
    /// Database password
    pub password: String,
    /// Namespace (default: "aivcs")
    pub namespace: String,
    /// Database name (default: "main")
    pub database: String,
    /// Whether this is a root user (true) or database user (false)
    pub is_root: bool,
}

impl CloudConfig {
    /// Create a new cloud configuration for a database user
    pub fn new(
        endpoint: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            username: username.into(),
            password: password.into(),
            namespace: "aivcs".to_string(),
            database: "main".to_string(),
            is_root: false,
        }
    }

    /// Create a new cloud configuration for a root user
    pub fn new_root(
        endpoint: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            username: username.into(),
            password: password.into(),
            namespace: "aivcs".to_string(),
            database: "main".to_string(),
            is_root: true,
        }
    }

    /// Set custom namespace
    pub fn with_namespace(mut self, ns: impl Into<String>) -> Self {
        self.namespace = ns.into();
        self
    }

    /// Set custom database
    pub fn with_database(mut self, db: impl Into<String>) -> Self {
        self.database = db.into();
        self
    }

    /// Set whether this is a root user
    pub fn with_root(mut self, is_root: bool) -> Self {
        self.is_root = is_root;
        self
    }

    /// Create from environment variables
    ///
    /// Reads:
    /// - SURREALDB_ENDPOINT (required)
    /// - SURREALDB_USERNAME (required)
    /// - SURREALDB_PASSWORD (required)
    /// - SURREALDB_NAMESPACE (optional, default: "aivcs")
    /// - SURREALDB_DATABASE (optional, default: "main")
    /// - SURREALDB_ROOT (optional, default: "false") - set to "true" for root users
    pub fn from_env() -> std::result::Result<Self, String> {
        let endpoint =
            std::env::var("SURREALDB_ENDPOINT").map_err(|_| "SURREALDB_ENDPOINT not set")?;
        let username =
            std::env::var("SURREALDB_USERNAME").map_err(|_| "SURREALDB_USERNAME not set")?;
        let password =
            std::env::var("SURREALDB_PASSWORD").map_err(|_| "SURREALDB_PASSWORD not set")?;
        let namespace =
            std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "aivcs".to_string());
        let database = std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "main".to_string());
        let is_root = std::env::var("SURREALDB_ROOT")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        Ok(Self {
            endpoint,
            username,
            password,
            namespace,
            database,
            is_root,
        })
    }
}

/// SurrealDB connection handle for AIVCS
#[derive(Clone)]
pub struct SurrealHandle {
    db: Surreal<Any>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbReleaseRecord {
    name: String,
    spec_digest: ContentDigest,
    metadata: ReleaseMetadata,
    version_label: Option<String>,
    promoted_by: String,
    notes: Option<String>,
    created_at: SurrealDatetime,
}

impl DbReleaseRecord {
    fn into_release_record(self) -> ReleaseRecord {
        ReleaseRecord {
            name: self.name,
            spec_digest: self.spec_digest,
            metadata: self.metadata,
            created_at: DateTime::<Utc>::from(self.created_at),
        }
    }
}

impl SurrealHandle {
    /// Connect to SurrealDB in-memory and set up schema
    ///
    /// # TDD: test_surreal_connection_and_schema_creation
    #[instrument(skip_all)]
    pub async fn setup_db() -> Result<Self> {
        info!("Connecting to SurrealDB (in-memory)");

        let db = surrealdb::engine::any::connect("mem://")
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

    /// Connect to SurrealDB Cloud
    ///
    /// # Example
    /// ```ignore
    /// let config = CloudConfig::new(
    ///     "wss://xxx.aws-use1.surrealdb.cloud",
    ///     "your_username",
    ///     "your_password",
    /// );
    /// let handle = SurrealHandle::setup_cloud(config).await?;
    /// ```
    #[instrument(skip(config), fields(endpoint = %config.endpoint, namespace = %config.namespace, database = %config.database))]
    pub async fn setup_cloud(config: CloudConfig) -> Result<Self> {
        info!("Connecting to SurrealDB Cloud (root={})", config.is_root);

        let db = surrealdb::engine::any::connect(&config.endpoint)
            .await
            .map_err(|e| {
                StateError::Connection(format!("Failed to connect to {}: {}", config.endpoint, e))
            })?;

        // Authenticate based on user type
        if config.is_root {
            // Root user authentication
            db.signin(Root {
                username: &config.username,
                password: &config.password,
            })
            .await
            .map_err(|e| StateError::Connection(format!("Root authentication failed: {}", e)))?;
        } else {
            // Database user authentication - requires namespace and database
            db.signin(Database {
                namespace: &config.namespace,
                database: &config.database,
                username: &config.username,
                password: &config.password,
            })
            .await
            .map_err(|e| {
                StateError::Connection(format!("Database authentication failed: {}", e))
            })?;
        }

        // Select namespace and database
        db.use_ns(&config.namespace)
            .use_db(&config.database)
            .await
            .map_err(|e| {
                StateError::Connection(format!("Failed to select namespace/database: {}", e))
            })?;

        let handle = SurrealHandle { db };
        handle.init_schema().await?;

        info!("SurrealDB Cloud connected and schema initialized");
        Ok(handle)
    }

    /// Connect using environment variables
    ///
    /// If SURREALDB_ENDPOINT is set, connects to cloud.
    /// If SURREALDB_URL is set, connects to that URL.
    /// Otherwise, falls back to local persistence in `.aivcs/db` using SurrealKV.
    #[instrument(skip_all)]
    pub async fn setup_from_env() -> Result<Self> {
        if let Ok(config) = CloudConfig::from_env() {
            info!("Cloud config found, connecting to SurrealDB Cloud");
            return Self::setup_cloud(config).await;
        }

        let url = if let Ok(url) = std::env::var("SURREALDB_URL") {
            info!("SURREALDB_URL found, connecting to {}", url);
            url
        } else {
            // Default to local persistence in .aivcs/db
            let path = ".aivcs/db";
            std::fs::create_dir_all(path).map_err(|e| {
                StateError::Connection(format!(
                    "Failed to create database directory {}: {}",
                    path, e
                ))
            })?;
            let url = format!("surrealkv://{}", path);
            info!(
                "No cloud config or SURREALDB_URL found, using local persistence: {}",
                url
            );
            url
        };

        let db = surrealdb::engine::any::connect(&url)
            .await
            .map_err(|e| StateError::Connection(format!("Failed to connect to {}: {}", url, e)))?;

        db.use_ns("aivcs")
            .use_db("main")
            .await
            .map_err(|e| StateError::Connection(e.to_string()))?;

        let handle = SurrealHandle { db };
        handle.init_schema().await?;
        Ok(handle)
    }

    /// Initialize the database schema
    async fn init_schema(&self) -> Result<()> {
        crate::migrations::init_schema(&self.db).await?;
        Ok(())
    }

    // ========== Commit Operations ==========

    /// Save a new commit record
    #[instrument(skip(self, record), fields(commit_id = %record.commit_id))]
    pub async fn save_commit(&self, record: &CommitRecord) -> Result<CommitRecord> {
        debug!("Saving commit");

        // Clone to owned value to satisfy SurrealDB lifetime requirements
        let record_owned = record.clone();

        let created: Option<CommitRecord> = self.db.create("commits").content(record_owned).await?;

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
    pub async fn save_snapshot(
        &self,
        commit_id: &CommitId,
        state: serde_json::Value,
    ) -> Result<()> {
        debug!("Saving snapshot for commit {}", commit_id.short());

        let record = SnapshotRecord::new(&commit_id.hash, state);

        let _created: Option<SnapshotRecord> =
            self.db.create("snapshots").content(record.clone()).await?;

        info!(
            "Snapshot saved: {} ({} bytes)",
            commit_id.short(),
            record.size_bytes
        );
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

        let _created: Option<GraphEdge> = self.db.create("graph_edges").content(edge).await?;

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

            let created: Option<BranchRecord> =
                self.db.create("branches").content(record_owned).await?;

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

    /// Delete a branch
    #[instrument(skip(self))]
    pub async fn delete_branch(&self, name: &str) -> Result<()> {
        debug!("Deleting branch: {}", name);

        let branch = self
            .get_branch(name)
            .await?
            .ok_or_else(|| StateError::BranchNotFound(name.to_string()))?;

        if branch.is_default {
            return Err(StateError::Transaction(
                "Cannot delete the default branch".to_string(),
            ));
        }

        let name_owned = name.to_string();

        let _result = self
            .db
            .query("DELETE FROM branches WHERE name = $name")
            .bind(("name", name_owned))
            .await?;

        Ok(())
    }

    // ========== Agent Operations ==========

    /// Register an agent
    #[instrument(skip(self, record), fields(agent_name = %record.name))]
    pub async fn register_agent(&self, record: &AgentRecord) -> Result<AgentRecord> {
        debug!("Registering agent");

        let record_owned = record.clone();

        let created: Option<AgentRecord> = self.db.create("agents").content(record_owned).await?;

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

        let created: Option<MemoryRecord> =
            self.db.create("memories").content(record_owned).await?;

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

    // ========== Release Registry Operations ==========

    /// Promote a new release for an agent.
    #[instrument(skip(self, spec_digest, metadata), fields(name = %name, digest = %spec_digest))]
    pub async fn release_promote(
        &self,
        name: &str,
        spec_digest: &ContentDigest,
        metadata: ReleaseMetadata,
    ) -> StorageResult<ReleaseRecord> {
        let record = DbReleaseRecord {
            name: name.to_string(),
            spec_digest: spec_digest.clone(),
            version_label: metadata.version_label.clone(),
            promoted_by: metadata.promoted_by.clone(),
            notes: metadata.notes.clone(),
            metadata,
            created_at: SurrealDatetime::from(Utc::now()),
        };

        let created: Option<DbReleaseRecord> = self
            .db
            .create("releases")
            .content(record.clone())
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        created
            .map(DbReleaseRecord::into_release_record)
            .ok_or_else(|| StorageError::Backend("failed to create release record".to_string()))
    }

    /// Roll back to the previous release for an agent by re-appending it.
    #[instrument(skip(self), fields(name = %name))]
    pub async fn release_rollback(&self, name: &str) -> StorageResult<ReleaseRecord> {
        let history = self.release_history(name).await?;
        if history.is_empty() {
            return Err(StorageError::ReleaseNotFound {
                name: name.to_string(),
            });
        }
        if history.len() < 2 {
            return Err(StorageError::NoPreviousRelease {
                name: name.to_string(),
            });
        }

        let previous = &history[1];
        self.release_promote(name, &previous.spec_digest, previous.metadata.clone())
            .await
    }

    /// Get the current release (most recent) for an agent.
    #[instrument(skip(self), fields(name = %name))]
    pub async fn release_current(&self, name: &str) -> StorageResult<Option<ReleaseRecord>> {
        let name_owned = name.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM releases WHERE name = $name ORDER BY created_at DESC LIMIT 1")
            .bind(("name", name_owned))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        let releases: Vec<DbReleaseRecord> = result
            .take(0)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(releases
            .into_iter()
            .next()
            .map(DbReleaseRecord::into_release_record))
    }

    /// Get full release history (newest first) for an agent.
    #[instrument(skip(self), fields(name = %name))]
    pub async fn release_history(&self, name: &str) -> StorageResult<Vec<ReleaseRecord>> {
        let name_owned = name.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM releases WHERE name = $name ORDER BY created_at DESC")
            .bind(("name", name_owned))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        let releases: Vec<DbReleaseRecord> = result
            .take(0)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(releases
            .into_iter()
            .map(DbReleaseRecord::into_release_record)
            .collect())
    }

    // ========== CI Operations ==========

    /// Save a CI snapshot as a content-addressed object.
    #[instrument(skip(self, snapshot))]
    pub async fn save_ci_snapshot(&self, snapshot: &CiSnapshot) -> Result<String> {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct CiSnapshotStore {
            digest: String,
            snapshot_json: String,
        }

        let digest = snapshot.digest();
        let snapshot_json = serde_json::to_string(snapshot)?;
        let payload = CiSnapshotStore {
            digest: digest.clone(),
            snapshot_json,
        };

        let _created: Option<CiSnapshotStore> =
            self.db.create("ci_snapshots").content(payload).await?;
        Ok(digest)
    }

    /// Load a CI snapshot by digest.
    #[instrument(skip(self))]
    pub async fn load_ci_snapshot(&self, digest: &str) -> Result<Option<CiSnapshot>> {
        #[derive(serde::Deserialize)]
        struct CiSnapshotStore {
            snapshot_json: String,
        }

        let digest_owned = digest.to_string();
        let mut result = self
            .db
            .query("SELECT snapshot_json FROM ci_snapshots WHERE digest = $digest")
            .bind(("digest", digest_owned))
            .await?;

        let rows: Vec<CiSnapshotStore> = result.take(0)?;
        rows.into_iter()
            .next()
            .map(|r| serde_json::from_str(&r.snapshot_json))
            .transpose()
            .map_err(Into::into)
    }

    /// Save a CI pipeline as a content-addressed object.
    #[instrument(skip(self, pipeline))]
    pub async fn save_ci_pipeline(&self, pipeline: &CiPipelineSpec) -> Result<String> {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct CiPipelineStore {
            digest: String,
            pipeline_json: String,
        }

        let digest = pipeline.digest();
        let pipeline_json = serde_json::to_string(pipeline)?;
        let payload = CiPipelineStore {
            digest: digest.clone(),
            pipeline_json,
        };

        let _created: Option<CiPipelineStore> =
            self.db.create("ci_pipelines").content(payload).await?;
        Ok(digest)
    }

    /// Load a CI pipeline by digest.
    #[instrument(skip(self))]
    pub async fn load_ci_pipeline(&self, digest: &str) -> Result<Option<CiPipelineSpec>> {
        #[derive(serde::Deserialize)]
        struct CiPipelineStore {
            pipeline_json: String,
        }

        let digest_owned = digest.to_string();
        let mut result = self
            .db
            .query("SELECT pipeline_json FROM ci_pipelines WHERE digest = $digest")
            .bind(("digest", digest_owned))
            .await?;

        let rows: Vec<CiPipelineStore> = result.take(0)?;
        rows.into_iter()
            .next()
            .map(|r| serde_json::from_str(&r.pipeline_json))
            .transpose()
            .map_err(Into::into)
    }

    /// Save a CI run record.
    #[instrument(skip(self, run), fields(run_id = %run.run_id))]
    pub async fn save_ci_run(&self, run: &CiRunRecord) -> Result<CiRunRecord> {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct CiRunStore {
            run_id: String,
            snapshot_digest: String,
            pipeline_digest: String,
            status: String,
            run_json: String,
            started_at: Option<String>,
            finished_at: Option<String>,
        }

        let payload = CiRunStore {
            run_id: run.run_id.clone(),
            snapshot_digest: run.snapshot_digest.clone(),
            pipeline_digest: run.pipeline_digest.clone(),
            status: serde_json::to_string(&run.status)?
                .trim_matches('"')
                .to_string(),
            run_json: serde_json::to_string(run)?,
            started_at: run.started_at.clone(),
            finished_at: run.finished_at.clone(),
        };

        let created: Option<CiRunStore> = self.db.create("ci_runs").content(payload).await?;
        if created.is_some() {
            Ok(run.clone())
        } else {
            Err(StateError::Transaction(
                "Failed to create CI run".to_string(),
            ))
        }
    }

    /// Get a CI run by run ID.
    #[instrument(skip(self))]
    pub async fn get_ci_run(&self, run_id: &str) -> Result<Option<CiRunRecord>> {
        #[derive(serde::Deserialize)]
        struct CiRunStore {
            run_json: String,
        }

        let run_id_owned = run_id.to_string();
        let mut result = self
            .db
            .query("SELECT run_json FROM ci_runs WHERE run_id = $run_id")
            .bind(("run_id", run_id_owned))
            .await?;
        let runs: Vec<CiRunStore> = result.take(0)?;
        runs.into_iter()
            .next()
            .map(|r| serde_json::from_str(&r.run_json))
            .transpose()
            .map_err(Into::into)
    }

    /// List CI runs for a given snapshot digest.
    #[instrument(skip(self))]
    pub async fn list_ci_runs_by_snapshot(
        &self,
        snapshot_digest: &str,
    ) -> Result<Vec<CiRunRecord>> {
        #[derive(serde::Deserialize)]
        struct CiRunStore {
            run_json: String,
        }

        let snapshot_digest_owned = snapshot_digest.to_string();
        let mut result = self
            .db
            .query("SELECT run_json FROM ci_runs WHERE snapshot_digest = $snapshot_digest")
            .bind(("snapshot_digest", snapshot_digest_owned))
            .await?;
        let runs: Vec<CiRunStore> = result.take(0)?;
        runs.into_iter()
            .map(|r| serde_json::from_str::<CiRunRecord>(&r.run_json))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    // ========== Decision and Provenance Operations (EPIC5) ==========

    /// Save a decision record
    #[instrument(skip(self, record))]
    pub async fn save_decision(&self, record: &DecisionRecord) -> Result<DecisionRecord> {
        debug!("Saving decision");

        let record_owned = record.clone();

        let created: Option<DecisionRecord> =
            self.db.create("decisions").content(record_owned).await?;

        created.ok_or_else(|| StateError::Transaction("Failed to save decision".to_string()))
    }

    /// Get decision by decision ID
    #[instrument(skip(self))]
    pub async fn get_decision(&self, decision_id: &str) -> Result<Option<DecisionRecord>> {
        let id_owned = decision_id.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM decisions WHERE decision_id = $id")
            .bind(("id", id_owned))
            .await?;

        let decisions: Vec<DecisionRecord> = result.take(0)?;
        Ok(decisions.into_iter().next())
    }

    /// Get decision history for a task
    #[instrument(skip(self))]
    pub async fn get_decision_history(
        &self,
        task: &str,
        limit: usize,
    ) -> Result<Vec<DecisionRecord>> {
        let task_owned = task.to_string();

        let mut result = self
            .db
            .query(
                "SELECT * FROM decisions WHERE task = $task ORDER BY timestamp DESC LIMIT $limit",
            )
            .bind(("task", task_owned))
            .bind(("limit", limit as i64))
            .await?;

        let decisions: Vec<DecisionRecord> = result.take(0)?;
        Ok(decisions)
    }

    /// Save a memory provenance record
    #[instrument(skip(self, record))]
    pub async fn save_provenance(
        &self,
        record: &MemoryProvenanceRecord,
    ) -> Result<MemoryProvenanceRecord> {
        debug!("Saving memory provenance");

        let record_owned = record.clone();

        let created: Option<MemoryProvenanceRecord> = self
            .db
            .create("memory_provenances")
            .content(record_owned)
            .await?;

        created.ok_or_else(|| StateError::Transaction("Failed to save provenance".to_string()))
    }

    /// Get provenance records for a memory
    #[instrument(skip(self))]
    pub async fn get_provenance(&self, memory_id: &str) -> Result<Vec<MemoryProvenanceRecord>> {
        let memory_id_owned = memory_id.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM memory_provenances WHERE memory_id = $memory_id")
            .bind(("memory_id", memory_id_owned))
            .await?;

        let provenances: Vec<MemoryProvenanceRecord> = result.take(0)?;
        Ok(provenances)
    }

    // ========== History Operations ==========

    /// Get commit history (walk back from a commit)
    #[instrument(skip(self))]
    pub async fn get_commit_history(
        &self,
        start_commit: &str,
        limit: usize,
    ) -> Result<Vec<CommitRecord>> {
        let mut history = Vec::new();
        let mut current = Some(start_commit.to_string());

        while let Some(commit_hash) = current {
            if history.len() >= limit {
                break;
            }

            if let Some(commit) = self.get_commit(&commit_hash).await? {
                // For linear history, we follow the first parent
                current = commit.parent_ids.first().cloned();
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
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn test_surreal_connection_and_schema_creation() {
        let handle = SurrealHandle::setup_db().await;
        assert!(handle.is_ok(), "Failed to connect: {:?}", handle.err());
    }

    #[tokio::test]
    async fn test_branch_deletion() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        // Create a branch
        let branch = BranchRecord::new("feature/test", "commit-123", false);
        handle.save_branch(&branch).await.unwrap();

        // Verify it exists
        let loaded = handle.get_branch("feature/test").await.unwrap();
        assert!(loaded.is_some());

        // Delete it
        handle.delete_branch("feature/test").await.unwrap();

        // Verify it's gone
        let deleted = handle.get_branch("feature/test").await.unwrap();
        assert!(deleted.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_branch() {
        let handle = SurrealHandle::setup_db().await.unwrap();
        let result = handle.delete_branch("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Branch not found"));
    }

    #[tokio::test]
    async fn test_delete_default_branch() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        // Create default branch
        let branch = BranchRecord::new("main", "commit-123", true);
        handle.save_branch(&branch).await.unwrap();

        let result = handle.delete_branch("main").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Cannot delete the default branch"));
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
        handle
            .save_snapshot(&commit_id, state.clone())
            .await
            .unwrap();

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
        handle
            .save_commit_graph_edge(child_id, parent_id)
            .await
            .unwrap();

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
        let commit = CommitRecord::new(commit_id.clone(), vec![], "Initial commit", "test-agent");

        // Save commit
        let saved = handle.save_commit(&commit).await.unwrap();
        assert_eq!(saved.commit_id.hash, commit_id.hash);

        // Get commit
        let loaded = handle.get_commit(&commit_id.hash).await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().message, "Initial commit");
    }

    #[tokio::test]
    async fn test_get_trace_for_commit_id_returns_correct_cot() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        // Create a chain of commits: initial -> step1 -> step2 -> step3
        let state_0 = serde_json::json!({"step": 0, "thought": "Starting exploration"});
        let state_1 = serde_json::json!({"step": 1, "thought": "Trying strategy A"});
        let state_2 = serde_json::json!({"step": 2, "thought": "Strategy A failed, pivoting"});
        let state_3 = serde_json::json!({"step": 3, "thought": "Strategy B succeeded"});

        let id_0 = CommitId::from_state(b"state-0");
        let id_1 = CommitId::from_state(b"state-1");
        let id_2 = CommitId::from_state(b"state-2");
        let id_3 = CommitId::from_state(b"state-3");

        // Save snapshots
        handle.save_snapshot(&id_0, state_0.clone()).await.unwrap();
        handle.save_snapshot(&id_1, state_1.clone()).await.unwrap();
        handle.save_snapshot(&id_2, state_2.clone()).await.unwrap();
        handle.save_snapshot(&id_3, state_3.clone()).await.unwrap();

        // Save commits with parent chain
        let commit_0 = CommitRecord::new(id_0.clone(), vec![], "Step 0", "agent");
        let commit_1 = CommitRecord::new(id_1.clone(), vec![id_0.hash.clone()], "Step 1", "agent");
        let commit_2 = CommitRecord::new(id_2.clone(), vec![id_1.hash.clone()], "Step 2", "agent");
        let commit_3 = CommitRecord::new(id_3.clone(), vec![id_2.hash.clone()], "Step 3", "agent");

        handle.save_commit(&commit_0).await.unwrap();
        handle.save_commit(&commit_1).await.unwrap();
        handle.save_commit(&commit_2).await.unwrap();
        handle.save_commit(&commit_3).await.unwrap();

        // Get reasoning trace from step 3
        let trace = handle.get_reasoning_trace(&id_3.hash).await.unwrap();

        // Should have 4 snapshots in reverse order (newest first)
        assert_eq!(trace.len(), 4, "Trace should contain all 4 commits");

        // Verify order (most recent first)
        assert_eq!(trace[0].state["step"], 3);
        assert_eq!(trace[1].state["step"], 2);
        assert_eq!(trace[2].state["step"], 1);
        assert_eq!(trace[3].state["step"], 0);

        // Verify Chain-of-Thought is preserved
        assert_eq!(trace[0].state["thought"], "Strategy B succeeded");
        assert_eq!(trace[1].state["thought"], "Strategy A failed, pivoting");
        assert_eq!(trace[2].state["thought"], "Trying strategy A");
        assert_eq!(trace[3].state["thought"], "Starting exploration");
    }

    #[tokio::test]
    async fn test_ci_records_roundtrip() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let snapshot = CiSnapshot {
            repo_sha: "abc123".to_string(),
            workspace_hash: "work-1".to_string(),
            local_ci_config_hash: "cfg-1".to_string(),
            env_hash: "env-1".to_string(),
        };
        let snapshot_digest = handle.save_ci_snapshot(&snapshot).await.unwrap();
        let loaded_snapshot = handle.load_ci_snapshot(&snapshot_digest).await.unwrap();
        assert_eq!(loaded_snapshot, Some(snapshot.clone()));

        let pipeline = CiPipelineSpec {
            name: "default".to_string(),
            steps: vec![crate::ci::CiStepSpec {
                name: "test".to_string(),
                command: crate::ci::CiCommand {
                    program: "cargo".to_string(),
                    args: vec!["test".to_string()],
                    env: BTreeMap::new(),
                    cwd: None,
                },
                timeout_secs: Some(300),
                allow_failure: false,
            }],
        };
        let pipeline_digest = handle.save_ci_pipeline(&pipeline).await.unwrap();
        let loaded_pipeline = handle.load_ci_pipeline(&pipeline_digest).await.unwrap();
        assert_eq!(loaded_pipeline, Some(pipeline.clone()));

        let run = CiRunRecord::queued(&snapshot_digest, &pipeline_digest);
        let saved_run = handle.save_ci_run(&run).await.unwrap();
        let loaded_run = handle.get_ci_run(&saved_run.run_id).await.unwrap();
        assert_eq!(loaded_run, Some(saved_run.clone()));

        let runs = handle
            .list_ci_runs_by_snapshot(&snapshot_digest)
            .await
            .unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, saved_run.run_id);
    }

    #[tokio::test]
    async fn test_release_fields_roundtrip_in_surreal() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        let metadata = ReleaseMetadata {
            version_label: Some("v1.2.3".to_string()),
            promoted_by: "test-user".to_string(),
            notes: Some("Release notes here".to_string()),
        };
        let digest = ContentDigest::from_bytes(b"spec-data");

        // Promote release
        let release = handle
            .release_promote("test-agent", &digest, metadata.clone())
            .await
            .unwrap();

        assert_eq!(release.name, "test-agent");
        assert_eq!(release.metadata.version_label, Some("v1.2.3".to_string()));

        // Check raw DB record to ensure top-level fields are set (since table is SCHEMAFULL)
        let mut result = handle
            .db
            .query("SELECT name, version_label, promoted_by, notes FROM releases WHERE name = 'test-agent'")
            .await
            .unwrap();

        #[derive(serde::Deserialize)]
        struct RawRelease {
            name: String,
            version_label: Option<String>,
            promoted_by: String,
            notes: Option<String>,
        }

        let rows: Vec<RawRelease> = result.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];

        assert_eq!(row.version_label, Some("v1.2.3".to_string()));
        assert_eq!(row.promoted_by, "test-user");
        assert_eq!(row.notes, Some("Release notes here".to_string()));
        assert_eq!(row.name, "test-agent");

        // Verify history roundtrip
        let history = handle.release_history("test-agent").await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].metadata.version_label,
            Some("v1.2.3".to_string())
        );
        assert_eq!(history[0].metadata.promoted_by, "test-user");
    }
}
