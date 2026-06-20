use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Represents the Agentic Issue VCS object known as an IssueGraph.
/// It tracks the state, constraints, intent, and execution of a specific issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGraph {
    pub id: Uuid,
    pub title: String,
    pub state: IssueState,
    pub intent: String,
    pub constraints: Vec<String>,
    pub branches: HashMap<String, IssueBranch>,
    pub current_branch: String,
    pub ledger: Vec<LedgerEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Planned,
    Exploration,
    Executing,
    Verified,
    Closed,
}

/// An Issue Branch represents parallel exploration of solutions or planning paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueBranch {
    pub name: String,
    pub commits: Vec<IssueCommit>,
}

/// Issue Commits represent snapshots of cognition, execution state, or plan revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCommit {
    pub id: Uuid,
    pub message: String,
    pub execution_state: Option<serde_json::Value>,
    pub diff_summary: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub metadata: Option<serde_json::Value>,
}

impl IssueGraph {
    pub fn new(title: String, intent: String) -> Self {
        let now = Utc::now();
        let main_branch = IssueBranch {
            name: "main".to_string(),
            commits: vec![],
        };
        let mut branches = HashMap::new();
        branches.insert("main".to_string(), main_branch);

        Self {
            id: Uuid::new_v4(),
            title,
            state: IssueState::Planned,
            intent,
            constraints: vec![],
            branches,
            current_branch: "main".to_string(),
            ledger: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    pub fn commit(
        &mut self,
        message: String,
        execution_state: Option<serde_json::Value>,
        diff_summary: Option<String>,
    ) -> Uuid {
        let commit = IssueCommit {
            id: Uuid::new_v4(),
            message,
            execution_state,
            diff_summary,
            timestamp: Utc::now(),
        };
        let branch = self
            .branches
            .get_mut(&self.current_branch)
            .expect("Current branch not found");
        branch.commits.push(commit.clone());
        self.updated_at = Utc::now();
        commit.id
    }

    pub fn add_ledger_entry(
        &mut self,
        actor: String,
        action: String,
        metadata: Option<serde_json::Value>,
    ) {
        self.ledger.push(LedgerEntry {
            timestamp: Utc::now(),
            actor,
            action,
            metadata,
        });
        self.updated_at = Utc::now();
    }

    pub fn semantic_merge(
        &mut self,
        target_branch: &str,
        source_branch: &str,
    ) -> Result<(), anyhow::Error> {
        if !self.branches.contains_key(source_branch) {
            return Err(anyhow::anyhow!("Source branch not found"));
        }

        let source = self.branches.get(source_branch).unwrap().clone();
        let target = self
            .branches
            .get_mut(target_branch)
            .ok_or_else(|| anyhow::anyhow!("Target branch not found"))?;

        // Semantic merging logic goes here.
        // For demonstration, we simply append the intent commits.
        for commit in source.commits {
            target.commits.push(commit);
        }

        self.add_ledger_entry(
            "system".into(),
            format!("semantic_merge {} into {}", source_branch, target_branch),
            None,
        );
        self.updated_at = Utc::now();
        Ok(())
    }
}
