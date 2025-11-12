SHELL := /bin/sh

.PHONY: quality-checks fmt clippy test markdownlint specify-checks specify-tasks specify-tests specify-compile specify-commits
.PHONY: openai-tests test-hooks
.PHONY: build-macos-x86_64 build-macos-aarch64 build-macos-all

TASKS ?= $(shell find specs -name tasks.md)

fmt:
	cargo fmt --check

clippy:
	cargo clippy -- -D warnings

test:
	cargo test

markdownlint:
	npx markdownlint-cli '**/*.md' --ignore node_modules --ignore .git

specify-tasks:
	@for file in $(TASKS); do \
		echo "🔍 Checking tasks in $$file"; \
		bash .specify/scripts/checks/check-tasks.sh $$file; \
	done

specify-tests:
	bash .specify/scripts/checks/check-tests.sh

specify-compile:
	bash .specify/scripts/checks/check-compile.sh

specify-commits:
	bash .specify/scripts/checks/check-commits.sh --from origin/main --to HEAD

specify-checks: specify-tasks specify-tests specify-compile specify-commits

quality-checks: fmt clippy test specify-checks markdownlint openai-tests test-hooks

openai-tests:
	cargo test -p ollama-coordinator-coordinator --test openai_proxy

test-hooks:
	npx bats tests/hooks/test-block-git-branch-ops.bats tests/hooks/test-block-cd-command.bats

# macOS cross-compilation targets
build-macos-x86_64:
	@echo "Building for macOS x86_64 (Intel)..."
	cargo build --release --target x86_64-apple-darwin \
		-p ollama-coordinator-coordinator \
		-p ollama-coordinator-agent

build-macos-aarch64:
	@echo "Building for macOS aarch64 (Apple Silicon)..."
	cargo build --release --target aarch64-apple-darwin \
		-p ollama-coordinator-coordinator \
		-p ollama-coordinator-agent

build-macos-all: build-macos-x86_64 build-macos-aarch64
	@echo "All macOS builds completed successfully!"
