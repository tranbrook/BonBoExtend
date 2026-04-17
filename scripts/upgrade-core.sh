#!/bin/bash
# BonBo Extend — Upgrade Script
#
# Usage:
#   ./scripts/upgrade-core.sh          # Upgrade BonBo core
#   ./scripts/upgrade-core.sh --build  # Upgrade + rebuild

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

BONBO_CORE_DIR="${BONBO_CORE_DIR:-$HOME/bonbo/bonbo-rust}"
BONBO_EXTEND_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo -e "${CYAN}═══════════════════════════════════════════════${NC}"
echo -e "${CYAN}  BonBo Extend — Core Upgrade Script${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════${NC}"
echo ""

# Step 1: Check BonBo core
echo -e "${YELLOW}[1/4]${NC} Checking BonBo core at ${BONBO_CORE_DIR}..."
if [ ! -d "$BONBO_CORE_DIR/.git" ]; then
    echo -e "${RED}Error: ${BONBO_CORE_DIR} is not a git repository${NC}"
    exit 1
fi

current_version=$(cd "$BONBO_CORE_DIR" && git describe --tags --always 2>/dev/null || echo "unknown")
echo -e "  Current version: ${GREEN}${current_version}${NC}"

# Step 2: Pull latest
echo -e "${YELLOW}[2/4]${NC} Pulling latest changes..."
cd "$BONBO_CORE_DIR"
git fetch origin main 2>/dev/null || git fetch origin
git pull origin main
echo -e "  ${GREEN}✅ Updated${NC}"

new_version=$(git describe --tags --always 2>/dev/null || echo "unknown")
echo -e "  New version: ${GREEN}${new_version}${NC}"

# Step 3: Rebuild BonBo core
echo -e "${YELLOW}[3/4]${NC} Building BonBo core..."
cargo build --release 2>&1 | tail -5
echo -e "  ${GREEN}✅ Build complete${NC}"

# Step 4: Rebuild BonBo Extend
echo -e "${YELLOW}[4/4]${NC} Building BonBo Extend..."
cd "$BONBO_EXTEND_DIR"
cargo build --release 2>&1 | tail -5
echo -e "  ${GREEN}✅ Build complete${NC}"

# Summary
echo ""
echo -e "${CYAN}═══════════════════════════════════════════════${NC}"
echo -e "${GREEN}✅ Upgrade complete!${NC}"
echo -e "  BonBo Core: ${current_version} → ${new_version}"
echo -e "  BonBo Extend: rebuilt successfully"
echo -e "${CYAN}═══════════════════════════════════════════════${NC}"

# Optional: install binaries
if [ "${1:-}" = "--install" ]; then
    echo -e "${YELLOW}Installing binaries...${NC}"
    cp "$BONBO_CORE_DIR/target/release/bonbo" /usr/local/bin/bonbo
    cp "$BONBO_EXTEND_DIR/target/release/bonbo-extend-mcp" /usr/local/bin/bonbo-extend-mcp
    echo -e "${GREEN}✅ Installed to /usr/local/bin/${NC}"
fi
