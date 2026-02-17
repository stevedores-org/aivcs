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
use nix_env_manager::{
    generate_environment_hash, generate_logic_hash, is_attic_available, is_nix_available,
    AtticClient, NixHash,
};
use oxidized_state::{
    BranchRecord, CommitId, CommitRecord, RunLedger, SurrealHandle, SurrealRunLedger,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use aivcs_core::fork_agent_parallel;

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

    /// Create a snapshot of agent state, linked to the current git HEAD
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

        /// Git SHA to associate (auto-detected from cwd if omitted)
        #[arg(long)]
        git_sha: Option<String>,

        /// CAS storage directory (default: .aivcs/cas in current directory)
        #[arg(long)]
        cas_dir: Option<PathBuf>,
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

    /// Environment management (Nix/Attic)
    Env {
        #[command(subcommand)]
        action: EnvAction,
    },

    /// Fork multiple parallel branches for exploration (Phase 4)
    Fork {
        /// Parent branch or commit to fork from
        #[arg(default_value = "main")]
        parent: String,

        /// Number of branches to create
        #[arg(short, long, default_value = "5")]
        count: u8,

        /// Branch name prefix
        #[arg(short, long, default_value = "experiment")]
        prefix: String,
    },

    /// Show reasoning trace for time-travel debugging (Phase 4)
    Trace {
        /// Commit ID or branch to trace
        commit: String,

        /// Maximum depth of trace
        #[arg(short, long, default_value = "20")]
        depth: usize,
    },

    /// Replay all events for a run in sequence order and print a digest
    Replay {
        /// Run ID to replay
        #[arg(long)]
        run: String,
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

#[derive(Subcommand)]
enum EnvAction {
    /// Show environment hash for a flake
    Hash {
        /// Path to flake directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Show hash of Rust source code (logic hash)
    LogicHash {
        /// Path to source directory
        #[arg(default_value = "src")]
        path: PathBuf,
    },

    /// Check Attic cache status
    CacheInfo,

    /// Check if environment is cached
    IsCached {
        /// Environment hash to check
        hash: String,
    },

    /// Show system info (Nix/Attic availability)
    Info,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    // Initialize database connection
    let handle = SurrealHandle::setup_from_env()
        .await
        .context("Failed to connect to AIVCS database")?;

    match cli.command {
        Commands::Init { path } => cmd_init(&handle, &path).await,
        Commands::Snapshot {
            state,
            message,
            author,
            branch,
            git_sha,
            cas_dir,
        } => {
            cmd_snapshot(
                &handle,
                &state,
                &message,
                &author,
                &branch,
                git_sha.as_deref(),
                cas_dir.as_deref(),
            )
            .await
        }
        Commands::Restore { commit, output } => {
            cmd_restore(&handle, &commit, output.as_deref()).await
        }
        Commands::Branch { action } => match action {
            BranchAction::List => cmd_branch_list(&handle).await,
            BranchAction::Create { name, from } => cmd_branch_create(&handle, &name, &from).await,
            BranchAction::Delete { name } => cmd_branch_delete(&handle, &name).await,
        },
        Commands::Log { reference, limit } => cmd_log(&handle, &reference, limit).await,
        Commands::Merge {
            source,
            target,
            message,
        } => cmd_merge(&handle, &source, &target, message.as_deref()).await,
        Commands::Diff { a, b } => cmd_diff(&handle, &a, &b).await,
        Commands::Env { action } => match action {
            EnvAction::Hash { path } => cmd_env_hash(&path).await,
            EnvAction::LogicHash { path } => cmd_logic_hash(&path).await,
            EnvAction::CacheInfo => cmd_cache_info().await,
            EnvAction::IsCached { hash } => cmd_is_cached(&hash).await,
            EnvAction::Info => cmd_env_info().await,
        },
        Commands::Fork {
            parent,
            count,
            prefix,
        } => cmd_fork(&handle, &parent, count, &prefix).await,
        Commands::Trace { commit, depth } => cmd_trace(&handle, &commit, depth).await,
        Commands::Replay { run } => {
            let ledger = SurrealRunLedger::from_env()
                .await
                .context("Failed to connect to run ledger")?;
            cmd_replay(&ledger, &run).await
        }
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

/// Create a snapshot of agent state, linked to the current git HEAD
async fn cmd_snapshot(
    handle: &SurrealHandle,
    state_path: &PathBuf,
    message: &str,
    author: &str,
    branch: &str,
    git_sha_override: Option<&str>,
    cas_dir: Option<&std::path::Path>,
) -> Result<()> {
    // Read state file
    let state_content = std::fs::read_to_string(state_path)
        .context(format!("Failed to read state file: {:?}", state_path))?;

    let state: serde_json::Value =
        serde_json::from_str(&state_content).context("Failed to parse state as JSON")?;

    // Resolve git SHA: use override, or auto-detect from cwd
    let git_sha = match git_sha_override {
        Some(sha) => sha.to_string(),
        None => {
            let cwd = std::env::current_dir().context("Failed to get current directory")?;
            aivcs_core::capture_head_sha(&cwd)
                .map_err(|e| anyhow::anyhow!("git SHA capture failed: {e}"))?
        }
    };

    // Store state in CAS
    let cas_root = cas_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".aivcs/cas"));
    let cas = aivcs_core::FsCasStore::new(&cas_root)
        .map_err(|e| anyhow::anyhow!("failed to open CAS store: {e}"))?;
    let cas_digest = aivcs_core::CasStore::put(&cas, state_content.as_bytes())
        .map_err(|e| anyhow::anyhow!("CAS put failed: {e}"))?;

    // Get parent commit from branch
    let parent_id = handle.get_branch(branch).await?.map(|b| b.head_commit_id);

    // Create commit ID
    let commit_id = CommitId::from_state(state_content.as_bytes());

    // Save snapshot to SurrealDB
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

    info!(
        cas_digest = %cas_digest,
        git_sha = %git_sha,
        "snapshot stored in CAS"
    );

    println!("[{}] {} ({})", branch, message, commit_id.short());
    println!("Commit:    {}", commit_id);
    println!("Git SHA:   {}", git_sha);
    println!("CAS digest: {}", cas_digest);

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
    let snapshot = handle
        .load_snapshot(&commit_hash)
        .await
        .context(format!("Commit not found: {}", reference))?;

    let state_json = serde_json::to_string_pretty(&snapshot.state)?;

    if let Some(path) = output {
        std::fs::write(path, &state_json).context(format!("Failed to write to {:?}", path))?;
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
        println!(
            "{}{} -> {}",
            prefix,
            branch.name,
            &branch.head_commit_id[..8]
        );
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

    println!(
        "Created branch '{}' at {}",
        name,
        &head_commit[..8.min(head_commit.len())]
    );

    Ok(())
}

/// Delete a branch
async fn cmd_branch_delete(handle: &SurrealHandle, name: &str) -> Result<()> {
    handle
        .delete_branch(name)
        .await
        .context(format!("Failed to delete branch '{}'", name))?;

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
        println!(
            "Date:   {}",
            commit.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
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
    let source_commit = handle
        .get_branch_head(source)
        .await
        .context(format!("Source branch not found: {}", source))?;

    let target_commit = handle
        .get_branch_head(target)
        .await
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
    )
    .await?;

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

// ========== Environment Commands (Phase 2) ==========

/// Generate and display environment hash
async fn cmd_env_hash(path: &PathBuf) -> Result<()> {
    let hash = generate_environment_hash(path).context(format!(
        "Failed to generate environment hash for {:?}",
        path
    ))?;

    println!("Environment Hash: {}", hash.hash);
    println!("Source: {:?}", hash.source);
    println!("Short: {}", hash.short());

    Ok(())
}

/// Generate and display logic hash (Rust source)
async fn cmd_logic_hash(path: &PathBuf) -> Result<()> {
    let hash = generate_logic_hash(path)
        .context(format!("Failed to generate logic hash for {:?}", path))?;

    println!("Logic Hash: {}", hash);
    println!("Short: {}", &hash[..12.min(hash.len())]);

    Ok(())
}

/// Show Attic cache information
async fn cmd_cache_info() -> Result<()> {
    let client = AtticClient::from_env();
    let info = client
        .get_cache_info()
        .await
        .context("Failed to get cache info")?;

    println!("Cache Name: {}", info.name);
    println!("Server: {}", info.server);
    println!("Available: {}", if info.available { "yes" } else { "no" });

    if let Some(details) = info.info {
        println!("\nDetails:");
        println!("{}", details);
    }

    Ok(())
}

/// Check if environment is cached
async fn cmd_is_cached(hash: &str) -> Result<()> {
    let client = AtticClient::from_env();
    let nix_hash = NixHash::new(hash.to_string(), nix_env_manager::HashSource::FlakeLock);

    let cached = client.is_environment_cached(&nix_hash).await;

    if cached {
        println!("Environment {} is CACHED", &hash[..12.min(hash.len())]);
    } else {
        println!("Environment {} is NOT cached", &hash[..12.min(hash.len())]);
    }

    Ok(())
}

/// Show environment system info
async fn cmd_env_info() -> Result<()> {
    println!("AIVCS Environment Info");
    println!("======================");
    println!();

    // Nix availability
    let nix = is_nix_available();
    println!("Nix installed: {}", if nix { "yes" } else { "no" });

    if nix {
        // Get Nix version
        if let Ok(output) = std::process::Command::new("nix").arg("--version").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                println!("Nix version: {}", version.trim());
            }
        }
    }

    // Attic availability
    let attic = is_attic_available();
    println!("Attic installed: {}", if attic { "yes" } else { "no" });

    if attic {
        if let Ok(output) = std::process::Command::new("attic")
            .arg("--version")
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                println!("Attic version: {}", version.trim());
            }
        }
    }

    println!();

    // Environment variables
    println!("Environment Variables:");
    if let Ok(server) = std::env::var("ATTIC_SERVER") {
        println!("  ATTIC_SERVER: {}", server);
    } else {
        println!("  ATTIC_SERVER: (not set)");
    }
    if let Ok(cache) = std::env::var("ATTIC_CACHE") {
        println!("  ATTIC_CACHE: {}", cache);
    } else {
        println!("  ATTIC_CACHE: (not set)");
    }
    if std::env::var("ATTIC_TOKEN").is_ok() {
        println!("  ATTIC_TOKEN: (set)");
    } else {
        println!("  ATTIC_TOKEN: (not set)");
    }

    Ok(())
}

// ========== Parallel Simulation Commands (Phase 4) ==========

/// Fork multiple parallel branches for exploration
async fn cmd_fork(handle: &SurrealHandle, parent: &str, count: u8, prefix: &str) -> Result<()> {
    // Resolve parent reference (branch name or commit ID)
    let parent_commit = if let Ok(Some(branch)) = handle.get_branch(parent).await {
        branch.head_commit_id
    } else {
        parent.to_string()
    };

    println!(
        "Forking {} branches from {} with prefix '{}'",
        count,
        &parent_commit[..8.min(parent_commit.len())],
        prefix
    );

    let handle_arc = Arc::new(handle.clone());

    let result = fork_agent_parallel(handle_arc, &parent_commit, count, prefix).await?;

    println!("\nCreated {} parallel branches:", result.branches.len());
    for (i, branch) in result.branches.iter().enumerate() {
        println!("  {} -> {}", branch, result.commit_ids[i].short());
    }

    println!("\nUse 'aivcs branch list' to see all branches");
    println!("Use 'aivcs trace <commit>' to debug a branch's reasoning");

    Ok(())
}

/// Show reasoning trace for time-travel debugging
async fn cmd_trace(handle: &SurrealHandle, reference: &str, depth: usize) -> Result<()> {
    // Resolve reference
    let commit_hash = if let Ok(Some(branch)) = handle.get_branch(reference).await {
        branch.head_commit_id
    } else {
        reference.to_string()
    };

    println!(
        "Reasoning Trace for {}",
        &commit_hash[..12.min(commit_hash.len())]
    );
    println!("=========================================\n");

    // Get commit history (limited by depth)
    let history = handle.get_commit_history(&commit_hash, depth).await?;

    if history.is_empty() {
        println!("No commits found for '{}'", reference);
        return Ok(());
    }

    // Load snapshots and display trace
    for (i, commit) in history.iter().enumerate() {
        let step_marker = if i == 0 { "HEAD" } else { &format!("~{}", i) };

        println!(
            "[{}] {} - {}",
            step_marker,
            commit.commit_id.short(),
            commit.message
        );
        println!(
            "    Author: {} | {}",
            commit.author,
            commit.created_at.format("%Y-%m-%d %H:%M:%S")
        );

        // Try to load and display state summary
        if let Ok(snapshot) = handle.load_snapshot(&commit.commit_id.hash).await {
            if let Some(obj) = snapshot.state.as_object() {
                // Show key state fields
                let keys: Vec<_> = obj.keys().take(5).collect();
                for key in keys {
                    if let Some(value) = obj.get(key) {
                        let value_str = match value {
                            serde_json::Value::String(s) => truncate(s, 40),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::Bool(b) => b.to_string(),
                            _ => format!("{}", value).chars().take(40).collect(),
                        };
                        println!("    {}: {}", key, value_str);
                    }
                }
            }
        }
        println!();
    }

    println!(
        "Showing {} of {} commits (use --depth to see more)",
        history.len(),
        depth
    );

    Ok(())
}

/// Replay all events for a run in sequence order and print a digest
async fn cmd_replay(ledger: &dyn RunLedger, run_id_str: &str) -> Result<()> {
    let (events, summary) = aivcs_core::replay_run(ledger, run_id_str)
        .await
        .with_context(|| format!("replay failed for run: {}", run_id_str))?;

    println!("Run:    {}", summary.run_id);
    println!("Agent:  {}", summary.agent_name);
    println!("Status: {:?}", summary.status);
    println!();

    for event in &events {
        println!("[{:>6}] {} | {}", event.seq, event.kind, event.payload);
    }

    println!();
    println!("Events: {}", summary.event_count);
    println!("Digest: {}", summary.replay_digest);

    Ok(())
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

        // Run snapshot command (with explicit git SHA since test isn't in a git repo)
        let result = cmd_snapshot(
            &handle,
            &state_path,
            "Test snapshot",
            "test-agent",
            "main",
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(temp_dir.path().join("cas").as_path()),
        )
        .await;

        assert!(result.is_ok(), "Snapshot failed: {:?}", result.err());

        // Verify we can get the branch head
        let head = handle.get_branch_head("main").await.unwrap();
        assert!(!head.is_empty());
    }

    #[tokio::test]
    async fn test_cmd_fork_creates_branches_in_same_db() {
        let handle = SurrealHandle::setup_db().await.unwrap();

        // Initialize repo
        cmd_init(&handle, &PathBuf::from(".")).await.unwrap();

        // Create a snapshot to fork from
        let temp_dir = tempfile::tempdir().unwrap();
        let state_path = temp_dir.path().join("state.json");
        std::fs::write(&state_path, r#"{"step": 1, "value": "test"}"#).unwrap();
        cmd_snapshot(
            &handle,
            &state_path,
            "Base",
            "agent",
            "main",
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            Some(temp_dir.path().join("cas").as_path()),
        )
        .await
        .unwrap();

        // Run fork command
        let result = cmd_fork(&handle, "main", 2, "test-fork").await;

        assert!(result.is_ok(), "Fork failed: {:?}", result.err());

        // Verify branches exist in the original handle
        let branches = handle.list_branches().await.unwrap();
        let fork_branches: Vec<_> = branches
            .iter()
            .filter(|b| b.name.starts_with("test-fork"))
            .collect();

        assert_eq!(
            fork_branches.len(),
            2,
            "Should have created 2 branches in the same DB"
        );
    }

    #[tokio::test]
    async fn test_snapshot_linked_to_git_sha() {
        let handle = SurrealHandle::setup_db().await.unwrap();
        cmd_init(&handle, &PathBuf::from(".")).await.unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let state_path = temp_dir.path().join("state.json");
        std::fs::write(&state_path, r#"{"model": "gpt-4", "step": 42}"#).unwrap();

        let git_sha = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let cas_dir = temp_dir.path().join("cas");

        // Create snapshot with explicit git SHA
        cmd_snapshot(
            &handle,
            &state_path,
            "Linked to git commit",
            "agent-v1",
            "main",
            Some(git_sha),
            Some(cas_dir.as_path()),
        )
        .await
        .unwrap();

        // Verify commit exists in DB
        let head = handle.get_branch_head("main").await.unwrap();
        assert!(!head.is_empty());

        // Verify state was stored in CAS
        let store = aivcs_core::FsCasStore::new(&cas_dir).unwrap();
        let state_bytes = r#"{"model": "gpt-4", "step": 42}"#.as_bytes();
        let digest = aivcs_core::Digest::compute(state_bytes);
        assert!(
            aivcs_core::CasStore::exists(&store, &digest).unwrap(),
            "State should exist in CAS after snapshot"
        );

        // Verify CAS roundtrip
        let retrieved = aivcs_core::CasStore::get(&store, &digest).unwrap();
        assert_eq!(retrieved, state_bytes);
    }

    #[tokio::test]
    async fn test_snapshot_auto_detects_git_sha_in_repo() {
        // Create a temporary git repo
        let temp_dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "test-user"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "initial"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        // Capture the actual HEAD SHA
        let expected_sha = aivcs_core::capture_head_sha(temp_dir.path()).unwrap();

        assert_eq!(expected_sha.len(), 40);
        assert!(expected_sha.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
