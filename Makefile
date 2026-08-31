.DEFAULT_GOAL := help
.PHONY: help build release test lint format check run install clean demo adr

BIN := ./target/debug/dotbanner
ARGS ?= render "dotbanner" --style band --colors omarchy

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: ## Build the debug binary
	cargo build

release: ## Build the optimized binary
	cargo build --release

test: ## Run the test suite
	cargo test

lint: ## Clippy (warnings are errors) and format check
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

format: ## Auto-format the workspace
	cargo fmt

check: lint test ## Run all quality gates

run: build ## Run the CLI (usage: make run ARGS='render "hi" --style trap')
	@$(BIN) $(ARGS)

demo: build ## Render every style, then every gradient
	@$(BIN) show styles
	@$(BIN) show gradients

install: ## Install dotbanner to ~/.cargo/bin
	cargo install --path crates/dotbanner

clean: ## Remove build artifacts
	cargo clean

adr: ## ADR management (usage: make adr CMD="list --group")
	@docs/scripts/adr $(CMD)
