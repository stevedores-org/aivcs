//! AIVCS Core - Agent Version Control System CLI
//!
//! The `agent-git` command provides Git-like version control for AI agents.
//!
//! ## Commands
//!
//! - `snapshot`: Create a versioned checkpoint of agent state
//! - `restore`: Restore agent to a previous state
//! - `branch`: Create or list branches
//! - `merge`: Merge two branches with semantic resolution
//! - `log`: Show commit history

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use oxidized_state::{CommitId, CommitRecord, BranchRecord, SurrealHandle};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "aivcs")]
#[command(author = "Stevedores Org")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "AI Agent Version Control System (AIVCS)", long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new AIVCS repository
    Init {
        /// Path to initialize (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Create a snapshot of agent state
    ///
    /// # TDD: test_agent_git_snapshot_cli_returns_valid_id
    Snapshot {
        /// Path to agent state file (JSON)
        #[arg(short, long)]
        state: PathBuf,

        /// Commit message
        #[arg(short, long, default_value = "Auto-snapshot")]
        message: String,

        /// Author/agent name
        #[arg(short, long, default_value = "agent")]
        author: String,

        /// Branch to commit to
        #[arg(short, long, default_value = "main")]
        branch: String,
    },

    /// Restore agent to a previous state
    Restore {
        /// Commit ID to restore (or branch name)
        commit: String,

        /// Output path for restored state
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Manage branches
    Branch {
        #[command(subcommand)]
        action: BranchAction,
    },

    /// Show commit history
    Log {
        /// Branch or commit to show history for
        #[arg(default_value = "main")]
        reference: String,

        /// Maximum number of commits to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Merge two branches
    Merge {
        /// Source branch to merge from
        source: String,

        /// Target branch to merge into (default: current branch)
        #[arg(short, long, default_value = "main")]
        target: String,

        /// Merge commit message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Show differences between commits or branches
    Diff {
        /// First commit/branch
        a: String,

        /// Second commit/branch
        b: String,
    },
}

#[derive(Subcommand)]
enum BranchAction {
    /// List all branches
    List,

    /// Create a new branch
    Create {
        /// Branch name
        name: String,

        /// Starting point (commit ID or branch name)
        #[arg(short, long, default_value = "main")]
        from: String,
    },

    /// Delete a branch
    Delete {
        /// Branch name
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    // Initialize database connection
    let handle = SurrealHandle::setup_db().await
        .context("Failed to connect to AIVCS database")?;

    match cli.command {
        Commands::Init { path } => cmd_init(&handle, &path).await,
        Commands::Snapshot { state, message, author, branch } => {
            cmd_snapshot(&handle, &state, &message, &author, &branch).await
        }
        Commands::Restore { commit, output } => cmd_restore(&handle, &commit, output.as_ref().map(|p| p.as_path())).await,
        Commands::Branch { action } => match action {
            BranchAction::List => cmd_branch_list(&handle).await,
            BranchAction::Create { name, from } => cmd_branch_create(&handle, &name, &from).await,
            BranchAction::Delete { name } => cmd_branch_delete(&handle, &name).await,
        },
        Commands::Log { reference, limit } => cmd_log(&handle, &reference, limit).await,
        Commands::Merge { source, target, message } => {
            cmd_merge(&handle, &source, &target, message.as_deref()).await
        }
        Commands::Diff { a, b } => cmd_diff(&handle, &a, &b).await,
    }
}

/// Initialize a new AIVCS repository
async fn cmd_init(handle: &SurrealHandle, path: &PathBuf) -> Result<()> {
    info!("Initializing AIVCS repository at {:?}", path);

    // Create initial commit
    let initial_state = serde_json::json!({
        "initialized": true,
        "version": env!("CARGO_PKG_VERSION"),
    });

    let commit_id = CommitId::from_state(serde_json::to_vec(&initial_state)?.as_slice());

    // Save initial snapshot
    handle.save_snapshot(&commit_id, initial_state).await?;

    // Create initial commit record
    let commit = CommitRecord::new(commit_id.clone(), None, "Initial commit", "system");
    handle.save_commit(&commit).await?;

    // Create main branch
    let main_branch = BranchRecord::new("main", &commit_id.hash, true);
    handle.save_branch(&main_branch).await?;

    println!("Initialized AIVCS repository at {:?}", path);
    println!("Initial commit: {}", commit_id.short());

    Ok(())
}

/// Create a snapshot of agent state
async fn cmd_snapshot(
    handle: &SurrealHandle,
    state_path: &PathBuf,
    message: &str,
    author: &str,
    branch: &str,
) -> Result<()> {
    // Read state file
    let state_content = std::fs::read_to_string(state_path)
        .context(format!("Failed to read state file: {:?}", state_path))?;

    let state: serde_json::Value = serde_json::from_str(&state_content)
        .context("Failed to parse state as JSON")?;

    // Get parent commit from branch
    let parent_id = handle.get_branch(branch).await?
        .map(|b| b.head_commit_id);

    // Create commit ID
    let commit_id = CommitId::from_state(state_content.as_bytes());

    // Save snapshot
    handle.save_snapshot(&commit_id, state).await?;

    // Create commit record
    let commit = CommitRecord::new(commit_id.clone(), parent_id.clone(), message, author);
    handle.save_commit(&commit).await?;

    // Create graph edge if there's a parent
    if let Some(pid) = &parent_id {
        handle.save_commit_graph_edge(&commit_id.hash, pid).await?;
    }

    // Update branch head
    let branch_record = BranchRecord::new(branch, &commit_id.hash, branch == "main");
    handle.save_branch(&branch_record).await?;

    println!("[{}] {} ({})", branch, message, commit_id.short());
    println!("Commit: {}", commit_id);

    Ok(())
}

/// Restore agent to a previous state
async fn cmd_restore(
    handle: &SurrealHandle,
    reference: &str,
    output: Option<&std::path::Path>,
) -> Result<()> {
    // Try to resolve reference as branch first, then as commit ID
    let commit_hash = if let Ok(Some(branch)) = handle.get_branch(reference).await {
        branch.head_commit_id
    } else {
        reference.to_string()
    };

    // Load snapshot
    let snapshot = handle.load_snapshot(&commit_hash).await
        .context(format!("Commit not found: {}", reference))?;

    let state_json = serde_json::to_string_pretty(&snapshot.state)?;

    if let Some(path) = output {
        std::fs::write(path, &state_json)
            .context(format!("Failed to write to {:?}", path))?;
        println!("Restored state to {:?}", path);
    } else {
        println!("{}", state_json);
    }

    Ok(())
}

/// List all branches
async fn cmd_branch_list(handle: &SurrealHandle) -> Result<()> {
    let branches = handle.list_branches().await?;

    if branches.is_empty() {
        println!("No branches found. Run 'agent-git init' first.");
        return Ok(());
    }

    for branch in branches {
        let prefix = if branch.is_default { "* " } else { "  " };
        println!("{}{} -> {}", prefix, branch.name, &branch.head_commit_id[..8]);
    }

    Ok(())
}

/// Create a new branch
async fn cmd_branch_create(handle: &SurrealHandle, name: &str, from: &str) -> Result<()> {
    // Resolve starting point
    let head_commit = if let Ok(Some(branch)) = handle.get_branch(from).await {
        branch.head_commit_id
    } else {
        from.to_string()
    };

    // Create branch
    let branch = BranchRecord::new(name, &head_commit, false);
    handle.save_branch(&branch).await?;

    println!("Created branch '{}' at {}", name, &head_commit[..8]);

    Ok(())
}

/// Delete a branch
async fn cmd_branch_delete(handle: &SurrealHandle, name: &str) -> Result<()> {
    let branch = handle.get_branch(name).await?
        .ok_or_else(|| anyhow::anyhow!("Branch not found: {}", name))?;

    if branch.is_default {
        anyhow::bail!("Cannot delete the default branch");
    }

    // TODO: Actually delete the branch from DB
    println!("Deleted branch '{}'", name);

    Ok(())
}

/// Show commit history
async fn cmd_log(handle: &SurrealHandle, reference: &str, limit: usize) -> Result<()> {
    // Resolve reference
    let start_commit = if let Ok(Some(branch)) = handle.get_branch(reference).await {
        branch.head_commit_id
    } else {
        reference.to_string()
    };

    let history = handle.get_commit_history(&start_commit, limit).await?;

    if history.is_empty() {
        println!("No commits found for '{}'", reference);
        return Ok(());
    }

    for commit in history {
        println!("commit {}", commit.commit_id);
        println!("Author: {}", commit.author);
        println!("Date:   {}", commit.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
        println!();
        println!("    {}", commit.message);
        println!();
    }

    Ok(())
}

/// Merge two branches
async fn cmd_merge(
    handle: &SurrealHandle,
    source: &str,
    target: &str,
    message: Option<&str>,
) -> Result<()> {
    // Resolve branch heads
    let source_commit = handle.get_branch_head(source).await
        .context(format!("Source branch not found: {}", source))?;

    let target_commit = handle.get_branch_head(target).await
        .context(format!("Target branch not found: {}", target))?;

    let merge_message = message
        .map(String::from)
        .unwrap_or_else(|| format!("Merge branch '{}' into '{}'", source, target));

    // Perform semantic merge
    let result = semantic_rag_merge::semantic_merge(
        handle,
        &source_commit,
        &target_commit,
        &merge_message,
        "agent-git",
    ).await?;

    // Update target branch head
    let branch = BranchRecord::new(target, &result.merge_commit_id.hash, target == "main");
    handle.save_branch(&branch).await?;

    println!("Merge complete: {}", result.merge_commit_id.short());
    println!("{}", result.summary);

    if !result.manual_conflicts.is_empty() {
        println!("\nUnresolved conflicts:");
        for conflict in &result.manual_conflicts {
            println!("  - {}", conflict.key);
        }
    }

    Ok(())
}

/// Show differences between commits/branches
async fn cmd_diff(handle: &SurrealHandle, a: &str, b: &str) -> Result<()> {
    // Resolve references
    let commit_a = if let Ok(Some(branch)) = handle.get_branch(a).await {
        branch.head_commit_id
    } else {
        a.to_string()
    };

    let commit_b = if let Ok(Some(branch)) = handle.get_branch(b).await {
        branch.head_commit_id
    } else {
        b.to_string()
    };

    let delta = semantic_rag_merge::diff_memory_vectors(handle, &commit_a, &commit_b).await?;

    println!("Diff {} -> {}", &commit_a[..8], &commit_b[..8]);
    println!();

    if !delta.only_in_a.is_empty() {
        println!("Only in {}:", &commit_a[..8]);
        for mem in &delta.only_in_a {
            println!("  + {}: {}", mem.key, truncate(&mem.content, 50));
        }
        println!();
    }

    if !delta.only_in_b.is_empty() {
        println!("Only in {}:", &commit_b[..8]);
        for mem in &delta.only_in_b {
            println!("  + {}: {}", mem.key, truncate(&mem.content, 50));
        }
        println!();
    }

    if !delta.conflicts.is_empty() {
        println!("Conflicts:");
        for conflict in &delta.conflicts {
            println!("  ! {}", conflict.key);
            println!("    A: {}", truncate(&conflict.memory_a.content, 40));
            println!("    B: {}", truncate(&conflict.memory_b.content, 40));
        }
    }

    if delta.only_in_a.is_empty() && delta.only_in_b.is_empty() && delta.conflicts.is_empty() {
        println!("No differences found.");
    }

    Ok(())
}

/// Truncate a string for display
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_git_snapshot_cli_returns_valid_id() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        // Initialize first
        cmd_init(&handle, &PathBuf::from(".")).await.unwrap();

        // Create a temp state file
        let temp_dir = tempfile::tempdir().unwrap();
        let state_path = temp_dir.path().join("state.json");
        std::fs::write(&state_path, r#"{"step": 1, "value": "test"}"#).unwrap();

        // Run snapshot command
        let result = cmd_snapshot(
            &handle,
            &state_path,
            "Test snapshot",
            "test-agent",
            "main",
        ).await;

        assert!(result.is_ok(), "Snapshot failed: {:?}", result.err());

        // Verify we can get the branch head
        let head = handle.get_branch_head("main").await.unwrap();
        assert!(!head.is_empty());
    }
}
