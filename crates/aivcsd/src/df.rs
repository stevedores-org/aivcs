/// data-fabric integration gateway

use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// PipelineEvent for data-fabric ingestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineEvent {
    pub event_type: String,
    pub status: String,
    pub check_name: Option<String>,
    pub output: Option<String>,
}

/// DataFabricGateway trait for abstraction
#[async_trait]
pub trait DataFabricGateway: Send + Sync {
    /// Ingest CI events to data-fabric
    async fn ingest_aivcs_events(&self, event: PipelineEvent) -> Result<()>;

    /// Complete a task in the data-fabric queue
    async fn complete_task(&self, task_id: &str) -> Result<()>;
}

/// Mock implementation for testing
pub struct MockDataFabricGateway {
    pub last_event: std::sync::Mutex<Option<PipelineEvent>>,
    pub completed_tasks: std::sync::Mutex<Vec<String>>,
}

impl MockDataFabricGateway {
    pub fn new() -> Self {
        Self {
            last_event: std::sync::Mutex::new(None),
            completed_tasks: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl DataFabricGateway for MockDataFabricGateway {
    async fn ingest_aivcs_events(&self, event: PipelineEvent) -> Result<()> {
        *self.last_event.lock().unwrap() = Some(event);
        Ok(())
    }

    async fn complete_task(&self, task_id: &str) -> Result<()> {
        self.completed_tasks.lock().unwrap().push(task_id.to_string());
        Ok(())
    }
}
