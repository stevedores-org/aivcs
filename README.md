# AIVCS - AI Agent Version Control System

A Rust-based version control system for AI agents, implementing **AgentGit 2.0** concepts for state rollback, branching, and semantic merging.

## Overview

AIVCS brings Git-like version control to AI agent workflows:

- **State Commits**: Save agent state snapshots with full rollback capability
- **Branching**: Create parallel exploration paths for different strategies
- **Semantic Merging**: LLM-assisted conflict resolution for agent memories
- **Time-Travel Debugging**: Trace agent reasoning through commit history

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   AIVCS-Core (CLI)                  │
│          aivcs init | snapshot | restore            │
└─────────────────────┬───────────────────────────────┘
                      │
        ┌─────────────┼─────────────┐
        │             │             │
        ▼             ▼             ▼
┌───────────────┐ ┌───────────┐ ┌──────────────────┐
│ Oxidized-State│ │ Nix-Env   │ │ Semantic-RAG     │
│ (SurrealDB)   │ │ Manager   │ │ Merge            │
│               │ │           │ │                  │
│ • Commits     │ │ • Flake   │ │ • Memory diff    │
│ • Snapshots   │ │   hashing │ │ • LLM arbiter    │
│ • Graph edges │ │ • Attic   │ │ • Vector merge   │
└───────────────┘ └───────────┘ └──────────────────┘
```

## Quick Start

```bash
# Build
cargo build --release

# Initialize repository
./target/release/aivcs init

# Create agent state snapshot
echo '{"step": 1, "memory": "learned X"}' > state.json
./target/release/aivcs snapshot --state state.json --message "Initial state"

# View history
./target/release/aivcs log

# Create a branch
./target/release/aivcs branch create experiment-1

# Restore previous state
./target/release/aivcs restore <commit-id> --output restored.json
```

## Commands

| Command | Description |
|---------|-------------|
| `init` | Initialize a new AIVCS repository |
| `snapshot` | Create a versioned checkpoint of agent state |
| `restore` | Restore agent to a previous state |
| `branch` | Manage branches (list, create, delete) |
| `log` | Show commit history |
| `merge` | Merge two branches with semantic resolution |
| `diff` | Show differences between commits/branches |
| `env` | Environment management (Nix/Attic integration) |

### Environment Commands (Phase 2)

```bash
# Show environment hash for a Nix Flake
aivcs env hash /path/to/flake

# Show logic hash (Rust source code)
aivcs env logic-hash src/

# Check Attic cache status
aivcs env cache-info

# Check if environment is cached
aivcs env is-cached <hash>

# Show system info (Nix/Attic availability)
aivcs env info
```

## Crate Structure

- **aivcs-core**: Main CLI and orchestration logic
- **oxidized-state**: SurrealDB backend for state persistence
- **nix-env-manager**: Nix Flakes and Attic integration for reproducible environments
- **semantic-rag-merge**: RAG-based semantic merging with LLM conflict resolution

## Development

```bash
# Run tests
cargo test --all

# Run specific test
cargo test test_snapshot_is_atomic

# Build with verbose output
cargo build --release -v
```

## Tech Stack

- **Rust** - Core implementation
- **SurrealDB** - Graph + Document database for commits and state
- **Nix/Attic** - Hermetic environment versioning (Phase 2)
- **Tokio** - Async runtime for parallel exploration

## Phase Roadmap

| Phase | Status | Features |
|-------|--------|----------|
| 1 - Snapshot Core | ✅ Complete | commit, restore, branch, log |
| 2 - Environment Lock | ✅ Complete | Nix Flake hashing, Attic cache, logic hashing |
| 3 - Semantic Merge | 🔜 | LLM arbiter, memory synthesis |
| 4 - Parallel Simulation | 🔜 | Concurrent exploration, pruning |

## References

- [AgentGit Paper](https://arxiv.org/abs/...) - Original research on Git-like rollback for LLM agents
- [Issue #1-4](https://github.com/stevedores-org/aivcs/issues) - Architecture and TDD plans

## License

Apache-2.0
