#!/bin/bash
# BonBo Auto Scanner — runs every 4 hours
# Usage: ./auto_scan.sh
# Or add to crontab: 0 */4 * * * /path/to/auto_scan.sh

set -e
cd "$(dirname "$0")/.."

REPORTS_DIR="reports"
mkdir -p "$REPORTS_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M)
REPORT_FILE="$REPORTS_DIR/scan_${TIMESTAMP}"
LOG_FILE="$REPORTS_DIR/auto_scan.log"

echo "[$(date)] Starting auto-scan..." >> "$LOG_FILE"

# Build and run scanner
echo "[$(date)] Building..." >> "$LOG_FILE"
cargo run --release --example best_trade > "$REPORT_FILE.txt" 2>> "$LOG_FILE"

# Also run Python alert scanner
if [ -f "scripts/scan_alert.py" ]; then
    python3 scripts/scan_alert.py >> "$LOG_FILE" 2>&1
fi

# Rotate: keep last 24 scans
cd "$REPORTS_DIR"
ls -t scan_*.txt 2>/dev/null | tail -n +25 | xargs rm -f 2>/dev/null
ls -t scan_*.json 2>/dev/null | tail -n +25 | xargs rm -f 2>/dev/null

echo "[$(date)] Auto-scan complete. Report: $REPORT_FILE.txt" >> "$LOG_FILE"
