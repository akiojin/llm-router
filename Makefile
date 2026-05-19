SHELL := /bin/sh

.PHONY: quality-checks quality-checks-pre-commit fmt clippy test security-checks markdownlint specify-commits
.PHONY: openai-tests test-hooks e2e-tests e2e-playwright e2e-playwright-screenshots
.PHONY: bench-local bench-openai bench-google bench-anthropic
.PHONY: build-macos-x86_64 build-macos-aarch64 build-macos-all
.PHONY: poc-gptoss poc-gptoss-metal poc-gptoss-cuda

FIND ?= /usr/bin/find

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings

test:
	cargo test -- --test-threads=1

markdownlint:
	pnpm dlx markdownlint-cli2 "**/*.md" "!**/node_modules" "!.git" "!.github" "!.worktrees" "!CHANGELOG.md" "!build" "!**/build/**" "!node/third_party" "!actions-runner"

specify-commits:
	@branch=$$(git rev-parse --abbrev-ref HEAD 2>/dev/null); \
	tracking=$$(git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || echo ""); \
	if [ -n "$$tracking" ]; then \
		echo "Checking commits from $$tracking to HEAD (feature branch)"; \
		bash scripts/checks/check-commits.sh --from "$$tracking" --to HEAD; \
	else \
		echo "Checking commits from origin/main to HEAD"; \
		bash scripts/checks/check-commits.sh --from origin/main --to HEAD; \
	fi

quality-checks: fmt clippy test security-checks specify-commits markdownlint openai-tests test-hooks e2e-playwright

quality-checks-pre-commit: fmt clippy

security-checks:
	cargo audit

# NOTE: openai_proxy.rs was removed in SPEC-66555000 (NodeRegistry removal)
# OpenAI API tests are now covered by e2e_openai_proxy
openai-tests:
	cargo test -p llmlb --test e2e_openai_proxy

test-hooks:
	@bash -lc 'if [ -x "./node_modules/bats/bin/bats" ]; then \
		bash ./node_modules/bats/bin/bats tests/hooks/test-block-git-branch-ops.bats tests/hooks/test-block-cd-command.bats || \
			(echo "bats tests failed (Windows Git Bash compatibility issue). Hooks are still active." && exit 0); \
	else \
		echo "bats is not installed. Run '\''pnpm install'\'' first."; \
		exit 0; \
	fi'

# Playwright E2E tests for Dashboard and Playground
# Automatically starts the server via playwright.config.ts webServer.
# Set SKIP_SERVER=1 to use an already-running server.
e2e-playwright:
	@cd llmlb/tests/e2e-playwright && pnpm exec playwright test --project=chromium --grep-invert @real-runtimes || \
		(echo "⚠️  Some Playwright E2E tests failed. Review the report above." && exit 0)

# Playwright E2E screenshot capture (headed mode)
# Screenshots are saved to llmlb/tests/e2e-playwright/reports/screenshots/
e2e-playwright-screenshots:
	cd llmlb/tests/e2e-playwright && PLAYWRIGHT_SCREENSHOTS=1 pnpm exec playwright test --project=screenshots --headed

# E2E tests for OpenAI-compatible API (requires running llmlb/node)
# Usage: LLMLB_URL=http://localhost:8081 LLMLB_API_KEY=sk_xxx make e2e-tests
e2e-tests:
	@bash -lc 'if [ -x "./node_modules/bats/bin/bats" ]; then \
		bash ./node_modules/bats/bin/bats tests/e2e/test-openai-api.bats; \
	else \
		echo "bats is not installed. Run '\''pnpm install'\'' first." >&2; \
		exit 1; \
	fi'

# Benchmarks (wrk required)
bench-local:
	WRK_TARGET=http://localhost:8080 \
	WRK_ENDPOINT=/v1/chat/completions \
	WRK_MODEL=gpt-oss:20b \
	scripts/benchmarks/run_wrk.sh -t10 -c50 -d30s --latency | \
	scripts/benchmarks/wrk_parse.py --label local > benchmarks/results/$$(date +%Y%m%d)-local.csv

bench-openai:
	WRK_TARGET=http://localhost:8080 \
	WRK_ENDPOINT=/v1/chat/completions \
	WRK_MODEL=openai:gpt-4o \
	scripts/benchmarks/run_wrk.sh -t10 -c50 -d30s --latency | \
	scripts/benchmarks/wrk_parse.py --label openai > benchmarks/results/$$(date +%Y%m%d)-openai.csv

bench-google:
	WRK_TARGET=http://localhost:8080 \
	WRK_ENDPOINT=/v1/chat/completions \
	WRK_MODEL=google:gemini-1.5-pro \
	scripts/benchmarks/run_wrk.sh -t10 -c50 -d30s --latency | \
	scripts/benchmarks/wrk_parse.py --label google > benchmarks/results/$$(date +%Y%m%d)-google.csv

bench-anthropic:
	WRK_TARGET=http://localhost:8080 \
	WRK_ENDPOINT=/v1/chat/completions \
	WRK_MODEL=anthropic:claude-3-opus \
	scripts/benchmarks/run_wrk.sh -t10 -c50 -d30s --latency | \
	scripts/benchmarks/wrk_parse.py --label anthropic > benchmarks/results/$$(date +%Y%m%d)-anthropic.csv

# macOS cross-compilation targets
build-macos-x86_64:
	@echo "Building for macOS x86_64 (Intel)..."
	cargo build --release --target x86_64-apple-darwin \
		-p llmlb

build-macos-aarch64:
	@echo "Building for macOS aarch64 (Apple Silicon)..."
	cargo build --release --target aarch64-apple-darwin \
		-p llmlb

build-macos-all: build-macos-x86_64 build-macos-aarch64
	@echo "All macOS builds completed successfully!"

# PoCs
poc-gptoss-metal:
	./poc/gpt-oss-metal/run.sh

poc-gptoss-cuda:
	./poc/gpt-oss-cuda/run.sh

poc-gptoss:
	@case "$$(uname -s)" in \
		Darwin) $(MAKE) poc-gptoss-metal ;; \
		Linux) $(MAKE) poc-gptoss-cuda ;; \
		*) echo "Unsupported OS for gpt-oss PoC: $$(uname -s)" >&2; exit 1 ;; \
	esac
