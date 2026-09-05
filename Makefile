# One entry point for the everyday commands, so nobody has to remember which of cargo, npm or node
# owns a given job. `make` on its own lists what is here.
#
# Everything here is a thin wrapper. The real work lives in cargo, npm and scripts/, and each recipe
# echoes the command it runs so it can be copied and used directly.

# Cargo installs itself outside the default PATH, and this is the single most common reason a command
# here fails on a fresh shell.
export PATH := $(HOME)/.cargo/bin:$(PATH)

MAKEFLAGS += --no-print-directory

.DEFAULT_GOAL := help

.PHONY: help tools node-version run dev-backend dev-frontend ui build test lint fmt check \
        version pending release wiki install-preview clean

help: ## List these targets
	@echo 'On Air Record'
	@echo ''
	@grep -hE '^[a-z][a-z0-9_-]*:.*## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN { FS = ":.*## " } { printf "  %-16s %s\n", $$1, $$2 }'
	@echo ''
	@echo '  Two terminals for development: make dev-backend, make dev-frontend'

tools: ## Check the toolchains are present and report how to fix them
	@command -v cargo >/dev/null || { \
		echo 'cargo is not on PATH. Install from https://rustup.rs, then: . "$$HOME/.cargo/env"'; \
		exit 1; }
	@command -v node >/dev/null || { echo 'node is not on PATH. Install Node 22.'; exit 1; }
	@echo "cargo $$(cargo --version | awk '{print $$2}'), node $$(node --version)"
	@major=$$(node -p 'process.versions.node.split(".")[0]'); \
	if [ "$$major" -lt 20 ]; then \
		echo 'That node is too old. Run: cd frontend && nvm use'; \
	else \
		echo 'Both are usable.'; \
	fi

# Node 16 is a common fallback on a developer machine and fails the Vite build with an unhelpful
# `styleText` export error. Say so plainly rather than letting the build explain it badly.
node-version:
	@major=$$(node -p 'process.versions.node.split(".")[0]'); \
	if [ "$$major" -lt 20 ]; then \
		echo "Node $$(node --version) is too old: the frontend build fails with an unhelpful"; \
		echo 'styleText error, and the scripts in scripts/ want Node 18 or newer.'; \
		echo 'Run: cd frontend && nvm use'; \
		exit 1; \
	fi

run: ## Run the service against the UI already in frontend/dist
	cd backend && cargo run

dev-backend: ## Terminal 1: the API and WebSocket on :8080, with debug logging
	cd backend && OAR_LOG_LEVEL=debug cargo run

dev-frontend: node-version ## Terminal 2: the Vite dev server on :5173, proxied to :8080
	cd frontend && npm run dev

ui: node-version ## Type check and build the web UI into frontend/dist
	cd frontend && npm run build

# The UI has to be built first. A release build embeds whatever is in frontend/dist at compile time,
# so the wrong order silently ships a stale interface. Encoding it here means it cannot be got wrong.
build: ui ## Build the release binary with the UI compiled into it
	cd backend && cargo build --release
	@echo ''
	@echo 'Built backend/target/release/on-air-record'

test: node-version ## Run the backend and frontend test suites
	cd backend && cargo test
	cd frontend && npm test

lint: node-version ## Clippy with warnings denied, and oxlint
	cd backend && cargo clippy --all-targets -- -D warnings
	cd frontend && npm run lint

fmt: ## Format the Rust code
	cd backend && cargo fmt

# The same things CI runs, in the same order, so a green run here means a green run there.
check: node-version ## Everything CI checks, before you push
	node scripts/version.mjs check
	cd backend && cargo fmt --all --check
	cd backend && cargo clippy --all-targets -- -D warnings
	cd backend && cargo test
	cd frontend && npm run lint
	cd frontend && npm test
	cd frontend && npm run build
	@echo ''
	@echo 'All clear.'

version: node-version ## Print the version the service reports
	@node scripts/version.mjs show

pending: node-version ## Say whether a release is due, and what it would be numbered
	@node scripts/version.mjs pending

release: node-version ## Choose the next version and move all four manifests
	@node scripts/version.mjs bump

wiki: node-version ## Build the wiki pages into /tmp/wiki-preview to see what would be published
	node scripts/build-wiki.mjs /tmp/wiki-preview

install-preview: ## Run the installer into /tmp, to try what a user gets
	mkdir -p /tmp/oar-install-preview
	cd /tmp/oar-install-preview && sh $(CURDIR)/scripts/install.sh --no-start

clean: ## Remove build output, leaving node_modules and any recordings alone
	rm -rf backend/target frontend/dist
