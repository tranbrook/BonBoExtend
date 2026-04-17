# BonBo Extend — Makefile

.PHONY: build release test check clean clippy fmt upgrade

# Build all
default: build

build:
	cargo build

release:
	cargo build --release

test:
	cargo test --workspace

check:
	cargo check --workspace

clippy:
	cargo clippy --workspace -- -W clippy::all

fmt:
	cargo fmt --all --check

clean:
	cargo clean

# Upgrade BonBo Core + rebuild
upgrade:
	@bash scripts/upgrade-core.sh

# Install binaries
install: release
	cp target/release/bonbo-extend-mcp /usr/local/bin/
	@echo "✅ Installed bonbo-extend-mcp to /usr/local/bin/"

# Run MCP server (for testing)
run-mcp:
	cargo run -p bonbo-extend-mcp

# Run tests with output
test-verbose:
	cargo test --workspace -- --nocapture
