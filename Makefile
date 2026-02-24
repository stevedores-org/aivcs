.PHONY: lci-build lci lci-install hooks

# Build the local CI runner
lci-build:
	cd tools/lci && go build -o lci .

# Build and run default stages
lci: lci-build
	./tools/lci/lci

# Install lci to ~/.local/bin as local-ci (backwards compat)
lci-install: lci-build
	mkdir -p $(HOME)/.local/bin
	cp tools/lci/lci $(HOME)/.local/bin/local-ci
	@echo "Installed local-ci to ~/.local/bin/local-ci"

# Install git pre-commit hook
hooks:
	@echo '#!/bin/bash' > .git/hooks/pre-commit
	@echo 'set -e' >> .git/hooks/pre-commit
	@echo '' >> .git/hooks/pre-commit
	@echo '# Find lci: prefer local build, then PATH' >> .git/hooks/pre-commit
	@echo 'LCI="$$(git rev-parse --show-toplevel)/tools/lci/lci"' >> .git/hooks/pre-commit
	@echo 'if [ ! -x "$$LCI" ]; then' >> .git/hooks/pre-commit
	@echo '  LCI="$$(command -v local-ci 2>/dev/null || command -v lci 2>/dev/null || true)"' >> .git/hooks/pre-commit
	@echo 'fi' >> .git/hooks/pre-commit
	@echo 'if [ -z "$$LCI" ]; then' >> .git/hooks/pre-commit
	@echo '  echo "⚠️  lci not found. Run: make lci-build"' >> .git/hooks/pre-commit
	@echo '  exit 0' >> .git/hooks/pre-commit
	@echo 'fi' >> .git/hooks/pre-commit
	@echo '' >> .git/hooks/pre-commit
	@echo '"$$LCI" fmt clippy || { echo ""; echo "❌ Pre-commit checks failed."; echo "💡 Tip: Run ./tools/lci/lci --fix fmt"; exit 1; }' >> .git/hooks/pre-commit
	@echo 'echo "✅ Pre-commit checks passed"' >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Installed pre-commit hook"
