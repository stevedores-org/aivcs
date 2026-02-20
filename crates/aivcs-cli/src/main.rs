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
    BranchRecord, CommitId, CommitRecord, ReleaseRegistry, RunEvent, RunLedger,
    SurrealDbReleaseRegistry, SurrealHandle, SurrealRunLedger,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};

use aivcs_ci::{BuiltinStage, CiGate, CiPipeline, CiSpec, StageConfig};
use aivcs_core::{diff_tool_calls, fork_agent_parallel, ToolCallChange};

#[derive(Parser)]
#[command(name = "aivcs")]
#[command(author = "Stevedores Org")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "AI Agent Version Control System (AIVCS)", long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Emit JSON-formatted log lines
    #[arg(long, global = true)]
    json: bool,

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

    /// Replay a recorded run artifact from disk by run ID
    ReplayArtifact {
        /// Run ID to replay
        #[arg(long)]
        run: String,

        /// Root directory containing run artifacts (default: .aivcs/runs)
        #[arg(long)]
        artifacts_dir: Option<PathBuf>,

        /// Optional output file path for replayed artifact
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

    /// Show differences for specs or runs
    Diff {
        #[command(subcommand)]
        action: DiffAction,
    },

    /// Environment management (Nix/Attic)
    Env {
        #[command(subcommand)]
        action: EnvAction,
    },

    /// Release registry operations (promote/rollback/current/history)
    Release {
        #[command(subcommand)]
        action: ReleaseAction,
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

    /// Diff the tool-call sequences of two runs
    DiffRuns {
        /// First run ID
        #[arg(long)]
        run_a: String,

        /// Second run ID
        #[arg(long)]
        run_b: String,
    },

    /// CI pipeline operations
    Ci {
        #[command(subcommand)]
        action: CiAction,
    },
}

#[derive(Subcommand)]
enum CiAction {
    /// Run CI stages and record execution
    Run {
        /// Workspace path (default: current directory)
        #[arg(short, long, default_value = ".")]
        workspace: PathBuf,

        /// Stages to run (comma-separated: fmt,check,clippy,test)
        #[arg(short, long, default_value = "fmt,check")]
        stages: String,

        /// Skip caching
        #[arg(long)]
        no_cache: bool,

        /// Auto-repair (use fix commands)
        #[arg(long)]
        fix: bool,
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

#[derive(Subcommand)]
enum DiffAction {
    /// Diff two spec JSON files
    Spec {
        /// First spec JSON file
        a: PathBuf,
        /// Second spec JSON file
        b: PathBuf,
        /// Emit JSON output instead of terminal text
        #[arg(long)]
        json: bool,
    },
    /// Diff two run event-log JSON files
    Run {
        /// First run events JSON file (array of RunEvent)
        a: PathBuf,
        /// Second run events JSON file (array of RunEvent)
        b: PathBuf,
        /// Emit JSON output instead of terminal text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ReleaseAction {
    /// Promote a validated agent spec as the latest release
    Promote {
        /// Agent name
        name: String,
        /// Git commit SHA linked to this spec
        #[arg(long)]
        git_sha: String,
        /// SHA256 hex of graph definition
        #[arg(long)]
        graph_digest: String,
        /// SHA256 hex of prompts definition
        #[arg(long)]
        prompts_digest: String,
        /// SHA256 hex of tools definition
        #[arg(long)]
        tools_digest: String,
        /// SHA256 hex of configuration
        #[arg(long)]
        config_digest: String,
        /// Who promoted this release
        #[arg(long, default_value = "aivcs-cli")]
        promoted_by: String,
        /// Optional version label (e.g. v1.2.3)
        #[arg(long)]
        version: Option<String>,
        /// Optional release notes
        #[arg(long)]
        notes: Option<String>,
    },
    /// Roll back the agent to the previous release (append-only history)
    Rollback {
        /// Agent name
        name: String,
    },
    /// Show the current release pointer for an agent
    Current {
        /// Agent name
        name: String,
    },
    /// Show release history for an agent (newest first)
    History {
        /// Agent name
        name: String,
    },
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
    aivcs_core::init_tracing(cli.json, level);

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
        Commands::ReplayArtifact {
            run,
            artifacts_dir,
            output,
        } => cmd_replay_artifact(&run, artifacts_dir.as_deref(), output.as_deref()),
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
        Commands::Diff { action } => cmd_diff(action).await,
        Commands::Env { action } => match action {
            EnvAction::Hash { path } => cmd_env_hash(&path).await,
            EnvAction::LogicHash { path } => cmd_logic_hash(&path).await,
            EnvAction::CacheInfo => cmd_cache_info().await,
            EnvAction::IsCached { hash } => cmd_is_cached(&hash).await,
            EnvAction::Info => cmd_env_info().await,
        },
        Commands::Release { action } => match action {
            ReleaseAction::Promote {
                name,
                git_sha,
                graph_digest,
                prompts_digest,
                tools_digest,
                config_digest,
                promoted_by,
                version,
                notes,
            } => {
                cmd_release_promote(
                    &handle,
                    &name,
                    &git_sha,
                    &graph_digest,
                    &prompts_digest,
                    &tools_digest,
                    &config_digest,
                    &promoted_by,
                    version.as_deref(),
                    notes.as_deref(),
                )
                .await
            }
            ReleaseAction::Rollback { name } => cmd_release_rollback(&handle, &name).await,
            ReleaseAction::Current { name } => cmd_release_current(&handle, &name).await,
            ReleaseAction::History { name } => cmd_release_history(&handle, &name).await,
        },
        Commands::Fork {
            parent,
            count,
            prefix,
        } => cmd_fork(&handle, &parent, count, &prefix).await,
        Commands::Trace { commit, depth } => cmd_trace(&handle, &commit, depth).await,
        Commands::DiffRuns { run_a, run_b } => {
            let ledger = SurrealRunLedger::from_env()
                .await
                .context("Failed to connect to run ledger")?;
            cmd_diff_runs(&ledger, &run_a, &run_b).await
        }
        Commands::Ci { action } => match action {
            CiAction::Run {
                workspace,
                stages,
                no_cache,
                fix,
            } => cmd_ci_run(&workspace, &stages, no_cache, fix).await,
        },
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
    let commit = CommitRecord::new(commit_id.clone(), vec![], "Initial commit", "system");
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

    // Resolve git SHA: use override, or auto-detect from cwd (optional)
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let git_sha = match git_sha_override {
        Some(sha) => sha.to_string(),
        None => aivcs_core::capture_head_sha(&cwd)
            .unwrap_or_else(|_| "0000000000000000000000000000000000000000".to_string()),
    };

    // Generate logic and environment hashes for composite CommitId
    let logic_hash = generate_logic_hash(&cwd.join("src")).ok();
    let env_hash = generate_environment_hash(&cwd).ok();

    // Store state in CAS
    let cas_root = cas_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".aivcs/cas"));
    let cas = aivcs_core::FsCasStore::new(&cas_root)
        .map_err(|e| anyhow::anyhow!("failed to open CAS store: {e}"))?;
    let cas_digest = aivcs_core::CasStore::put(&cas, state_content.as_bytes())
        .map_err(|e| anyhow::anyhow!("CAS put failed: {e}"))?;

    // Get parent commit from branch
    let parent_ids = handle
        .get_branch(branch)
        .await?
        .map(|b| vec![b.head_commit_id])
        .unwrap_or_default();

    // Create composite commit ID
    let commit_id = CommitId::new(
        logic_hash.as_deref(),
        &cas_digest.to_hex(),
        env_hash.as_ref().map(|h| h.hash.as_str()),
    );

    // Save snapshot to SurrealDB
    handle.save_snapshot(&commit_id, state).await?;

    // Create commit record
    let commit = CommitRecord::new(commit_id.clone(), parent_ids.clone(), message, author);
    handle.save_commit(&commit).await?;

    // Create graph edges for all parents
    for pid in &parent_ids {
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

/// Replay a recorded run artifact from disk.
///
/// Expected layout:
/// - `<artifacts_dir>/<run_id>/output.json`
/// - `<artifacts_dir>/<run_id>/output.digest`
fn cmd_replay_artifact(
    run_id: &str,
    artifacts_dir: Option<&std::path::Path>,
    output: Option<&std::path::Path>,
) -> Result<()> {
    let root = artifacts_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".aivcs/runs"));
    let run_dir = root.join(run_id);
    let output_path = run_dir.join("output.json");
    let digest_path = run_dir.join("output.digest");

    if !output_path.exists() {
        anyhow::bail!("Recorded artifact not found: {:?}", output_path);
    }
    if !digest_path.exists() {
        anyhow::bail!("Recorded digest not found: {:?}", digest_path);
    }

    let artifact_bytes = std::fs::read(&output_path)
        .with_context(|| format!("Failed to read recorded artifact: {:?}", output_path))?;

    // Validate artifact is JSON for replayability.
    let _: serde_json::Value = serde_json::from_slice(&artifact_bytes)
        .with_context(|| format!("Recorded artifact is not valid JSON: {:?}", output_path))?;

    let expected_digest = std::fs::read_to_string(&digest_path)
        .with_context(|| format!("Failed to read recorded digest: {:?}", digest_path))?
        .trim()
        .to_string();

    let actual_digest = aivcs_core::Digest::compute(&artifact_bytes).to_hex();
    if actual_digest != expected_digest {
        anyhow::bail!(
            "Replay digest mismatch for run {}: expected {}, got {}",
            run_id,
            expected_digest,
            actual_digest
        );
    }

    if let Some(path) = output {
        std::fs::write(path, &artifact_bytes)
            .with_context(|| format!("Failed to write replay output to {:?}", path))?;
        println!("Replayed run {} to {:?}", run_id, path);
    } else {
        println!("{}", String::from_utf8_lossy(&artifact_bytes));
    }

    println!("Replay digest verified: {}", actual_digest);
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
            &branch.head_commit_id[..8.min(branch.head_commit_id.len())]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
struct SpecDiffOutput {
    changed_paths: Vec<String>,
    only_in_a: Vec<String>,
    only_in_b: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct RunDiffOutput {
    events_a: usize,
    events_b: usize,
    tool_call_changes: usize,
    added: usize,
    removed: usize,
    reordered: usize,
    param_changed: usize,
}

async fn cmd_diff(action: DiffAction) -> Result<()> {
    match action {
        DiffAction::Spec { a, b, json } => cmd_diff_spec(&a, &b, json),
        DiffAction::Run { a, b, json } => cmd_diff_run(&a, &b, json),
    }
}

fn cmd_diff_spec(a: &PathBuf, b: &PathBuf, json: bool) -> Result<()> {
    let left: Value = read_json_file(a)?;
    let right: Value = read_json_file(b)?;
    let diff = build_spec_diff(&left, &right);

    if json {
        println!("{}", serde_json::to_string_pretty(&diff)?);
    } else {
        println!("{}", render_spec_diff_text(&diff));
    }
    Ok(())
}

fn cmd_diff_run(a: &PathBuf, b: &PathBuf, json: bool) -> Result<()> {
    let left: Vec<RunEvent> = read_json_file(a)?;
    let right: Vec<RunEvent> = read_json_file(b)?;
    let diff = build_run_diff(&left, &right);

    if json {
        println!("{}", serde_json::to_string_pretty(&diff)?);
    } else {
        println!("{}", render_run_diff_text(&diff));
    }
    Ok(())
}

fn read_json_file<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read JSON file: {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("Invalid JSON in {:?}", path))
}

fn collect_leaf_paths(prefix: &str, value: &Value, out: &mut BTreeMap<String, Value>) {
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            let next = if prefix.is_empty() {
                format!("/{}", k.replace('~', "~0").replace('/', "~1"))
            } else {
                format!("{}/{}", prefix, k.replace('~', "~0").replace('/', "~1"))
            };
            collect_leaf_paths(&next, v, out);
        }
        return;
    }

    if let Some(arr) = value.as_array() {
        for (idx, v) in arr.iter().enumerate() {
            let next = if prefix.is_empty() {
                format!("/{}", idx)
            } else {
                format!("{}/{}", prefix, idx)
            };
            collect_leaf_paths(&next, v, out);
        }
        return;
    }

    let path = if prefix.is_empty() {
        "/".to_string()
    } else {
        prefix.to_string()
    };
    out.insert(path, value.clone());
}

fn build_spec_diff(a: &Value, b: &Value) -> SpecDiffOutput {
    let mut left = BTreeMap::new();
    let mut right = BTreeMap::new();
    collect_leaf_paths("", a, &mut left);
    collect_leaf_paths("", b, &mut right);

    let mut changed_paths = Vec::new();
    let mut only_in_a = Vec::new();
    let mut only_in_b = Vec::new();

    for (path, val_a) in &left {
        match right.get(path) {
            Some(val_b) if val_a != val_b => changed_paths.push(path.clone()),
            None => only_in_a.push(path.clone()),
            _ => {}
        }
    }

    for path in right.keys() {
        if !left.contains_key(path) {
            only_in_b.push(path.clone());
        }
    }

    SpecDiffOutput {
        changed_paths,
        only_in_a,
        only_in_b,
    }
}

fn build_run_diff(a: &[RunEvent], b: &[RunEvent]) -> RunDiffOutput {
    let tool_diff = diff_tool_calls(a, b);
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut reordered = 0usize;
    let mut param_changed = 0usize;

    for change in &tool_diff.changes {
        match change {
            ToolCallChange::Added(_) => added += 1,
            ToolCallChange::Removed(_) => removed += 1,
            ToolCallChange::Reordered { .. } => reordered += 1,
            ToolCallChange::ParamChanged { .. } => param_changed += 1,
        }
    }

    RunDiffOutput {
        events_a: a.len(),
        events_b: b.len(),
        tool_call_changes: tool_diff.changes.len(),
        added,
        removed,
        reordered,
        param_changed,
    }
}

fn render_spec_diff_text(diff: &SpecDiffOutput) -> String {
    let mut out = String::new();
    out.push_str("Spec Diff\n");
    out.push_str("=========\n");
    out.push_str(&format!("changed_paths: {}\n", diff.changed_paths.len()));
    out.push_str(&format!("only_in_a: {}\n", diff.only_in_a.len()));
    out.push_str(&format!("only_in_b: {}\n", diff.only_in_b.len()));

    if !diff.changed_paths.is_empty() {
        out.push_str("\nChanged:\n");
        for p in &diff.changed_paths {
            out.push_str(&format!("  ~ {}\n", p));
        }
    }
    if !diff.only_in_a.is_empty() {
        out.push_str("\nOnly in A:\n");
        for p in &diff.only_in_a {
            out.push_str(&format!("  - {}\n", p));
        }
    }
    if !diff.only_in_b.is_empty() {
        out.push_str("\nOnly in B:\n");
        for p in &diff.only_in_b {
            out.push_str(&format!("  + {}\n", p));
        }
    }

    out.trim_end().to_string()
}

fn render_run_diff_text(diff: &RunDiffOutput) -> String {
    format!(
        "Run Diff\n========\nevents_a: {}\nevents_b: {}\ntool_call_changes: {}\n  added: {}\n  removed: {}\n  reordered: {}\n  param_changed: {}",
        diff.events_a,
        diff.events_b,
        diff.tool_call_changes,
        diff.added,
        diff.removed,
        diff.reordered,
        diff.param_changed
    )
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

// ========== Release Registry Commands (Phase 4) ==========

#[allow(clippy::too_many_arguments)]
async fn cmd_release_promote(
    handle: &SurrealHandle,
    name: &str,
    git_sha: &str,
    graph_digest: &str,
    prompts_digest: &str,
    tools_digest: &str,
    config_digest: &str,
    promoted_by: &str,
    version: Option<&str>,
    notes: Option<&str>,
) -> Result<()> {
    let spec = aivcs_core::AgentSpec::new(
        git_sha.to_string(),
        graph_digest.to_string(),
        prompts_digest.to_string(),
        tools_digest.to_string(),
        config_digest.to_string(),
    )
    .context("failed to build AgentSpec")?;

    let registry = SurrealDbReleaseRegistry::new(Arc::new(handle.clone()));
    let api = aivcs_core::ReleaseRegistryApi::new(registry);

    let release = api
        .promote(
            name,
            &spec,
            promoted_by,
            version.map(ToString::to_string),
            notes.map(ToString::to_string),
        )
        .await
        .context("promote failed")?;

    println!(
        "Promoted {} -> {}",
        release.name,
        release.spec_digest.as_str()
    );
    Ok(())
}

async fn cmd_release_rollback(handle: &SurrealHandle, name: &str) -> Result<()> {
    let registry = SurrealDbReleaseRegistry::new(Arc::new(handle.clone()));
    let release = registry.rollback(name).await?;
    println!(
        "Rolled back {} -> {}",
        release.name,
        release.spec_digest.as_str()
    );
    Ok(())
}

async fn cmd_release_current(handle: &SurrealHandle, name: &str) -> Result<()> {
    let registry = SurrealDbReleaseRegistry::new(Arc::new(handle.clone()));
    let current = registry.current(name).await?;
    match current {
        Some(release) => {
            println!(
                "Current {} -> {}",
                release.name,
                release.spec_digest.as_str()
            );
        }
        None => println!("No release found for {}", name),
    }
    Ok(())
}

async fn cmd_release_history(handle: &SurrealHandle, name: &str) -> Result<()> {
    let registry = SurrealDbReleaseRegistry::new(Arc::new(handle.clone()));
    let history = registry.history(name).await?;

    if history.is_empty() {
        println!("No release history for {}", name);
        return Ok(());
    }

    for release in history {
        println!(
            "{} {} {}",
            release.created_at.to_rfc3339(),
            release.name,
            release.spec_digest.as_str()
        );
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

/// Diff the tool-call sequences of two runs
async fn cmd_diff_runs(ledger: &dyn RunLedger, id_a: &str, id_b: &str) -> Result<()> {
    let (events_a, summary_a) = aivcs_core::replay_run(ledger, id_a)
        .await
        .with_context(|| format!("replay failed for run: {}", id_a))?;
    let (events_b, summary_b) = aivcs_core::replay_run(ledger, id_b)
        .await
        .with_context(|| format!("replay failed for run: {}", id_b))?;

    let diff = diff_tool_calls(&events_a, &events_b);

    println!("A: {} ({})", summary_a.run_id, summary_a.agent_name);
    println!("B: {} ({})", summary_b.run_id, summary_b.agent_name);
    println!();

    if diff.is_empty() {
        println!("Tool-call sequences are identical.");
        return Ok(());
    }

    for change in &diff.changes {
        match change {
            ToolCallChange::Added(call) => {
                println!("  + [{}] {}", call.seq, call.tool_name);
            }
            ToolCallChange::Removed(call) => {
                println!("  - [{}] {}", call.seq, call.tool_name);
            }
            ToolCallChange::Reordered {
                call,
                from_index,
                to_index,
            } => {
                println!(
                    "  ~ {} (pos {} -> {})",
                    call.tool_name, from_index, to_index
                );
            }
            ToolCallChange::ParamChanged {
                tool_name,
                seq_a,
                seq_b,
                deltas,
            } => {
                println!("  Δ {} (A:[{}] / B:[{}])", tool_name, seq_a, seq_b);
                for d in deltas {
                    println!("      {} : {} -> {}", d.key, d.before, d.after);
                }
            }
        }
    }

    println!("\nChanges: {}", diff.changes.len());
    Ok(())
}

/// Run CI stages and record execution
async fn cmd_ci_run(
    workspace: &PathBuf,
    stages_str: &str,
    _no_cache: bool,
    _fix: bool,
) -> Result<()> {
    // Get git SHA
    let git_sha = if let Ok(output) = std::process::Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
    {
        String::from_utf8(output.stdout)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        "unknown".to_string()
    };

    // Get toolchain hash
    let toolchain_hash = if let Ok(output) = std::process::Command::new("rustup")
        .args(&["show", "active-toolchain"])
        .output()
    {
        String::from_utf8(output.stdout)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        "unknown".to_string()
    };

    // Parse stages
    let stage_names: Vec<String> = stages_str
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .collect();

    let mut stage_configs = Vec::new();
    for stage_name in &stage_names {
        let config = match stage_name.as_str() {
            "fmt" => StageConfig::from_builtin(BuiltinStage::CargoFmt, 300),
            "check" => StageConfig::from_builtin(BuiltinStage::CargoCheck, 300),
            "clippy" => StageConfig::from_builtin(BuiltinStage::CargoClippy, 600),
            "test" => StageConfig::from_builtin(BuiltinStage::CargoTest, 1200),
            _ => anyhow::bail!("Unknown stage: {}", stage_name),
        };
        stage_configs.push(config);
    }

    // Create CI spec
    let ci_spec = CiSpec::new(
        workspace.clone(),
        &stage_names,
        git_sha.clone(),
        toolchain_hash.clone(),
    );

    println!("Running CI pipeline for workspace: {:?}", workspace);
    println!("Stages: {}", stages_str);
    println!("Git SHA: {}", git_sha);
    println!();

    // Run pipeline
    let ledger_arc = std::sync::Arc::new(oxidized_state::SurrealRunLedger::from_env().await?);
    let result = CiPipeline::run(ledger_arc.clone(), &ci_spec, stage_configs)
        .await
        .context("CI pipeline failed to run")?;

    // Print results
    println!("Run ID: {}", result.run_id);
    println!("Status: {}", if result.success { "✓ PASSED" } else { "✗ FAILED" });
    println!("Duration: {}ms", result.duration_ms);
    println!();

    for stage_result in &result.stages {
        let status = if stage_result.passed() { "✓" } else { "✗" };
        println!(
            "  {} {} ({}ms, exit code: {})",
            status, stage_result.stage_name, stage_result.duration_ms, stage_result.exit_code
        );
    }

    println!();
    println!("Summary: {}/{} stages passed", result.passed_count(), result.stages.len());

    // Evaluate gate
    let events = ledger_arc
        .get_events(&oxidized_state::RunId(result.run_id.clone()))
        .await?;

    let verdict = CiGate::evaluate(&events);
    println!("Gate: {}", if verdict.passed { "✓ PASSED" } else { "✗ FAILED" });

    if !verdict.violations.is_empty() {
        println!("Violations:");
        for violation in &verdict.violations {
            println!("  - {}", violation);
        }
    }

    if result.success && verdict.passed {
        println!("\n✓ All checks passed!");
        Ok(())
    } else {
        anyhow::bail!("CI checks failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn test_replay_golden_digest_equality() {
        let temp_dir = tempfile::tempdir().unwrap();
        let run_id = "run-golden-1";
        let run_dir = temp_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap();

        let output_bytes = br#"{"result":"ok","tokens":42}"#;
        std::fs::write(run_dir.join("output.json"), output_bytes).unwrap();
        let digest = aivcs_core::Digest::compute(output_bytes).to_hex();
        std::fs::write(run_dir.join("output.digest"), format!("{}\n", digest)).unwrap();

        let replayed = temp_dir.path().join("replayed.json");
        let result = cmd_replay_artifact(run_id, Some(temp_dir.path()), Some(replayed.as_path()));
        assert!(result.is_ok(), "replay failed: {:?}", result.err());

        let written = std::fs::read(replayed).unwrap();
        assert_eq!(written, output_bytes);
    }

    #[test]
    fn test_replay_missing_artifact_rejected() {
        let temp_dir = tempfile::tempdir().unwrap();
        let run_id = "run-missing-1";

        let err = cmd_replay_artifact(run_id, Some(temp_dir.path()), None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Recorded artifact not found"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_spec_diff_json_output_stability() {
        let a = json!({
            "model": "gpt-4",
            "routing": {"strategy": "math"},
            "threshold": 0.9
        });
        let b = json!({
            "model": "gpt-4o",
            "routing": {"strategy": "search"},
            "new_flag": true
        });

        let diff = build_spec_diff(&a, &b);
        let actual = serde_json::to_string_pretty(&diff).unwrap();
        let expected = r#"{
  "changed_paths": [
    "/model",
    "/routing/strategy"
  ],
  "only_in_a": [
    "/threshold"
  ],
  "only_in_b": [
    "/new_flag"
  ]
}"#;

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_run_diff_json_output_stability() {
        let a: Vec<RunEvent> = serde_json::from_value(json!([{
            "seq": 1,
            "kind": "tool_called",
            "payload": {"tool_name":"search","query":"rust"},
            "timestamp": "2026-01-01T00:00:00Z"
        }]))
        .unwrap();
        let b: Vec<RunEvent> = serde_json::from_value(json!([{
            "seq": 1,
            "kind": "tool_called",
            "payload": {"tool_name":"search","query":"python"},
            "timestamp": "2026-01-01T00:00:00Z"
        }]))
        .unwrap();

        let diff = build_run_diff(&a, &b);
        let actual = serde_json::to_string_pretty(&diff).unwrap();
        let expected = r#"{
  "events_a": 1,
  "events_b": 1,
  "tool_call_changes": 1,
  "added": 0,
  "removed": 0,
  "reordered": 0,
  "param_changed": 1
}"#;

        assert_eq!(actual, expected);
    }
}
