# Getting Started with AIVCS

## Prerequisites

| Requirement | Version | Notes |
|---|---|---|
| Rust toolchain | stable (1.75+) | Install via [rustup](https://rustup.rs/) |
| Git | 2.x | For SHA linking and repo detection |
| Nix *(optional)* | 2.18+ | Only needed for `aivcs env` commands |
| Attic *(optional)* | latest | Only needed for binary cache features |

## Installation

```bash
# Clone
git clone https://github.com/stevedores-org/aivcs.git
cd aivcs

# Build release binary
cargo build --release

# The binary is at:
./target/release/aivcs --version
```

## First-Run Walkthrough

### 1. Initialise a repository

```bash
aivcs init
```

This creates an initial commit and a `main` branch in the backing SurrealDB store.

### 2. Create a state snapshot

```bash
echo '{"step": 1, "memory": "learned X"}' > state.json
aivcs snapshot --state state.json --message "First snapshot"
```

### 3. View history

```bash
aivcs log
```

### 4. Branch and explore

```bash
aivcs branch create experiment-1
aivcs snapshot --state state.json --message "Experiment" --branch experiment-1
```

### 5. Merge back

```bash
aivcs merge experiment-1 --target main
```

### 6. Restore a previous state

```bash
aivcs restore <commit-id> --output restored.json
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `SURREALDB_ENDPOINT` | *(in-memory)* | WebSocket URL for SurrealDB Cloud |
| `SURREALDB_USERNAME` | — | Database user |
| `SURREALDB_PASSWORD` | — | Database password |
| `SURREALDB_NAMESPACE` | `aivcs` | SurrealDB namespace |
| `SURREALDB_DATABASE` | `main` | SurrealDB database name |
| `ATTIC_SERVER` | — | Attic binary cache server URL |
| `ATTIC_CACHE` | — | Attic cache name |
| `ATTIC_TOKEN` | — | Attic authentication token |
| `RUST_LOG` | `info` | Tracing filter (e.g. `debug`, `aivcs_core=trace`) |

## Next Steps

- [Architecture overview](./architecture.md)
- [Local development runbook](./runbooks/local-development.md)
- [Database configuration runbook](./runbooks/database-configuration.md)
