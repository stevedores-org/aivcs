# CI Troubleshooting Runbook

## CI Overview

CI runs on every push to `develop`/`main` and on all pull requests. Two jobs:

| Job | Tool | Blocking? |
|---|---|---|
| `og-crab` | `lornu-ai/og-crab` (Rust) | Yes |
| `nix-report` | `nix flake check` | No (report-only) |

### og-crab

`og-crab run` runs the checks defined in `propel.toml`. For AIVCS this includes:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

### nix-report

Runs `nix flake check` and uploads the log as a build artifact. Failures here do **not** block merge.

## Common Failures

### Clippy warnings

```
error: ... implied by `-D warnings`
```

**Fix:** Run `cargo clippy --all -- -D warnings` locally, fix all warnings, then push.

### Format check failure

```
Diff in src/foo.rs
```

**Fix:** Run `cargo fmt --all` locally, commit the formatted files.

### Test failure

**Fix:** Run `cargo test --all` locally. For flaky tests involving SurrealDB, ensure no global state leaks between tests (each test should call `SurrealHandle::setup_db()` for a fresh in-memory instance).

### og-crab not found

```
og-crab: command not found
```

Install it from source:

```bash
cargo install --git https://github.com/lornu-ai/og-crab og-crab
og-crab run
```

### Nix flake check failure (non-blocking)

Check the uploaded `nix-flake-check-log` artifact in the GitHub Actions run. Common issues:

- Missing flake input → update `flake.lock` with `nix flake update`
- Build failure → check Rust compilation errors in the Nix derivation

## Reproduce CI Locally

```bash
# Exact CI sequence
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Or use og-crab directly
cargo install --git https://github.com/lornu-ai/og-crab og-crab
og-crab run
```
