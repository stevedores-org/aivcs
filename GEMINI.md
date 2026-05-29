# aivcs — Gemini CLI Instructions

## Project Context

**aivcs** is the AI Agent Version Control System. It provides state commits, branching, and semantic merging for agent workflows. It serves as the content-addressed ledger for the Lornu Sovereign Stack.

## Core Mandates

1. **Safety First**: Ensure all state captures are deterministic and content-addressed (content-hash based IDs).
2. **Persistence**: The `RunLedger` must support both in-memory (for testing) and persistent (Postgres/SurrealDB) backends.
3. **Auditability**: Every agent decision and state transition must be recordable via the A2A protocol.
4. **Resilience**: The A2A transport must handle timeouts (10s default) and intermittent connectivity issues gracefully.

## Development Patterns

- **Trait boundaries**: Program against `RunLedger` and `EventSink` traits to enable backend polymorphism.
- **TDD-First**: Add integration tests in `crates/aivcs-core/tests` for any core API changes.
- **Clippy hygiene**: Maintain zero warnings workspace-wide (`-D warnings`).

## Required Files (7-File Rule)

1. `README.md`
2. `CLAUDE.md`
3. `AGENTS.md`
4. `GEMINI.md` (this file)
5. `.cursorrules`
6. `.github/copilot-instructions.md`
7. `.github/system-instruction.md`

## Useful Commands

```bash
# Run all tests
cargo test --workspace

# Run AIVCS daemon
cargo run -p aivcsd

# Check clippy
cargo clippy --all-targets --all-features -- -D warnings
```
