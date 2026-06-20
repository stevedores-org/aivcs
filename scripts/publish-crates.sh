#!/usr/bin/env bash
# Publish AIVCS library crates to crates.io in dependency order.
# aivcs-cli and aivcsd are publish = false and are skipped.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=true
fi

# Leaves first, then dependents. Matches workspace [workspace.dependencies].
CRATES=(
  oxidized-state
  nix-env-manager
  semantic-rag-merge
  aivcs-core
  aivcs-ci
)

if [[ "$DRY_RUN" == false && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "error: CARGO_REGISTRY_TOKEN is not set" >&2
  echo "helper: export CARGO_REGISTRY_TOKEN=\$(security find-generic-password -s \"CARGO_REGISTRY_TOKEN\" -w)" >&2
  exit 1
fi

WORKSPACE_VERSION="$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
echo "workspace version: ${WORKSPACE_VERSION}"

for crate in "${CRATES[@]}"; do
  echo "==> ${crate}"
  if [[ "$DRY_RUN" == true ]]; then
    cargo publish -p "$crate" --dry-run
  else
    cargo publish -p "$crate"
  fi
done

echo "done: ${#CRATES[@]} crate(s)"
