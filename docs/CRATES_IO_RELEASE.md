# crates.io release runbook (AIVCS)

How to publish AIVCS **library crates** to [crates.io](https://crates.io). Owner: **`community-stevedores-org`**.

Binary CLI releases (GitHub Releases) stay in [`.github/workflows/release.yml`](../.github/workflows/release.yml). Crate publishing is [`.github/workflows/publish.yml`](../.github/workflows/publish.yml).

## Published crates

| Crate | Role | Notes |
|-------|------|-------|
| `oxidized-state` | Layer 0 — SurrealDB persistence | leaf |
| `nix-env-manager` | Layer 2 — Nix/Attic | leaf |
| `semantic-rag-merge` | Layer 3 — semantic merge | depends on `oxidized-state` |
| `aivcs-core` | Domain orchestration | depends on layers + `oxidizedgraph` |
| `aivcs-ci` | CI pipeline / gates | depends on `aivcs-core` |

**Not published:** `aivcs-cli`, `aivcsd` (`publish = false` in their `Cargo.toml`).

**External dependency:** `aivcs-core` pins `oxidizedgraph = "0.2.0"` from crates.io — publish or verify `oxidizedgraph` before bumping AIVCS if that bound changes.

## Registry strategy

| Target | When to use |
|--------|-------------|
| **crates.io** (default) | Public library crates consumed outside the monorepo |
| **`crates.stevedores.org`** | Private/pre-release (see [oxidizedgraph publish workflow](https://github.com/stevedores-org/oxidizedgraph/blob/main/.github/workflows/publish.yml)) |
| **Path deps** | `lornu-ai/brains`, in-flight cross-repo work — no publish required |

## Prerequisites

### Local token (macOS Keychain)

```bash
export CARGO_REGISTRY_TOKEN=$(security find-generic-password -s "CARGO_REGISTRY_TOKEN" -w)
```

Store once:

```bash
security add-generic-password -a "$USER" -s "CARGO_REGISTRY_TOKEN" -w "<crates.io API token>"
```

### GitHub Actions secret

Repo **`stevedores-org/aivcs`** → Settings → Secrets → `CARGO_REGISTRY_TOKEN` (same crates.io API token, scoped to publish).

## Release checklist

1. **Merge** changes to `main` (CI green).
2. **Bump** `[workspace.package] version` in root `Cargo.toml` and matching `[workspace.dependencies]` version pins for internal crates.
3. **Update** `CHANGELOG.md` under `[Unreleased]` → new version section.
4. **Dry-run locally:**
   ```bash
   ./scripts/publish-crates.sh --dry-run
   ```
5. **Tag** (semver, with `v` prefix):
   ```bash
   git tag -a v0.3.2 -m "Release v0.3.2"
   git push origin v0.3.2
   ```
6. **Verify** [publish workflow](https://github.com/stevedores-org/aivcs/actions/workflows/publish.yml) — test job → `publish-crates-io`.
7. **Confirm** on crates.io: `oxidized-state`, `nix-env-manager`, `semantic-rag-merge`, `aivcs-core`, `aivcs-ci` all show the new version.

### Manual publish (fallback)

```bash
export CARGO_REGISTRY_TOKEN=$(security find-generic-password -s "CARGO_REGISTRY_TOKEN" -w)
./scripts/publish-crates.sh
```

### CI dry-run (no upload)

Actions → **publish** → **Run workflow** → leave **dry_run** checked.

## Publish order

Enforced by [`scripts/publish-crates.sh`](../scripts/publish-crates.sh):

```text
oxidized-state → nix-env-manager → semantic-rag-merge → aivcs-core → aivcs-ci
```

`cargo publish` rewrites path dependencies to semver requirements from `Cargo.toml`.

## Related repos

| Repo | Publish workflow |
|------|------------------|
| [stevedores-org/aivcs](https://github.com/stevedores-org/aivcs) | `publish.yml` (this doc) |
| [stevedores-org/oxidizedgraph](https://github.com/stevedores-org/oxidizedgraph) | `publish.yml` (single crate + optional `crates.stevedores.org`) |

Agent skill: [lornu.ai-agent-skills `crates-io`](https://github.com/lornu-ai/lornu.ai-agent-skills/tree/main/skills/crates-io).
