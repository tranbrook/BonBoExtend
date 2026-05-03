#!/usr/bin/env python3
"""
BonBo Position Monitor — checks open positions for trend reversal,
overbought/oversold, and liquidation proximity.

Uses MCP client to fetch positions and candle data.

Usage:
    python3 scripts/position_monitor.py
"""

import sys
import os
import json
import re
from datetime import datetime

# Ensure scripts/ is on path for mcp_client import
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__))))
from mcp_client import MCPClient, safe_float


REPORTS_DIR = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "reports"
)


def compute_ema(closes: list[float], period: int) -> list[float]:
    """Compute EMA over a list of closing prices."""
    if not closes or len(closes) < period:
        return []
    multiplier = 2.0 / (period + 1)
    ema = [sum(closes[:period]) / period]
    for price in closes[period:]:
        ema.append((price - ema[-1]) * multiplier + ema[-1])
    return ema


def compute_rsi(closes: list[float], period: int = 14) -> list[float]:
    """Compute RSI from closing prices."""
    if len(closes) < period + 1:
        return []
    gains, losses = [], []
    for i in range(1, period + 1):
        delta = closes[i] - closes[i - 1]
        gains.append(max(delta, 0))
        losses.append(max(-delta, 0))
    avg_gain = sum(gains) / period
    avg_loss = sum(losses) / period
    rsi_values = []
    if avg_loss == 0:
        rsi_values.append(100.0)
    else:
        rs = avg_gain / avg_loss
        rsi_values.append(100.0 - 100.0 / (1.0 + rs))
    for i in range(period + 1, len(closes)):
        delta = closes[i] - closes[i - 1]
        gain = max(delta, 0)
        loss = max(-delta, 0)
        avg_gain = (avg_gain * (period - 1) + gain) / period
        avg_loss = (avg_loss * (period - 1) + loss) / period
        if avg_loss == 0:
            rsi_values.append(100.0)
        else:
            rs = avg_gain / avg_loss
            rsi_values.append(100.0 - 100.0 / (1.0 + rs))
    return rsi_values


def parse_candles(candle_text: str) -> list[dict]:
    """Parse candle data from MCP technical_analysis output."""
    candles = []
    lines = candle_text.strip().split("\n")
    for line in lines:
        # Try to extract OHLCV from various formats
        # Format: timestamp o h l c vol  or  date | O | H | L | C | Vol
        parts = re.split(r"[|\t,]+", line.strip())
        if len(parts) < 5:
            continue
        try:
            # Try to find numeric values — skip the date/timestamp
            nums = []
            for p in parts:
                p = p.strip()
                try:
                    nums.append(float(p))
                except ValueError:
                    continue
            if len(nums) >= 4:
                # Assume: open, high, low, close [, volume]
                candles.append({
                    "open": nums[0],
                    "high": nums[1],
                    "low": nums[2],
                    "close": nums[3],
                    "volume": nums[4] if len(nums) > 4 else 0,
                })
        except Exception:
            continue
    return candles


def determine_trend(ema12: list[float], ema26: list[float]) -> str:
    """Determine trend from EMA12/EMA26 relationship. Returns LONG/SHORT/NEUTRAL."""
    if not ema12 or not ema26:
        return "NEUTRAL"
    # Use the last few values to confirm trend direction
    lookback = min(3, len(ema12), len(ema26))
    e12_last = ema12[-lookback:]
    e26_last = ema26[-lookback:]

    # Count how many bars EMA12 > EMA26
    bullish = sum(1 for a, b in zip(e12_last, e26_last) if a > b)
    bearish = sum(1 for a, b in zip(e12_last, e26_last) if a < b)

    if bullish == lookback:
        return "LONG"
    elif bearish == lookback:
        return "SHORT"
    else:
        return "NEUTRAL"


def run_monitor():
    """Main position monitor logic."""
    os.makedirs(REPORTS_DIR, exist_ok=True)

    try:
        client = MCPClient()
    except FileNotFoundError as e:
        print(f"❌ {e}")
        return

    timestamp = datetime.now().strftime("%Y%m%d_%H%M")
    alerts = []
    position_rows = []

    # 1. Fetch current positions
    print("📊 Fetching open positions...")
    pos_text = client.call("get_positions")
    if not pos_text:
        print("⚠️  No position data returned. Are there open positions?")
        return

    # Parse positions — look for symbol and side
    # Expected format varies; try JSON first
    positions = []
    try:
        positions = json.loads(pos_text)
        if not isinstance(positions, list):
            positions = [positions]
    except json.JSONDecodeError:
        # Parse text format — each line may have symbol, side, entry, liq_price
        for line in pos_text.strip().split("\n"):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            # Try to extract key fields
            pos = {}
            # Look for common patterns
            sym_match = re.search(r"symbol[:\s]+(\S+)", line, re.I)
            side_match = re.search(r"side[:\s]+(long|short|buy|sell)", line, re.I)
            entry_match = re.search(r"entry(?:_price)?[:\s]+([\d.]+)", line, re.I)
            liq_match = re.search(r"liq(?:uidation)?(?:_price)?[:\s]+([\d.]+)", line, re.I)
            size_match = re.search(r"(?:size|qty|amount)[:\s]+([\d.]+)", line, re.I)
            if sym_match:
                pos["symbol"] = sym_match.group(1).upper()
                pos["side"] = side_match.group(1).upper() if side_match else "UNKNOWN"
                pos["entry"] = safe_float(entry_match.group(1)) if entry_match else 0
                pos["liq_price"] = safe_float(liq_match.group(1)) if liq_match else 0
                pos["size"] = safe_float(size_match.group(1)) if size_match else 0
                positions.append(pos)

    if not positions:
        print("ℹ️  No open positions detected.")
        return

    print(f"   Found {len(positions)} position(s). Analyzing...")

    for pos in positions:
        symbol = pos.get("symbol", "UNKNOWN")
        side = str(pos.get("side", "UNKNOWN")).upper()
        entry = safe_float(pos.get("entry", 0))
        liq_price = safe_float(pos.get("liq_price", 0))
        size = safe_float(pos.get("size", 0))

        # Normalize side
        if side in ("BUY", "LONG"):
            side = "LONG"
        elif side in ("SELL", "SHORT"):
            side = "SHORT"

        # 2. Fetch 4H candles for this symbol
        candle_text = client.call("technical_analysis", {
            "symbol": symbol,
            "timeframe": "4h",
            "limit": 60,
        })

        if not candle_text:
            alerts.append(f"⚠️  {symbol}: Could not fetch 4H candles")
            position_rows.append({
                "symbol": symbol, "side": side, "entry": entry,
                "current": 0, "trend_4h": "N/A", "rsi_4h": "N/A",
                "status": "NO DATA", "alerts": ["No candle data"],
            })
            continue

        # 3. Parse candles and compute indicators
        candles = parse_candles(candle_text)
        if len(candles) < 30:
            alerts.append(f"⚠️  {symbol}: Insufficient candle data ({len(candles)} bars)")
            continue

        closes = [c["close"] for c in candles]
        current_price = closes[-1]

        ema12 = compute_ema(closes, 12)
        ema26 = compute_ema(closes, 26)
        rsi_values = compute_rsi(closes, 14)

        trend_4h = determine_trend(ema12, ema26)
        rsi_4h = rsi_values[-1] if rsi_values else 50.0

        pos_alerts = []

        # 4. Check trend reversal
        if side == "LONG" and trend_4h == "SHORT":
            alert = f"⚠️  WARNING: {symbol} LONG but 4H trend reversed to SHORT"
            alerts.append(alert)
            pos_alerts.append("TREND REVERSAL")
        elif side == "SHORT" and trend_4h == "LONG":
            alert = f"⚠️  WARNING: {symbol} SHORT but 4H trend reversed to LONG"
            alerts.append(alert)
            pos_alerts.append("TREND REVERSAL")

        # 5. Check RSI overbought/oversold
        if side == "LONG" and rsi_4h > 70:
            alert = f"🔺 OVERBOUGHT: {symbol} LONG with RSI(4H) = {rsi_4h:.1f}"
            alerts.append(alert)
            pos_alerts.append(f"RSI {rsi_4h:.1f}")
        elif side == "SHORT" and rsi_4h < 30:
            alert = f"🔻 OVERSOLD: {symbol} SHORT with RSI(4H) = {rsi_4h:.1f}"
            alerts.append(alert)
            pos_alerts.append(f"RSI {rsi_4h:.1f}")

        # 6. Check liquidation proximity
        status = "OK"
        if liq_price > 0 and current_price > 0:
            distance_pct = abs(current_price - liq_price) / current_price * 100
            if distance_pct < 10:
                alert = f"🚨 CRITICAL: {symbol} liquidation < 10% away! " \
                        f"(price={current_price:.4f}, liq={liq_price:.4f}, " \
                        f"dist={distance_pct:.1f}%)"
                alerts.append(alert)
                pos_alerts.append(f"LIQ {distance_pct:.1f}%")
                status = "CRITICAL"
            elif distance_pct < 20:
                alert = f"⚠️  CAUTION: {symbol} liquidation < 20% away ({distance_pct:.1f}%)"
                alerts.append(alert)
                pos_alerts.append(f"LIQ {distance_pct:.1f}%")
                status = "CAUTION"

        if not pos_alerts and trend_4h == side:
            status = "✅ OK"
        elif not pos_alerts:
            status = "WATCH"

        position_rows.append({
            "symbol": symbol, "side": side, "entry": entry,
            "current": current_price, "trend_4h": trend_4h,
            "rsi_4h": f"{rsi_4h:.1f}", "status": status,
            "alerts": pos_alerts if pos_alerts else ["—"],
            "liq_price": liq_price,
        })

    # 7. Print summary table
    print("\n" + "=" * 90)
    print("  POSITION MONITOR SUMMARY")
    print("=" * 90)
    header = f"{'Symbol':<12} {'Side':<7} {'Entry':>10} {'Current':>10} {'4H Trend':<10} {'RSI(4H)':>8} {'Status':<10}"
    print(header)
    print("-" * 90)
    for r in position_rows:
        row_str = (
            f"{r['symbol']:<12} {r['side']:<7} "
            f"{r['entry']:>10.4f} {r['current']:>10.4f} "
            f"{r['trend_4h']:<10} {str(r['rsi_4h']):>8} {r['status']:<10}"
        )
        print(row_str)
    print("=" * 90)

    # Print alerts
    if alerts:
        print("\n🔔 ALERTS:")
        for a in alerts:
            print(f"  {a}")
    else:
        print("\n✅ No alerts — all positions look healthy.")

    # 8. Save report to markdown
    report_path = os.path.join(REPORTS_DIR, f"position_monitor_{timestamp}.md")
    with open(report_path, "w") as f:
        f.write(f"# Position Monitor Report — {timestamp}\n\n")
        f.write(f"**Generated:** {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n\n")
        f.write(f"## Summary Table\n\n")
        f.write(f"| Symbol | Side | Entry | Current | 4H Trend | RSI(4H) | Status |\n")
        f.write(f"|--------|------|-------|---------|----------|---------|--------|\n")
        for r in position_rows:
            f.write(
                f"| {r['symbol']} | {r['side']} | {r['entry']:.4f} | "
                f"{r['current']:.4f} | {r['trend_4h']} | {r['rsi_4h']} | "
                f"{r['status']} |\n"
            )
        if alerts:
            f.write(f"\n## Alerts\n\n")
            for a in alerts:
                f.write(f"- {a}\n")
        else:
            f.write(f"\n✅ No alerts — all positions healthy.\n")

    print(f"\n📄 Report saved: {report_path}")
    print(f"   MCP stats: {client.stats}")


if __name__ == "__main__":
    run_monitor()
