//! Schema definitions for AIVCS SurrealDB tables
//!
//! Tables:
//! - commits: Version control commits (graph nodes)
//! - branches: Branch pointers to commit IDs
//! - agents: Registered agent metadata
//! - memories: Agent memory/context snapshots

use chrono::{DateTime, Utc};
use surrealdb::sql::Datetime as SurrealDatetime;

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

        // For MVP, composite hash is just the state hash
        CommitId {
            hash: state_hash.clone(),
            logic_hash: None,
            state_hash,
            env_hash: None,
        }
    }

    /// Create a full composite CommitId (Phase 2+)
    pub fn new(logic_hash: Option<&str>, state_hash: &str, env_hash: Option<&str>) -> Self {
        let mut hasher = Sha256::new();
        if let Some(lh) = logic_hash {
            hasher.update(lh.as_bytes());
        }
        hasher.update(state_hash.as_bytes());
        if let Some(eh) = env_hash {
            hasher.update(eh.as_bytes());
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
    pub fn short(&self) -> &str {
        &self.hash[..8.min(self.hash.len())]
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
    /// Parent commit ID (None for root commits)
    pub parent_id: Option<String>,
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
    pub fn new(commit_id: CommitId, parent_id: Option<String>, message: &str, author: &str) -> Self {
        CommitRecord {
            id: None,
            commit_id,
            parent_id,
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
        let commit_id = CommitId::new(
            Some("logic-hash"),
            "state-hash",
            Some("env-hash"),
        );

        assert!(!commit_id.hash.is_empty());
        assert_eq!(commit_id.logic_hash, Some("logic-hash".to_string()));
        assert_eq!(commit_id.env_hash, Some("env-hash".to_string()));
    }

    #[test]
    fn test_snapshot_record_size() {
        let state = serde_json::json!({"key": "value", "nested": {"a": 1}});
        let snapshot = SnapshotRecord::new("commit-123", state);

        assert!(snapshot.size_bytes > 0);
    }
}
