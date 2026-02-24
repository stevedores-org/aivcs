# Contributing to AIVCS

This is a practical map for contributors who want to make focused, high-value changes quickly.

## Local Setup

```bash
git clone https://github.com/stevedores-org/aivcs.git
cd aivcs
cargo test --all
```

## Start Contributing Map

### 1) Core domain and orchestration
- Primary files: `crates/aivcs-core/src/domain/*`, `crates/aivcs-core/src/lib.rs`
- Use this area for snapshot/run/release models, domain invariants, and orchestration behavior.

### 2) Diff, replay, and observability flows
- Primary files: `crates/aivcs-core/src/diff/*`, `crates/aivcs-core/src/replay.rs`, `crates/aivcs-core/src/obs.rs`, `crates/aivcs-core/src/telemetry.rs`
- Tests: `crates/aivcs-core/tests/*`
- Use this area for change tracking, run replay correctness, and runtime instrumentation.

### 3) CI/gates/promotion pipeline logic
- Primary files: `crates/aivcs-ci/src/*`, `crates/aivcs-core/src/gate.rs`, `crates/aivcs-core/src/publish_gate.rs`, `crates/aivcs-core/src/deploy*`
- Tests: `crates/aivcs-ci/tests/pipeline_integration.rs`, `crates/aivcs-core/tests/merge_gate.rs`, `crates/aivcs-core/tests/publish_gate.rs`
- Use this area for verification stages, promotion checks, and release gating semantics.

### 4) Storage and state backend
- Primary files: `crates/oxidized-state/src/*`
- Tests: `crates/oxidized-state/tests/*`
- Use this area for schema, handles, storage traits, SurrealDB interactions, and migration behavior.

### 5) Environment and reproducibility tooling
- Primary files: `crates/nix-env-manager/src/*`
- Use this area for flake hashing, logic hashing, and Attic/cache integration.

### 6) Semantic merge and RAG behavior
- Primary files: `crates/semantic-rag-merge/src/lib.rs`
- Use this area for memory diff/merge logic and conflict resolution behavior.

### 7) User-facing surfaces (CLI and daemon)
- CLI: `crates/aivcs-cli/src/main.rs`
- Daemon: `crates/aivcsd/src/main.rs`
- Use this area for command ergonomics and API/daemon entrypoints.

### 8) Docs and runbooks
- Primary files: `README.md`, `docs/*.md`, `docs/runbooks/*.md`
- Keep docs aligned with implemented behavior and command outputs.

## Workflow

1. Branch from `develop`.
2. Keep one change theme per PR.
3. Add/update tests for behavior changes.
4. Run:

```bash
cargo test --all
```

5. Open PR to `develop` with:
- problem statement
- approach and design notes
- test evidence
- compatibility/risk notes

## Review Expectations

- Correctness before optimization.
- Explicit error semantics (avoid silent fallback in core paths).
- Compatibility impact called out when behavior changes.
- Tests cover happy path and key failure modes.
