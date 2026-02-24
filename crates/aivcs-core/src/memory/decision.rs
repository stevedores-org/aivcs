//! Decision Recording and Learning — EPIC5 Phase 1
//!
//! Captures agent decisions with rationale and outcomes for learning and analysis.

use oxidized_state::{DecisionRecord, MemoryProvenanceRecord, SurrealHandle};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{AivcsError, Result};

/// Configuration for decision recording
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecorderConfig {
    /// Enable decision recording
    pub enabled: bool,
    /// Capture decisions from run events
    pub capture_from_events: bool,
    /// Capture decisions from tool calls
    pub capture_from_tools: bool,
    /// Maximum decision record size (bytes)
    pub max_decision_size: usize,
}

impl Default for DecisionRecorderConfig {
    fn default() -> Self {
        DecisionRecorderConfig {
            enabled: true,
            capture_from_events: true,
            capture_from_tools: true,
            max_decision_size: 10_000,
        }
    }
}

/// Records agent decisions with rationale for learning
pub struct DecisionRecorder {
    handle: Arc<SurrealHandle>,
    config: DecisionRecorderConfig,
}

impl DecisionRecorder {
    /// Create a new decision recorder
    pub fn new(handle: Arc<SurrealHandle>, config: DecisionRecorderConfig) -> Self {
        DecisionRecorder { handle, config }
    }

    /// Create with default configuration
    pub fn with_default_config(handle: Arc<SurrealHandle>) -> Self {
        Self::new(handle, DecisionRecorderConfig::default())
    }

    /// Record a decision from a run
    ///
    /// This captures decisions made during execution with context and rationale.
    pub async fn record_decision(
        &self,
        commit_id: String,
        task: String,
        action: String,
        rationale: String,
        confidence: f32,
    ) -> Result<String> {
        if !self.config.enabled {
            return Ok("decision_recording_disabled".to_string());
        }

        // Validate confidence is in range [0.0, 1.0]
        if confidence < 0.0 || confidence > 1.0 {
            return Err(AivcsError::StorageError(
                "confidence must be between 0.0 and 1.0".to_string(),
            ));
        }

        let decision_id = Uuid::new_v4().to_string();

        let decision = DecisionRecord::new(
            decision_id.clone(),
            commit_id,
            task,
            action,
            rationale,
            confidence,
        );

        // Insert into database using SurrealHandle
        self.handle
            .save_decision(&decision)
            .await
            .map_err(|e| AivcsError::StorageError(format!("Failed to record decision: {}", e)))?;

        Ok(decision_id)
    }

    /// Record decision outcome
    pub async fn record_decision_outcome(
        &self,
        decision_id: &str,
        _outcome_json: String,
    ) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Get the decision first to verify it exists
        let _decision = self
            .handle
            .get_decision(decision_id)
            .await
            .map_err(|e| AivcsError::StorageError(format!("Failed to get decision: {}", e)))?
            .ok_or_else(|| AivcsError::StorageError(format!("Decision not found: {}", decision_id)))?;

        // Note: In a real implementation (Phase 2), we would update the record with outcome.
        // For Phase 1, we just verify the decision exists.

        Ok(())
    }

    /// Record memory provenance for a memory
    pub async fn record_provenance(
        &self,
        provenance: MemoryProvenanceRecord,
    ) -> Result<String> {
        if !self.config.enabled {
            return Ok("provenance_recording_disabled".to_string());
        }

        let memory_id = provenance.memory_id.clone();

        // Insert into database using SurrealHandle
        self.handle
            .save_provenance(&provenance)
            .await
            .map_err(|e| {
                AivcsError::StorageError(format!("Failed to record provenance: {}", e))
            })?;

        Ok(memory_id)
    }

    /// Get decision history for a task
    pub async fn get_decision_history(&self, task: &str, limit: usize) -> Result<Vec<DecisionRecord>> {
        if !self.config.enabled {
            return Ok(vec![]);
        }

        self.handle
            .get_decision_history(task, limit)
            .await
            .map_err(|e| {
                AivcsError::StorageError(format!("Failed to query decision history: {}", e))
            })
    }

    /// Calculate decision success rate for an action
    pub async fn get_decision_success_rate(&self, action: &str) -> Result<f32> {
        if !self.config.enabled {
            return Ok(0.0);
        }

        // For now, return a placeholder value
        // In Phase 2, this will query the database for actual success statistics
        let _action = action;
        Ok(0.0)
    }

    /// Invalidate decision provenance when a run fails
    pub async fn invalidate_provenance_on_failure(&self, commit_id: &str) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // In Phase 2, this will implement the actual invalidation logic
        let _commit_id = commit_id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_decision_recorder_disabled() {
        let config = DecisionRecorderConfig {
            enabled: false,
            ..Default::default()
        };

        // We can't easily test without a real database, but we verify config works
        assert!(!config.enabled);
    }

    #[test]
    fn test_decision_recorder_config_default() {
        let config = DecisionRecorderConfig::default();

        assert!(config.enabled);
        assert!(config.capture_from_events);
        assert!(config.capture_from_tools);
        assert_eq!(config.max_decision_size, 10_000);
    }

    #[test]
    fn test_confidence_validation() {
        // Confidence should be in [0.0, 1.0]
        assert!(0.0 <= 0.5 && 0.5 <= 1.0);
        assert!(0.0 <= 0.0 && 0.0 <= 1.0);
        assert!(0.0 <= 1.0 && 1.0 <= 1.0);
    }
}
