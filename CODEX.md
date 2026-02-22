# CODEX.md

Operating instructions for AI/code agents working in this repository.

## Scope

- Applies to the full workspace.
- Prefer focused, minimal diffs per PR.

## Branch and PR Rules

- Base branch: `develop`.
- Create feature branches from `develop`.
- Keep PRs scoped to a single change theme.

## Read First

- `README.md`
- `CONTRIBUTING.md`
- `docs/architecture.md`
- `docs/runbooks/local-development.md`
- `docs/runbooks/ci-troubleshooting.md`

## Module Ownership Map

- Core domain/orchestration: `crates/aivcs-core/src/domain/*`, `crates/aivcs-core/src/lib.rs`
- Replay/diff/telemetry: `crates/aivcs-core/src/replay.rs`, `crates/aivcs-core/src/diff/*`, `crates/aivcs-core/src/obs.rs`, `crates/aivcs-core/src/telemetry.rs`
- CI/pipeline/gates: `crates/aivcs-ci/src/*`, `crates/aivcs-core/src/gate.rs`, `crates/aivcs-core/src/publish_gate.rs`
- Storage backend: `crates/oxidized-state/src/*`
- Env manager: `crates/nix-env-manager/src/*`
- Semantic merge: `crates/semantic-rag-merge/src/lib.rs`
- CLI/daemon: `crates/aivcs-cli/src/main.rs`, `crates/aivcsd/src/main.rs`

## Coding Expectations

- Preserve public behavior unless the PR explicitly changes contract.
- Prefer explicit errors over silent fallback.
- Avoid unrelated refactors.

## Validation

Run at minimum:

```bash
cargo test --all
```

If touching style/lints and feasible:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features
```

## PR Checklist

- State problem and approach clearly.
- Include test evidence.
- Call out compatibility and risk impacts.
- Keep changes aligned with module ownership map.
