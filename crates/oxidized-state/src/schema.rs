//! Schema definitions for AIVCS SurrealDB tables
//!
//! Tables:
//! - commits: Version control commits (graph nodes)
//! - branches: Branch pointers to commit IDs
//! - agents: Registered agent metadata
//! - memories: Agent memory/context snapshots

use chrono::{DateTime, Utc};

/// Module for serializing chrono DateTime to SurrealDB datetime format
mod surreal_datetime {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};
    use surrealdb::sql::Datetime as SurrealDatetime;

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let sd = SurrealDatetime::from(*date);
        serde::Serialize::serialize(&sd, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sd = SurrealDatetime::deserialize(deserializer)?;
        Ok(DateTime::from(sd))
    }
}

/// Module for serializing optional chrono DateTime to SurrealDB datetime format
mod surreal_datetime_opt {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};
    use surrealdb::sql::Datetime as SurrealDatetime;

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(d) => {
                let sd = SurrealDatetime::from(*d);
                serde::Serialize::serialize(&Some(sd), serializer)
            }
            None => serde::Serialize::serialize(&None::<SurrealDatetime>, serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sd = Option::<SurrealDatetime>::deserialize(deserializer)?;
        Ok(sd.map(DateTime::from))
    }
}
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Composite Commit ID - hash of (Logic + State + Environment)
///
/// A commit in AIVCS is a tuple of:
/// 1. Logic: The Rust binaries/scripts hash
/// 2. State: The agent state snapshot hash
/// 3. Environment: The Nix Flake hash
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommitId {
    /// The composite hash
    pub hash: String,
    /// Logic hash component
    pub logic_hash: Option<String>,
    /// State hash component
    pub state_hash: String,
    /// Environment (Nix) hash component
    pub env_hash: Option<String>,
}

impl CommitId {
    /// Create a new CommitId from state only (Phase 1 MVP)
    pub fn from_state(state: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(state);
        let state_hash = hex::encode(hasher.finalize());

        // Consistent with new(None, state_hash, None)
        Self::new(None, &state_hash, None)
    }

    /// Create a new CommitId from a JSON value, ensuring canonical key ordering.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let bytes = if let Some(obj) = value.as_object() {
            // Force BTreeMap sorting even if preserve_order is enabled
            let sorted: std::collections::BTreeMap<_, _> = obj.iter().collect();
            serde_json::to_vec(&sorted).unwrap_or_else(|_| serde_json::to_vec(value).unwrap())
        } else {
            serde_json::to_vec(value).unwrap()
        };
        Self::from_state(&bytes)
    }

    /// Create a full composite CommitId (Phase 2+)
    pub fn new(logic_hash: Option<&str>, state_hash: &str, env_hash: Option<&str>) -> Self {
        let mut hasher = Sha256::new();

        // Use markers and separators to prevent hash collisions
        // Logic component
        hasher.update(b"L");
        if let Some(lh) = logic_hash {
            hasher.update(b"S");
            hasher.update(lh.as_bytes());
        } else {
            hasher.update(b"N");
        }
        hasher.update(b"\0");

        // State component (always present)
        hasher.update(b"S:");
        hasher.update(state_hash.as_bytes());
        hasher.update(b"\0");

        // Environment component
        hasher.update(b"E");
        if let Some(eh) = env_hash {
            hasher.update(b"S");
            hasher.update(eh.as_bytes());
        } else {
            hasher.update(b"N");
        }

        let composite = hex::encode(hasher.finalize());

        CommitId {
            hash: composite,
            logic_hash: logic_hash.map(String::from),
            state_hash: state_hash.to_string(),
            env_hash: env_hash.map(String::from),
        }
    }

    /// Get short hash (first 8 characters)
    pub fn short(&self) -> String {
        self.hash.chars().take(8).collect()
    }
}

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.hash)
    }
}

/// Commit record stored in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// The commit ID (composite hash)
    pub commit_id: CommitId,
    /// Parent commit IDs (empty for root commits)
    pub parent_ids: Vec<String>,
    /// Commit message
    pub message: String,
    /// Author/agent that created the commit
    pub author: String,
    /// Timestamp of commit creation
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
    /// Branch name (if this is a branch head)
    pub branch: Option<String>,
}

impl CommitRecord {
    /// Create a new commit record
    pub fn new(commit_id: CommitId, parent_ids: Vec<String>, message: &str, author: &str) -> Self {
        CommitRecord {
            id: None,
            commit_id,
            parent_ids,
            message: message.to_string(),
            author: author.to_string(),
            created_at: Utc::now(),
            branch: None,
        }
    }
}

/// Snapshot record - the actual agent state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// The commit ID this snapshot belongs to
    pub commit_id: String,
    /// Serialized agent state (JSON)
    pub state: serde_json::Value,
    /// Size in bytes
    pub size_bytes: u64,
    /// Timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
}

impl SnapshotRecord {
    /// Create a new snapshot record
    pub fn new(commit_id: &str, state: serde_json::Value) -> Self {
        let size = serde_json::to_string(&state)
            .map(|s| s.len() as u64)
            .unwrap_or(0);

        SnapshotRecord {
            id: None,
            commit_id: commit_id.to_string(),
            state,
            size_bytes: size,
            created_at: Utc::now(),
        }
    }
}

/// Branch record - pointer to a commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// Branch name (e.g., "main", "feature/experiment-1")
    pub name: String,
    /// Current head commit ID
    pub head_commit_id: String,
    /// Is this the default branch?
    pub is_default: bool,
    /// Created timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
    /// Last updated timestamp
    #[serde(with = "surreal_datetime")]
    pub updated_at: DateTime<Utc>,
}

impl BranchRecord {
    /// Create a new branch record
    pub fn new(name: &str, head_commit_id: &str, is_default: bool) -> Self {
        let now = Utc::now();
        BranchRecord {
            id: None,
            name: name.to_string(),
            head_commit_id: head_commit_id.to_string(),
            is_default,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Agent record - registered agent metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// Agent UUID
    pub agent_id: Uuid,
    /// Agent name
    pub name: String,
    /// Agent type/kind
    pub agent_type: String,
    /// Configuration (JSON)
    pub config: serde_json::Value,
    /// Created timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
}

impl AgentRecord {
    /// Create a new agent record
    pub fn new(name: &str, agent_type: &str, config: serde_json::Value) -> Self {
        AgentRecord {
            id: None,
            agent_id: Uuid::new_v4(),
            name: name.to_string(),
            agent_type: agent_type.to_string(),
            config,
            created_at: Utc::now(),
        }
    }
}

/// Memory record - agent memory/context for RAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// The commit ID this memory belongs to
    pub commit_id: String,
    /// Memory key/namespace
    pub key: String,
    /// Memory content (text for embedding)
    pub content: String,
    /// Optional embedding vector (for semantic search)
    pub embedding: Option<Vec<f32>>,
    /// Metadata
    pub metadata: serde_json::Value,
    /// Created timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
}

impl MemoryRecord {
    /// Create a new memory record
    pub fn new(commit_id: &str, key: &str, content: &str) -> Self {
        MemoryRecord {
            id: None,
            commit_id: commit_id.to_string(),
            key: key.to_string(),
            content: content.to_string(),
            embedding: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        }
    }

    /// Set embedding vector
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Set metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Graph edge - represents commit relationships (parent -> child)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Child commit ID
    pub child_id: String,
    /// Parent commit ID
    pub parent_id: String,
    /// Edge type (normal, merge, fork)
    pub edge_type: EdgeType,
    /// Created timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
}

/// Type of graph edge
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType {
    /// Normal parent-child relationship
    Normal,
    /// Merge commit (multiple parents)
    Merge,
    /// Fork/branch point
    Fork,
}

impl GraphEdge {
    /// Create a new normal graph edge
    pub fn new(child_id: &str, parent_id: &str) -> Self {
        GraphEdge {
            child_id: child_id.to_string(),
            parent_id: parent_id.to_string(),
            edge_type: EdgeType::Normal,
            created_at: Utc::now(),
        }
    }

    /// Create a merge edge
    pub fn merge(child_id: &str, parent_id: &str) -> Self {
        GraphEdge {
            child_id: child_id.to_string(),
            parent_id: parent_id.to_string(),
            edge_type: EdgeType::Merge,
            created_at: Utc::now(),
        }
    }

    /// Create a fork edge
    pub fn fork(child_id: &str, parent_id: &str) -> Self {
        GraphEdge {
            child_id: child_id.to_string(),
            parent_id: parent_id.to_string(),
            edge_type: EdgeType::Fork,
            created_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// RunLedger Records — Execution Run Persistence
// ---------------------------------------------------------------------------

/// Run record - execution run metadata and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// Unique run ID (UUID string)
    pub run_id: String,
    /// Agent spec digest (SHA256)
    pub spec_digest: String,
    /// Git SHA at time of run (optional)
    pub git_sha: Option<String>,
    /// Agent name
    pub agent_name: String,
    /// Arbitrary tags (JSON)
    pub tags: serde_json::Value,
    /// Run status: "RUNNING" | "COMPLETED" | "FAILED" | "CANCELLED"
    pub status: String,
    /// Total events recorded
    pub total_events: u64,
    /// Final state digest (if completed)
    pub final_state_digest: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Whether run succeeded
    pub success: bool,
    /// Created timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
    /// Completed timestamp (if terminal)
    #[serde(default, with = "surreal_datetime_opt")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl RunRecord {
    /// Create a new run record in "running" state
    pub fn new(
        run_id: String,
        spec_digest: String,
        git_sha: Option<String>,
        agent_name: String,
        tags: serde_json::Value,
    ) -> Self {
        RunRecord {
            id: None,
            run_id,
            spec_digest,
            git_sha,
            agent_name,
            tags,
            status: "RUNNING".to_string(),
            total_events: 0,
            final_state_digest: None,
            duration_ms: 0,
            success: false,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    /// Mark run as completed
    pub fn complete(
        mut self,
        total_events: u64,
        final_state_digest: Option<String>,
        duration_ms: u64,
    ) -> Self {
        self.status = "COMPLETED".to_string();
        self.total_events = total_events;
        self.final_state_digest = final_state_digest;
        self.duration_ms = duration_ms;
        self.success = true;
        self.completed_at = Some(Utc::now());
        self
    }

    /// Mark run as failed
    pub fn fail(mut self, total_events: u64, duration_ms: u64) -> Self {
        self.status = "FAILED".to_string();
        self.total_events = total_events;
        self.duration_ms = duration_ms;
        self.success = false;
        self.completed_at = Some(Utc::now());
        self
    }

    /// Mark run as cancelled
    pub fn cancel(mut self, total_events: u64, duration_ms: u64) -> Self {
        self.status = "CANCELLED".to_string();
        self.total_events = total_events;
        self.duration_ms = duration_ms;
        self.success = false;
        self.completed_at = Some(Utc::now());
        self
    }
}

/// Run event record - single event in execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEventRecord {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// Run ID this event belongs to
    pub run_id: String,
    /// Monotonic sequence number within run (1-indexed)
    pub seq: u64,
    /// Event kind (e.g. "graph_started", "node_entered", "tool_called")
    pub kind: String,
    /// Event payload (JSON)
    pub payload: serde_json::Value,
    /// Event timestamp
    #[serde(with = "surreal_datetime")]
    pub timestamp: DateTime<Utc>,
}

impl RunEventRecord {
    /// Create a new run event record
    pub fn new(run_id: String, seq: u64, kind: String, payload: serde_json::Value) -> Self {
        RunEventRecord {
            id: None,
            run_id,
            seq,
            kind,
            payload,
            timestamp: Utc::now(),
        }
    }
}

/// Release record - agent release and version management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseRecordSchema {
    /// SurrealDB record ID
    pub id: Option<surrealdb::sql::Thing>,
    /// Release/Agent name
    pub name: String,
    /// Spec digest being released
    pub spec_digest: String,
    /// Version label (e.g. "v1.2.3")
    pub version_label: Option<String>,
    /// Who or what promoted this release
    pub promoted_by: String,
    /// Release notes
    pub notes: Option<String>,
    /// Created timestamp
    #[serde(with = "surreal_datetime")]
    pub created_at: DateTime<Utc>,
}

impl ReleaseRecordSchema {
    /// Create a new release record
    pub fn new(
        name: String,
        spec_digest: String,
        version_label: Option<String>,
        promoted_by: String,
        notes: Option<String>,
    ) -> Self {
        ReleaseRecordSchema {
            id: None,
            name,
            spec_digest,
            version_label,
            promoted_by,
            notes,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_id_from_state() {
        let state = b"test state data";
        let commit_id = CommitId::from_state(state);

        assert!(!commit_id.hash.is_empty());
        assert_eq!(commit_id.hash.len(), 64); // SHA256 hex = 64 chars
        assert!(commit_id.logic_hash.is_none());
        assert!(commit_id.env_hash.is_none());
    }

    #[test]
    fn test_commit_id_deterministic() {
        let state = b"same state";
        let id1 = CommitId::from_state(state);
        let id2 = CommitId::from_state(state);

        assert_eq!(id1.hash, id2.hash);
    }

    #[test]
    fn test_commit_id_different_states() {
        let id1 = CommitId::from_state(b"state 1");
        let id2 = CommitId::from_state(b"state 2");

        assert_ne!(id1.hash, id2.hash);
    }

    #[test]
    fn test_commit_id_short() {
        let commit_id = CommitId::from_state(b"test");
        assert_eq!(commit_id.short().len(), 8);
    }

    #[test]
    fn test_composite_commit_id() {
        let commit_id = CommitId::new(Some("logic-hash"), "state-hash", Some("env-hash"));

        assert!(!commit_id.hash.is_empty());
        assert_eq!(commit_id.logic_hash, Some("logic-hash".to_string()));
        assert_eq!(commit_id.env_hash, Some("env-hash".to_string()));
    }

    #[test]
    fn test_commit_id_collision_prevention() {
        // Test that swapping components results in different hashes
        let id1 = CommitId::new(Some("ab"), "cd", None);
        let id2 = CommitId::new(Some("a"), "bcd", None);
        assert_ne!(id1.hash, id2.hash);

        // Test that None vs "none" string doesn't collide if we use prefixes correctly
        // (Wait, we use "none" for None, so if state_hash was "none" and logic_hash was None,
        // it might collide if we don't have prefixes)
        let id3 = CommitId::new(None, "state", None);
        let id4 = CommitId::new(Some("none"), "state", None);
        assert_ne!(id3.hash, id4.hash);
    }

    #[test]
    fn test_snapshot_record_size() {
        let state = serde_json::json!({"key": "value", "nested": {"a": 1}});
        let snapshot = SnapshotRecord::new("commit-123", state);

        assert!(snapshot.size_bytes > 0);
    }

    #[test]
    fn test_run_record_new() {
        let run = RunRecord::new(
            "run-123".to_string(),
            "spec-digest-abc".to_string(),
            Some("abc123".to_string()),
            "test-agent".to_string(),
            serde_json::json!({"env": "test"}),
        );

        assert_eq!(run.run_id, "run-123");
        assert_eq!(run.status, "RUNNING");
        assert_eq!(run.total_events, 0);
        assert!(!run.success);
    }

    #[test]
    fn test_run_record_complete() {
        let run = RunRecord::new(
            "run-123".to_string(),
            "spec-digest-abc".to_string(),
            Some("abc123".to_string()),
            "test-agent".to_string(),
            serde_json::json!({}),
        )
        .complete(5, Some("state-digest-xyz".to_string()), 1000);

        assert_eq!(run.status, "COMPLETED");
        assert_eq!(run.total_events, 5);
        assert!(run.success);
        assert!(run.completed_at.is_some());
    }

    #[test]
    fn test_run_record_fail() {
        let run = RunRecord::new(
            "run-123".to_string(),
            "spec-digest-abc".to_string(),
            None,
            "test-agent".to_string(),
            serde_json::json!({}),
        )
        .fail(2, 500);

        assert_eq!(run.status, "FAILED");
        assert_eq!(run.total_events, 2);
        assert!(!run.success);
        assert!(run.completed_at.is_some());
    }

    #[test]
    fn test_run_event_record() {
        let event = RunEventRecord::new(
            "run-123".to_string(),
            1,
            "graph_started".to_string(),
            serde_json::json!({"graph_id": "g1"}),
        );

        assert_eq!(event.run_id, "run-123");
        assert_eq!(event.seq, 1);
        assert_eq!(event.kind, "graph_started");
    }

    #[test]
    fn test_release_record() {
        let release = ReleaseRecordSchema::new(
            "my-agent".to_string(),
            "spec-digest-abc".to_string(),
            Some("v1.0.0".to_string()),
            "alice".to_string(),
            Some("Initial release".to_string()),
        );

        assert_eq!(release.name, "my-agent");
        assert_eq!(release.version_label, Some("v1.0.0".to_string()));
    }
}
