.PHONY: build build-cli build-docs test clean deploy deploy-docs serve help

# CLI (Rust)
build: build-cli

build-cli:
	@echo "Building CLI..."
	cargo build --release

test:
	@echo "Running tests..."
	cargo test

# Docs (MkDocs)
build-docs:
	@echo "Building documentation..."
	uv run mkdocs build

serve:
	@echo "Starting local docs server..."
	uv run mkdocs serve

deploy-docs:
	@echo "Building and deploying docs to tenement.dev..."
	@uv run mkdocs build
	@wrangler pages deploy site --project-name tenement --commit-dirty=true
	@echo "✓ Deployed to https://tenement.dev"

# Combined
deploy: build-cli deploy-docs
	@echo "✓ All deployed"

clean:
	@echo "Cleaning build artifacts..."
	@rm -rf site/ target/
	@echo "Clean complete!"

help:
	@echo "Available targets:"
	@echo ""
	@echo "  CLI (Rust):"
	@echo "    make build      - Build CLI binary"
	@echo "    make test       - Run tests"
	@echo ""
	@echo "  Docs (MkDocs):"
	@echo "    make build-docs - Build documentation"
	@echo "    make serve      - Start local docs server"
	@echo "    make deploy-docs- Deploy docs to tenement.dev"
	@echo ""
	@echo "  Combined:"
	@echo "    make deploy     - Build CLI and deploy docs"
	@echo "    make clean      - Remove all build artifacts"

.DEFAULT_GOAL := help
