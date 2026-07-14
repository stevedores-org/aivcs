.PHONY: ci hooks bench bench-build

# Run the og-crab CI assessment (config: propel.toml)
ci:
	og-crab run

# Build the release binary the bench harness drives. Separate target so CI can
# cache the build and only re-bench on rebuild.
bench-build:
	cargo build --release -p aivcs-cli

# Wall-clock bench of aivcs-cli hot paths via hyperfine.
# See tools/bench/aivcs-cli.sh for env overrides (AIVCS, BENCH_OUT, BENCH_RUNS).
bench: bench-build
	./tools/bench/aivcs-cli.sh

# Install git pre-commit hook
hooks:
	@echo '#!/bin/bash' > .git/hooks/pre-commit
	@echo 'set -e' >> .git/hooks/pre-commit
	@echo '' >> .git/hooks/pre-commit
	@echo 'if ! command -v og-crab >/dev/null 2>&1; then' >> .git/hooks/pre-commit
	@echo '  echo "⚠️  og-crab not found. Install: cargo install --git https://github.com/lornu-ai/og-crab og-crab"' >> .git/hooks/pre-commit
	@echo '  exit 0' >> .git/hooks/pre-commit
	@echo 'fi' >> .git/hooks/pre-commit
	@echo '' >> .git/hooks/pre-commit
	@echo 'og-crab run fmt clippy || { echo ""; echo "❌ Pre-commit checks failed."; echo "💡 Tip: run cargo fmt --all, then re-run og-crab run"; exit 1; }' >> .git/hooks/pre-commit
	@echo 'echo "✅ Pre-commit checks passed"' >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Installed pre-commit hook"
