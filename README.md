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
| `fork` | Fork multiple parallel branches for exploration |
| `trace` | Time-travel debugging - show reasoning trace |
| `replay` | Replay a recorded run artifact by run ID |
| `diff-runs` | Diff the tool-call sequences of two runs |
| `pr open` | Open a GitHub Pull Request and request review from the Librarian Agent |

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

### Parallel Simulation Commands (Phase 4)

```bash
# Fork 5 parallel branches from main for exploration
aivcs fork main --count 5 --prefix experiment

# Fork 3 branches from a specific commit
aivcs fork abc123 -c 3 -p strategy

# Show reasoning trace (time-travel debugging)
aivcs trace main

# Show trace with more depth
aivcs trace experiment-0 --depth 50
```

### A2A CODE_COMMITTED Events

`aivcs snapshot` and `aivcs merge` can notify an A2A JSON-RPC transport after an AIVCS commit is durably stored. Emission is opt-in and best-effort: transport failures are logged and retried with bounded exponential backoff, but they do not fail the local commit operation.

```bash
export AIVCS_A2A_JSONRPC_URL="https://a2a.example.com/jsonrpc"
export AIVCS_AGENT_ID="builder-agent"
export AIVCS_JOB_ID="agent-job-123"
aivcs snapshot --state state.json --message "Update state" --branch develop
```

Optional settings:

| Variable | Description |
|----------|-------------|
| `AIVCS_A2A_JSONRPC_URL` | Enables JSON-RPC event emission when set |
| `AIVCS_A2A_JSONRPC_METHOD` | Overrides the default method, `a2a.events.publish` |
| `AIVCS_AGENT_ID` | Authoring agent ID; falls back to the snapshot author |
| `AIVCS_JOB_ID` | Ephemeral job ID included in the event payload |
| `GITHUB_REPOSITORY` | Repository in `owner/name` form; otherwise detected from `origin` |

The JSON-RPC params contain the AIVCS commit hash. Snapshot events include the state file path in `changed_paths`; merge events may emit an empty list because they merge persisted AIVCS state rather than filesystem paths.

```json
{
  "event": {
    "kind": "CODE_COMMITTED",
    "payload": {
      "repo": "stevedores-org/aivcs",
      "branch": "develop",
      "commit_sha": "<aivcs-commit-hash>",
      "changed_paths": ["state.json"],
      "authoring_agent_id": "builder-agent",
      "job_id": "agent-job-123",
      "timestamp": "2026-05-27T00:00:00Z"
    }
  }
}
```

### Git forge integration (`pr pipeline` — GitHub or GitLab)

Autonomous agents branch, commit, and open **PRs (GitHub) or MRs (GitLab)** via the
forge API. Host selection: `AIVCS_GIT_HOST=github|gitlab` (default: GitLab when
`GITLAB_TOKEN` / GitLab CI env is present).

| Host | Token env |
|------|-----------|
| GitHub | `GITHUB_TOKEN` or `GITHUB_TOKEN_FILE` |
| GitLab | `GITLAB_TOKEN` or `GITLAB_TOKEN_FILE` |

```bash
export AIVCS_GIT_HOST=gitlab
export GITLAB_TOKEN="<gitlab-project-or-group-token>"

uv run aivcs pr pipeline \
  --branch feature/my-change \
  --base develop \
  --path docs/example.md \
  --file ./example.md \
  --message "docs: add example" \
  --title "feat: my change" \
  --body "Summary." \
  --owner lornu-ai \
  --repo infra-code
```

See [Sovereign infra (GitLab, no GHA)](docs/runbooks/sovereign-infra-gitlab.md).

### Sovereign infra (`aivcs infra` — no GitHub Actions)

In-cluster reconcilers for Cloudflare LB hygiene and Flux handoff:

```bash
# Audit CF pools vs git allowlist (exit 2 on orphan drift)
aivcs infra cloudflare-lb audit --allowlist policy/cloudflare-lb-allowlist.txt

# Prune unreferenced legacy pools (e.g. aks-lornu-hub)
aivcs infra cloudflare-lb prune --allowlist policy/cloudflare-lb-allowlist.txt --dry-run

# Resume a suspended Flux Kustomization after MR merge
export FLUX_CONTEXT=gke_gcp-lornu-ai_us-central1_lornu-gke-prod
aivcs infra flux reconcile --kustomization cloudflare-lb-aivcs-io --with-source
```

### OCI publish (`aivcs oci` — GitLab CI, no GHA)

Nix OCI → GAR via Rust CLI (skopeo). See [oci-publish-gitlab.md](docs/runbooks/oci-publish-gitlab.md).

```bash
cargo run -p aivcs-cli -- oci publish --target aivcs-cli --dry-run
GCP_ACCESS_TOKEN="$(gcloud auth print-access-token)" \
  cargo run -p aivcs-cli -- oci publish --target aivcs-cli
```

### GitHub Integration (legacy path)

GitHub remains supported when `AIVCS_GIT_HOST=github` (default without GitLab token):

```bash
export GITHUB_TOKEN="<github-app-installation-token-or-pat>"
export RELIC_LIBRARIAN_USERNAME="librarian-bot"

uv run aivcs pr pipeline \
  --branch feature/my-change \
  --base develop \
  --path docs/example.md \
  --file ./example.md \
  --message "docs: add example" \
  --title "feat: my change" \
  --body "Summary of what changed." \
  --owner stevedores-org \
  --repo aivcs
```

Step-by-step (GitHub or GitLab — same flags):

```bash
aivcs pr branch --name feature/my-change --base develop --owner stevedores-org --repo aivcs

aivcs pr commit \
  --branch feature/my-change \
  --path docs/example.md \
  --file ./example.md \
  --message "docs: add example" \
  --owner stevedores-org \
  --repo aivcs

aivcs pr open \
  --owner stevedores-org \
  --repo aivcs \
  --head feature/my-change \
  --base develop \
  --title "feat: my change" \
  --body  "Summary of what changed."
```


### **Zero-Trust MCP Agent Authentication**

This repository ships the MCP identity stack for repo-scoped agent tools:

| Crate | Port | Role |
|-------|------|------|
| `aivcs-auth` | 8081 | Workload bootstrap → JWT |
| `aivcs-mcp-gateway` | 8082 | Scope/risk, HITL approvals, revocation |

- **Canonical guide:** [docs/mcp-auth-guide.md](docs/mcp-auth-guide.md)
- **Agent skill:** `.cursor/skills/mcp-auth/SKILL.md`
- **Run locally:** `cargo run -p aivcs-auth` and `cargo run -p aivcs-mcp-gateway`

## **Development Standards**

### **The 7-File Rule (Documentation as Infrastructure)**

Treating documentation as infrastructure ensures all AI assistants (Cursor, Windsurf, Copilot) operate with synchronized context, preventing schema drift and maintaining the **Sovereign Knowledge Fabric.**

**Mandatory Context Files:**
1. **`.cursorrules`**: Local IDE rules.
2. **`AGENTS.md`**: A2A Protocol and Agent capabilities.
3. **`CLAUDE.md`**: Build/test commands and project context.
4. **`README.md`**: High-level project map.
5. **`ARCH_PRESERVE.md`**: Architectural preservation rules.
6. **`.github/copilot-instructions.md`**: GitHub Copilot context.
7. **`.github/system-instruction.md`**: Global "Sovereign Intelligence" standards.

**When to update:**
- New features added to `apps/` or `app-agents/`
- Changes to deployment patterns, build commands, or infrastructure
- New standards, conventions, or governance policies

### **Agent Workflow (Autonomous Operation Loop)**

`aivcs pr open` creates a Pull Request via the GitHub API and, by default, requests review from the Librarian Agent so it can audit changes before downstream OCI builds. This is the canonical PR-creation path used by autonomous builder agents running in ephemeral ADK Jobs.

Required environment:

| Variable | Description |
|----------|-------------|
| `GITHUB_TOKEN` | Bearer token for the GitHub API. Accepts a GitHub App installation token (preferred for autonomous Jobs) or a personal access token. |
| `GITHUB_TOKEN_FILE` | Alternative token source: path to a file containing the bearer token (typical ESO secret volume mount). Used when `GITHUB_TOKEN` is unset or whitespace-only. |
| `RELIC_LIBRARIAN_USERNAME` | GitHub username of the Librarian Agent. Required when `--librarian` is enabled (the default). Missing or whitespace-only values are rejected eagerly so the failure surfaces before the API call rather than mid-pipeline. |

### **Pull Request Requirements**

The `--librarian` flag defaults to `true`; pass `--librarian=false` to skip the review request in development or test contexts where the Librarian is not deployed.

See [Zero-Touch PR Pipeline](docs/runbooks/zero-touch-pr-pipeline.md) for the full ADK Job runbook.

#### A2A `CODE_COMMITTED` emission

`aivcs snapshot`, `aivcs merge`, `aivcs pr commit`, and `aivcs pr pipeline` emit a `CODE_COMMITTED` Agent-to-Agent event when the JSON-RPC URL is configured. The repo is resolved from `GITHUB_REPOSITORY` if set, otherwise from `git remote get-url origin`. The emission is no-op when the URL var is absent.

| Variable | Description |
|----------|-------------|
| `AIVCS_A2A_JSONRPC_URL` | JSON-RPC endpoint to POST the event to. Absent or whitespace-only ⇒ no emission. |
| `AIVCS_A2A_JSONRPC_METHOD` | Override the JSON-RPC method name. Defaults to `a2a.events.publish`. |
| `AIVCS_AGENT_ID` | Authoring agent identifier in the event payload. Falls back to the local commit author. |
| `AIVCS_JOB_ID` | Optional job/run correlation ID. Whitespace-only values are dropped. |

> ⚠️ The emission is awaited synchronously inside `snapshot` / `merge` / `pr commit` / `pr pipeline`. Transport failures retry per `A2aRetryPolicy::default()` before returning; the CLI blocks for that window. Pin the retry policy if you tighten snapshot-latency SLOs.

## **Infrastructure & Deployment**

### **GitOps Architecture (Flux + ArgoCD + Crossplane + ESO)**

Grow Without Limits uses a **declarative GitOps model** with clear separation of concerns:

- **Flux CD**: Primary GitOps engine for cluster bootstrapping and application delivery.
- **ArgoCD**: Visual orchestrator for multi-cluster workload management.
- **Crossplane**: Control plane for managing cloud resources (AWS, GCP, Azure) as Kubernetes objects.
- **External Secrets Operator (ESO)**: Secure secret injection from AWS Secrets Manager/Azure Key Vault.

#### `pr commit` and file content

The `pr commit` and `pr pipeline` commands support both text and binary files. Content is base64-encoded and sent via the GitHub Contents API. This allows committing generated diagrams (PNG), compiled artifacts, or other non-UTF-8 assets.

## Documentation

- [Getting Started](docs/getting-started.md) — prerequisites, install, first-run walkthrough
- [NixOS-WSL Runbook](docs/runbooks/nixos-wsl.md) — build/import the WSL tarball, validate the environment
- [Architecture](docs/architecture.md) — crate layers, data flow, key abstractions
- [Local Development](docs/runbooks/local-development.md) — build, test, dev workflows
- [Database Configuration](docs/runbooks/database-configuration.md) — in-memory, local, cloud setup
- [CI Troubleshooting](docs/runbooks/ci-troubleshooting.md) — common failures, reproduce locally
- [crates.io Release](docs/CRATES_IO_RELEASE.md) — publish library crates (tag → `publish.yml`)
- [Zero-Touch PR Pipeline](docs/runbooks/zero-touch-pr-pipeline.md) — autonomous agent Jobs: branch, commit, PR, A2A

## Contributing

- Start here: [CONTRIBUTING.md](CONTRIBUTING.md)
- Agent/operator workflow rules: [CODEX.md](CODEX.md)

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

## Database Configuration

AIVCS supports both in-memory (local development) and SurrealDB Cloud (production) backends.

### Local Development (Default)

No configuration needed - uses in-memory SurrealDB automatically.

### SurrealDB Cloud

1. Create an account at [SurrealDB Cloud](https://surrealdb.com/cloud)
2. Create a database user with Editor/Owner role at `Authentication > Database Users`
3. Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
```

```env
SURREALDB_ENDPOINT=wss://YOUR_INSTANCE.aws-use1.surrealdb.cloud
SURREALDB_USERNAME=your_username
SURREALDB_PASSWORD=your_password
SURREALDB_NAMESPACE=aivcs
SURREALDB_DATABASE=main
```

The library automatically detects cloud credentials:
- If `SURREALDB_ENDPOINT` is set, connects to cloud
- Otherwise, falls back to in-memory database

## Tech Stack

- **Rust** - Core implementation
- **SurrealDB** - Graph + Document database for commits and state (local or cloud)
- **Nix/Attic** - Hermetic environment versioning (Phase 2)
- **Tokio** - Async runtime for parallel exploration

## Phase Roadmap

| Phase | Status | Features |
|-------|--------|----------|
| 1 - Snapshot Core | ✅ Complete | commit, restore, branch, log |
| 2 - Environment Lock | ✅ Complete | Nix Flake hashing, Attic cache, logic hashing |
| 3 - Semantic Merge | ✅ Complete | Memory diff, conflict arbiter, memory synthesis |
| 4 - Parallel Simulation | ✅ Complete | Concurrent fork, branch pruning, time-travel trace |

## References

- [AgentGit Paper](https://arxiv.org/abs/...) - Original research on Git-like rollback for LLM agents
- [Issue #1-4](https://github.com/stevedores-org/aivcs/issues) - Architecture and TDD plans

## **Official Contact Information**

- **Contact**: `contact@lornu.ai` - Official email for all external communications
- **Policy**: Only publish `contact@lornu.ai` in documentation and UI. Never publish personal or alternative email addresses.

## License

Apache-2.0

 
