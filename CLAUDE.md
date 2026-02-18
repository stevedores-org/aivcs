# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

AIVCS (AI Agent Version Control System) implements AgentGit 2.0 — Git-like version control (commits, branches, merging) for AI agent workflows. It enables state snapshots, rollbacks, parallel exploration via branch forking, and semantic merging with LLM-assisted conflict resolution.

## Build & Test Commands

```bash
cargo build                              # Dev build
cargo build --release                    # Release build (LTO, stripped)
cargo test --all                         # Run all tests (~190 tests)
cargo test -p oxidized-state             # Run tests for a single crate
cargo test test_snapshot_is_atomic       # Run a specific test by name
cargo clippy --all -- -D warnings        # Lint (CI enforces zero warnings)
cargo fmt --all -- --check               # Check formatting
```

CI runs `local-ci --json` (a Go tool from `stevedores-org/local-ci`) which executes Rust tests + clippy. Nix checks (`nix flake check`) run separately as report-only (non-blocking).

## Architecture

Six crates in a layered architecture:

**Layer 0 — `oxidized-state`** (persistence): SurrealDB backend. `SurrealHandle` manages connections; records include `SnapshotRecord`, `CommitRecord`, `BranchRecord`, `MemoryRecord`, `GraphEdge`, `RunRecord`. Uses in-memory SurrealDB by default; production uses WebSocket to SurrealDB Cloud (`SURREALDB_ENDPOINT` env var).

**Layer 1 — `aivcs-core`** (domain logic): Orchestration layer. Modules: `cas/` (content-addressed store with SHA256 digests), `git/` (git HEAD capture), `domain/` (business types: AgentSpec, Run, Release, Event), `parallel/` (concurrent branch forking), `recording/` (execution ledger), `diff/` (tool-call sequence diffing).

**Layer 2 — `nix-env-manager`** (environment): Nix Flakes + Attic binary cache integration. Generates environment hashes from flake.lock and logic hashes from Rust source.

**Layer 3 — `semantic-rag-merge`** (merge logic): Memory vector diffing and semantic merge with heuristic conflict resolution (prefers longer content). Depends on `oxidized-state`.

**Layer 4 — `aivcs-cli`** (binary): Clap-based CLI. Commands: init, snapshot, restore, branch, log, merge, diff, env, fork, trace, replay.

**`aivcsd`** — daemon stub (placeholder).

## Dependency Flow

```
aivcs-cli → aivcs-core → oxidized-state
                       → nix-env-manager
                       → semantic-rag-merge → oxidized-state
```

## Key Patterns

- **Async-first**: Tokio runtime everywhere; `async-trait` for trait objects; `Arc<SurrealHandle>` for shared concurrent DB access.
- **Content addressing**: SHA256 digests for state deduplication in the CAS layer.
- **Error handling**: `thiserror` enums per crate (`StateError`, `NixError`, `CasError`); `anyhow` at the CLI boundary.
- **Tests**: Co-located in each file (`mod tests`). DB tests use `SurrealHandle::setup_db()` (in-memory). Filesystem tests use `tempfile::tempdir()`. Mix of `#[test]` and `#[tokio::test]`.
