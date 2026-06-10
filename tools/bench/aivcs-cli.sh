#!/usr/bin/env bash
# tools/bench/aivcs-cli.sh — wall-clock bench of aivcs-cli hot paths via hyperfine.
#
# Scenarios:
#   1. cold init+snapshot — full process boot + DB init + first I/O
#   2. warm snapshot      — repeat snapshots into an already-initialised dir
#   3. pr-note            — generate the PR linkage block (issue #220 Phase 0 path)
#
# Env overrides:
#   AIVCS         path to the aivcs binary (default: target/release/aivcs from repo root)
#   BENCH_OUT     if set, hyperfine markdown summaries are written into this dir
#   BENCH_WARMUP  hyperfine --warmup count (default: 3)
#   BENCH_RUNS    hyperfine --runs count (default: 10)
#
# Exit codes:
#   0 ok / 1 setup error / 2 hyperfine missing / 3 binary missing
set -euo pipefail

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "error: hyperfine not found on PATH" >&2
  echo "       install via: brew install hyperfine   (macOS)" >&2
  echo "                    cargo install hyperfine  (any)"   >&2
  exit 2
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
AIVCS="${AIVCS:-$REPO_ROOT/target/release/aivcs}"
WARMUP="${BENCH_WARMUP:-3}"
RUNS="${BENCH_RUNS:-10}"

if [[ ! -x "$AIVCS" ]]; then
  echo "error: aivcs binary not found or not executable: $AIVCS" >&2
  echo "       build with: cargo build --release -p aivcs-cli" >&2
  exit 3
fi

WORK="$(mktemp -d -t aivcs-bench.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

# Each scenario gets its own subdir. Hyperfine commands run in WORK by default,
# so we cd into the scenario dir via the shell.
mkdir -p "$WORK/cold" "$WORK/warm" "$WORK/note"

# All scenarios assume a real git repo under their cwd so `git rev-parse HEAD`
# succeeds — this is the realistic case and it exercises the subprocess spawns
# the code review flagged.
seed_git_repo() {
  local dir="$1"
  (
    cd "$dir"
    git init --quiet
    git config user.email "bench@aivcs.invalid"
    git config user.name  "bench"
    echo "seed" > seed.txt
    git add seed.txt
    git commit --quiet -m "seed"
  )
}

seed_git_repo "$WORK/cold"
seed_git_repo "$WORK/warm"
seed_git_repo "$WORK/note"

# Pre-seed warm/note with one init+snapshot so the bench measures only the
# repeated path, not the one-shot setup.
(
  cd "$WORK/warm"
  echo '{"step":0}' > state.json
  "$AIVCS" init . >/dev/null
  "$AIVCS" snapshot --state state.json --message seed --author bench --branch main >/dev/null
)
(
  cd "$WORK/note"
  echo '{"step":0}' > state.json
  "$AIVCS" init . >/dev/null
  "$AIVCS" snapshot --state state.json --message seed --author bench --branch main >/dev/null
)

OUT_DIR=""
if [[ -n "${BENCH_OUT:-}" ]]; then
  mkdir -p "$BENCH_OUT"
  OUT_DIR="$BENCH_OUT"
fi

export_args() {
  local name="$1"
  if [[ -n "$OUT_DIR" ]]; then
    echo "--export-markdown $OUT_DIR/$name.md"
  fi
}

echo "=== aivcs-cli bench ==="
echo "binary : $AIVCS"
echo "workdir: $WORK"
echo "warmup : $WARMUP runs / measured: $RUNS"
echo

# --- 1. cold init+snapshot ---------------------------------------------------
# --prepare wipes the .aivcs dir between runs so each iteration is truly cold.
echo "--- scenario 1: cold init+snapshot ---"
# shellcheck disable=SC2046
hyperfine \
  --warmup "$WARMUP" \
  --runs "$RUNS" \
  --command-name "cold init+snapshot" \
  --prepare "rm -rf '$WORK/cold/.aivcs'" \
  $(export_args cold-init-snapshot) \
  "cd '$WORK/cold' && '$AIVCS' init . >/dev/null && echo '{\"step\":1}' > state.json && '$AIVCS' snapshot --state state.json --message bench --author bench --branch main >/dev/null"

# --- 2. warm snapshot --------------------------------------------------------
# No --prepare: each iteration writes a new commit into the existing DB. The
# CAS dir grows linearly with RUNS+WARMUP — at the default 13 runs that's
# ~13 small JSON files. Acceptable for a wall-clock bench; if RUNS climbs into
# the hundreds, wipe the dir manually between bench invocations.
echo
echo "--- scenario 2: warm snapshot ---"
# shellcheck disable=SC2046
hyperfine \
  --warmup "$WARMUP" \
  --runs "$RUNS" \
  --command-name "warm snapshot" \
  $(export_args warm-snapshot) \
  "cd '$WORK/warm' && '$AIVCS' snapshot --state state.json --message bench --author bench --branch main >/dev/null"

# --- 3. pr-note --------------------------------------------------------------
# Conditional: pr-note lands with issue #220 Phase 0 (PR #223). Skip cleanly
# on binaries that predate it so the harness can ship before #223 merges.
echo
if "$AIVCS" pr-note --help >/dev/null 2>&1; then
  echo "--- scenario 3: pr-note ---"
  # shellcheck disable=SC2046
  hyperfine \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --command-name "pr-note" \
    $(export_args pr-note) \
    "cd '$WORK/note' && '$AIVCS' pr-note --branch main >/dev/null"
else
  echo "--- scenario 3: pr-note --- (skipped: subcommand not in this build)"
fi

echo
echo "done."
if [[ -n "$OUT_DIR" ]]; then
  echo "markdown summaries: $OUT_DIR/"
fi
