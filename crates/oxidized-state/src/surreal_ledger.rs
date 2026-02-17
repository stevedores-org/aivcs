//! SurrealDB-backed RunLedger implementation.
//!
//! Persists execution runs and events to SurrealDB with monotonic
//! sequence ordering. Designed for the hosted instance at
//! `surrealdb.stevedores.org` (AKS) or any SurrealDB endpoint.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::engine::any::Any;
use surrealdb::Surreal;
use tracing::{debug, instrument};

use crate::error::StorageError;
use crate::storage_traits::*;

/// SurrealDB-backed run ledger.
///
/// Stores run records in the `runs` table and events in the `run_events` table.
/// Both tables are created with SCHEMAFULL mode for type safety.
#[derive(Clone)]
pub struct SurrealRunLedger {
    db: Surreal<Any>,
}

/// Internal row type for SurrealDB run records.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunRow {
    run_id: String,
    spec_digest: String,
    metadata: serde_json::Value,
    status: String,
    summary: Option<serde_json::Value>,
    created_at: String,
    completed_at: Option<String>,
}

/// Internal row type for SurrealDB event records.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventRow {
    run_id: String,
    seq: u64,
    kind: String,
    payload: serde_json::Value,
    timestamp: String,
}

impl SurrealRunLedger {
    /// Create a new SurrealRunLedger from an existing SurrealDB connection.
    pub fn new(db: Surreal<Any>) -> Self {
        Self { db }
    }

    /// Create and connect to an in-memory SurrealDB for testing.
    pub async fn in_memory() -> std::result::Result<Self, StorageError> {
        let db = surrealdb::engine::any::connect("mem://")
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        db.use_ns("aivcs")
            .use_db("ledger")
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        let ledger = Self { db };
        ledger.init_schema().await?;
        Ok(ledger)
    }

    /// Create from environment (SURREALDB_ENDPOINT or local fallback).
    pub async fn from_env() -> std::result::Result<Self, StorageError> {
        if let Ok(endpoint) = std::env::var("SURREALDB_ENDPOINT") {
            let db = surrealdb::engine::any::connect(&endpoint)
                .await
                .map_err(|e| StorageError::Backend(format!("connect to {endpoint}: {e}")))?;

            if let (Ok(user), Ok(pass)) = (
                std::env::var("SURREALDB_USERNAME"),
                std::env::var("SURREALDB_PASSWORD"),
            ) {
                let is_root = std::env::var("SURREALDB_ROOT")
                    .map(|v| v.to_lowercase() == "true")
                    .unwrap_or(false);

                if is_root {
                    db.signin(surrealdb::opt::auth::Root {
                        username: &user,
                        password: &pass,
                    })
                    .await
                    .map_err(|e| StorageError::Backend(format!("root auth: {e}")))?;
                } else {
                    let ns =
                        std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "aivcs".into());
                    let dbname =
                        std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "ledger".into());
                    db.signin(surrealdb::opt::auth::Database {
                        namespace: &ns,
                        database: &dbname,
                        username: &user,
                        password: &pass,
                    })
                    .await
                    .map_err(|e| StorageError::Backend(format!("db auth: {e}")))?;
                }
            }

            let ns = std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "aivcs".into());
            let dbname = std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "ledger".into());

            db.use_ns(&ns)
                .use_db(&dbname)
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;

            let ledger = Self { db };
            ledger.init_schema().await?;
            Ok(ledger)
        } else {
            // Default to local persistence
            let path = ".aivcs/ledger";
            std::fs::create_dir_all(path).map_err(|e| {
                StorageError::Backend(format!("Failed to create ledger directory {}: {}", path, e))
            })?;
            let url = format!("surrealkv://{}", path);
            let db = surrealdb::engine::any::connect(&url)
                .await
                .map_err(|e| StorageError::Backend(format!("connect to {url}: {e}")))?;

            db.use_ns("aivcs")
                .use_db("ledger")
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;

            let ledger = Self { db };
            ledger.init_schema().await?;
            Ok(ledger)
        }
    }

    /// Initialize the run ledger schema in SurrealDB.
    pub async fn init_schema(&self) -> std::result::Result<(), StorageError> {
        let schema = r#"
            -- Runs table
            DEFINE TABLE IF NOT EXISTS runs SCHEMAFULL;
            DEFINE FIELD run_id ON runs TYPE string;
            DEFINE FIELD spec_digest ON runs TYPE string;
            DEFINE FIELD metadata ON runs FLEXIBLE TYPE object;
            DEFINE FIELD status ON runs TYPE string;
            DEFINE FIELD summary ON runs FLEXIBLE TYPE option<object>;
            DEFINE FIELD created_at ON runs TYPE string;
            DEFINE FIELD completed_at ON runs TYPE option<string>;
            DEFINE INDEX IF NOT EXISTS idx_run_id ON runs FIELDS run_id UNIQUE;
            DEFINE INDEX IF NOT EXISTS idx_run_spec ON runs FIELDS spec_digest;

            -- Run events table
            DEFINE TABLE IF NOT EXISTS run_events SCHEMAFULL;
            DEFINE FIELD run_id ON run_events TYPE string;
            DEFINE FIELD seq ON run_events TYPE int;
            DEFINE FIELD kind ON run_events TYPE string;
            DEFINE FIELD payload ON run_events FLEXIBLE TYPE object;
            DEFINE FIELD timestamp ON run_events TYPE string;
            DEFINE INDEX IF NOT EXISTS idx_event_run ON run_events FIELDS run_id;
            DEFINE INDEX IF NOT EXISTS idx_event_run_seq ON run_events FIELDS run_id, seq UNIQUE;
        "#;

        self.db
            .query(schema)
            .await
            .map_err(|e| StorageError::Backend(format!("schema init: {e}")))?;

        debug!("RunLedger schema initialized");
        Ok(())
    }

    fn run_row_to_record(&self, row: RunRow) -> std::result::Result<RunRecord, StorageError> {
        let status = match row.status.as_str() {
            "Running" => RunStatus::Running,
            "Completed" => RunStatus::Completed,
            "Failed" => RunStatus::Failed,
            other => {
                return Err(StorageError::Backend(format!(
                    "unknown run status: {other}"
                )))
            }
        };

        let metadata: RunMetadata = serde_json::from_value(row.metadata)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let summary: Option<RunSummary> = row
            .summary
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let created_at = row
            .created_at
            .parse()
            .map_err(|e| StorageError::Serialization(format!("parse created_at: {e}")))?;

        let completed_at = row
            .completed_at
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| StorageError::Serialization(format!("parse completed_at: {e}")))?;

        let spec_digest = ContentDigest::try_from(row.spec_digest)?;

        Ok(RunRecord {
            run_id: RunId(row.run_id),
            spec_digest,
            metadata,
            status,
            summary,
            created_at,
            completed_at,
        })
    }
}

#[async_trait]
impl RunLedger for SurrealRunLedger {
    #[instrument(skip(self, metadata), fields(spec = %spec_digest))]
    async fn create_run(
        &self,
        spec_digest: &ContentDigest,
        metadata: RunMetadata,
    ) -> StorageResult<RunId> {
        let run_id = RunId::new();
        let now = Utc::now().to_rfc3339();

        let metadata_json = serde_json::to_value(&metadata)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let row = RunRow {
            run_id: run_id.0.clone(),
            spec_digest: spec_digest.as_str().to_string(),
            metadata: metadata_json,
            status: "Running".to_string(),
            summary: None,
            created_at: now,
            completed_at: None,
        };

        let _created: Option<RunRow> = self
            .db
            .create("runs")
            .content(row)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        debug!(run_id = %run_id, "run created");
        Ok(run_id)
    }

    #[instrument(skip(self, event), fields(run = %run_id, seq = event.seq))]
    async fn append_event(&self, run_id: &RunId, event: RunEvent) -> StorageResult<()> {
        // Check run exists and is Running
        let run = self.get_run(run_id).await?;
        if run.status != RunStatus::Running {
            return Err(StorageError::InvalidRunState {
                run_id: run_id.0.clone(),
                status: format!("{:?}", run.status),
                expected: "Running".to_string(),
            });
        }

        let row = EventRow {
            run_id: run_id.0.clone(),
            seq: event.seq,
            kind: event.kind,
            payload: event.payload,
            timestamp: event.timestamp.to_rfc3339(),
        };

        let _created: Option<EventRow> = self
            .db
            .create("run_events")
            .content(row)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        debug!("event appended");
        Ok(())
    }

    #[instrument(skip(self, summary))]
    async fn complete_run(&self, run_id: &RunId, summary: RunSummary) -> StorageResult<()> {
        let run = self.get_run(run_id).await?;
        if run.status != RunStatus::Running {
            return Err(StorageError::InvalidRunState {
                run_id: run_id.0.clone(),
                status: format!("{:?}", run.status),
                expected: "Running".to_string(),
            });
        }

        let now = Utc::now().to_rfc3339();
        let summary_json = serde_json::to_value(&summary)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.db
            .query("UPDATE runs SET status = $status, summary = $summary, completed_at = $now WHERE run_id = $rid")
            .bind(("status", "Completed"))
            .bind(("summary", summary_json))
            .bind(("now", now))
            .bind(("rid", run_id.0.clone()))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        debug!("run completed");
        Ok(())
    }

    #[instrument(skip(self, summary))]
    async fn fail_run(&self, run_id: &RunId, summary: RunSummary) -> StorageResult<()> {
        let run = self.get_run(run_id).await?;
        if run.status != RunStatus::Running {
            return Err(StorageError::InvalidRunState {
                run_id: run_id.0.clone(),
                status: format!("{:?}", run.status),
                expected: "Running".to_string(),
            });
        }

        let now = Utc::now().to_rfc3339();
        let summary_json = serde_json::to_value(&summary)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.db
            .query("UPDATE runs SET status = $status, summary = $summary, completed_at = $now WHERE run_id = $rid")
            .bind(("status", "Failed"))
            .bind(("summary", summary_json))
            .bind(("now", now))
            .bind(("rid", run_id.0.clone()))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        debug!("run failed");
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_run(&self, run_id: &RunId) -> StorageResult<RunRecord> {
        let mut result = self
            .db
            .query("SELECT * FROM runs WHERE run_id = $rid")
            .bind(("rid", run_id.0.clone()))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        let rows: Vec<RunRow> = result
            .take(0)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let row = rows.into_iter().next().ok_or_else(|| StorageError::RunNotFound {
            run_id: run_id.0.clone(),
        })?;

        self.run_row_to_record(row)
    }

    #[instrument(skip(self))]
    async fn get_events(&self, run_id: &RunId) -> StorageResult<Vec<RunEvent>> {
        // Verify run exists
        let _ = self.get_run(run_id).await?;

        let mut result = self
            .db
            .query("SELECT * FROM run_events WHERE run_id = $rid ORDER BY seq ASC")
            .bind(("rid", run_id.0.clone()))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        let rows: Vec<EventRow> = result
            .take(0)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let events = rows
            .into_iter()
            .map(|row| {
                let timestamp = row
                    .timestamp
                    .parse()
                    .unwrap_or_else(|_| Utc::now());
                RunEvent {
                    seq: row.seq,
                    kind: row.kind,
                    payload: row.payload,
                    timestamp,
                }
            })
            .collect();

        Ok(events)
    }

    #[instrument(skip(self))]
    async fn list_runs(
        &self,
        spec_digest: Option<&ContentDigest>,
    ) -> StorageResult<Vec<RunRecord>> {
        let rows: Vec<RunRow> = if let Some(digest) = spec_digest {
            let mut result = self
                .db
                .query("SELECT * FROM runs WHERE spec_digest = $digest ORDER BY created_at DESC")
                .bind(("digest", digest.as_str().to_string()))
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            result
                .take(0)
                .map_err(|e| StorageError::Serialization(e.to_string()))?
        } else {
            let mut result = self
                .db
                .query("SELECT * FROM runs ORDER BY created_at DESC")
                .await
                .map_err(|e| StorageError::Backend(e.to_string()))?;
            result
                .take(0)
                .map_err(|e| StorageError::Serialization(e.to_string()))?
        };

        rows.into_iter().map(|r| self.run_row_to_record(r)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_ledger() -> SurrealRunLedger {
        SurrealRunLedger::in_memory().await.unwrap()
    }

    fn test_metadata() -> RunMetadata {
        RunMetadata {
            git_sha: Some("abc123".to_string()),
            agent_name: "test-agent".to_string(),
            tags: serde_json::json!({"env": "test"}),
        }
    }

    fn test_digest() -> ContentDigest {
        ContentDigest::from_bytes(b"test spec content")
    }

    #[tokio::test]
    async fn create_run_returns_unique_ids() {
        let ledger = make_ledger().await;
        let d = test_digest();
        let id1 = ledger.create_run(&d, test_metadata()).await.unwrap();
        let id2 = ledger.create_run(&d, test_metadata()).await.unwrap();
        assert_ne!(id1.0, id2.0);
    }

    #[tokio::test]
    async fn get_run_returns_created_run() {
        let ledger = make_ledger().await;
        let d = test_digest();
        let run_id = ledger.create_run(&d, test_metadata()).await.unwrap();
        let run = ledger.get_run(&run_id).await.unwrap();
        assert_eq!(run.run_id, run_id);
        assert_eq!(run.status, RunStatus::Running);
        assert_eq!(run.metadata.agent_name, "test-agent");
    }

    #[tokio::test]
    async fn get_run_not_found() {
        let ledger = make_ledger().await;
        let err = ledger.get_run(&RunId("nonexistent".into())).await.unwrap_err();
        assert!(matches!(err, StorageError::RunNotFound { .. }));
    }

    #[tokio::test]
    async fn append_and_get_events_ordered() {
        let ledger = make_ledger().await;
        let run_id = ledger
            .create_run(&test_digest(), test_metadata())
            .await
            .unwrap();

        // Append events out of order to verify seq ordering
        for seq in [3, 1, 2] {
            ledger
                .append_event(
                    &run_id,
                    RunEvent {
                        seq,
                        kind: format!("Event{seq}"),
                        payload: serde_json::json!({"seq": seq}),
                        timestamp: Utc::now(),
                    },
                )
                .await
                .unwrap();
        }

        let events = ledger.get_events(&run_id).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 3);
    }

    #[tokio::test]
    async fn complete_run_sets_status() {
        let ledger = make_ledger().await;
        let run_id = ledger
            .create_run(&test_digest(), test_metadata())
            .await
            .unwrap();

        ledger
            .complete_run(
                &run_id,
                RunSummary {
                    total_events: 5,
                    final_state_digest: None,
                    duration_ms: 100,
                    success: true,
                },
            )
            .await
            .unwrap();

        let run = ledger.get_run(&run_id).await.unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.summary.is_some());
        assert!(run.completed_at.is_some());
    }

    #[tokio::test]
    async fn fail_run_sets_status() {
        let ledger = make_ledger().await;
        let run_id = ledger
            .create_run(&test_digest(), test_metadata())
            .await
            .unwrap();

        ledger
            .fail_run(
                &run_id,
                RunSummary {
                    total_events: 2,
                    final_state_digest: None,
                    duration_ms: 50,
                    success: false,
                },
            )
            .await
            .unwrap();

        let run = ledger.get_run(&run_id).await.unwrap();
        assert_eq!(run.status, RunStatus::Failed);
    }

    #[tokio::test]
    async fn cannot_append_to_completed_run() {
        let ledger = make_ledger().await;
        let run_id = ledger
            .create_run(&test_digest(), test_metadata())
            .await
            .unwrap();

        ledger
            .complete_run(
                &run_id,
                RunSummary {
                    total_events: 0,
                    final_state_digest: None,
                    duration_ms: 10,
                    success: true,
                },
            )
            .await
            .unwrap();

        let err = ledger
            .append_event(
                &run_id,
                RunEvent {
                    seq: 1,
                    kind: "Late".into(),
                    payload: serde_json::json!({}),
                    timestamp: Utc::now(),
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(err, StorageError::InvalidRunState { .. }));
    }

    #[tokio::test]
    async fn cannot_complete_twice() {
        let ledger = make_ledger().await;
        let run_id = ledger
            .create_run(&test_digest(), test_metadata())
            .await
            .unwrap();

        let summary = RunSummary {
            total_events: 0,
            final_state_digest: None,
            duration_ms: 10,
            success: true,
        };

        ledger.complete_run(&run_id, summary.clone()).await.unwrap();
        let err = ledger.complete_run(&run_id, summary).await.unwrap_err();
        assert!(matches!(err, StorageError::InvalidRunState { .. }));
    }

    #[tokio::test]
    async fn list_runs_all() {
        let ledger = make_ledger().await;
        let d1 = ContentDigest::from_bytes(b"spec-a");
        let d2 = ContentDigest::from_bytes(b"spec-b");

        ledger.create_run(&d1, test_metadata()).await.unwrap();
        ledger.create_run(&d2, test_metadata()).await.unwrap();
        ledger.create_run(&d1, test_metadata()).await.unwrap();

        let all = ledger.list_runs(None).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn list_runs_filtered_by_spec() {
        let ledger = make_ledger().await;
        let d1 = ContentDigest::from_bytes(b"spec-a");
        let d2 = ContentDigest::from_bytes(b"spec-b");

        ledger.create_run(&d1, test_metadata()).await.unwrap();
        ledger.create_run(&d2, test_metadata()).await.unwrap();
        ledger.create_run(&d1, test_metadata()).await.unwrap();

        let filtered = ledger.list_runs(Some(&d1)).await.unwrap();
        assert_eq!(filtered.len(), 2);
        for run in &filtered {
            assert_eq!(run.spec_digest, d1);
        }
    }

    #[tokio::test]
    async fn event_sequence_monotonic_verified() {
        let ledger = make_ledger().await;
        let run_id = ledger
            .create_run(&test_digest(), test_metadata())
            .await
            .unwrap();

        for seq in 1..=10 {
            ledger
                .append_event(
                    &run_id,
                    RunEvent {
                        seq,
                        kind: "NodeEntered".into(),
                        payload: serde_json::json!({"node": format!("node-{seq}")}),
                        timestamp: Utc::now(),
                    },
                )
                .await
                .unwrap();
        }

        let events = ledger.get_events(&run_id).await.unwrap();
        assert_eq!(events.len(), 10);
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.seq, (i + 1) as u64, "events must be monotonically ordered");
        }
    }
}
