#!/bin/bash
# BonBo Agent — Live Trading Launcher
#
# Usage:
#   ./scripts/run_agent.sh           # Run with config/trading.toml
#   ./scripts/run_agent.sh dry_run   # Force dry-run mode
#   ./scripts/run_agent.sh testnet   # Force testnet mode
#   ./scripts/run_agent.sh live      # Force live mode (⚠️ real money!)

set -euo pipefail

# Navigate to project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# Load .env if exists
if [ -f .env ]; then
    echo "📋 Loading .env..."
    set -a
    source .env
    set +a
fi

# Override mode from argument
if [ -n "${1:-}" ]; then
    export BONBO_MODE="$1"
    echo "🔧 Mode override: $1"
fi

# Build release
echo "🔨 Building bonbo-agent..."
cargo build -p bonbo-agent --release 2>/dev/null

# Run
echo "🚀 Starting bonbo-agent..."
echo "────────────────────────────────────"
exec ./target/release/bonbo-agent
