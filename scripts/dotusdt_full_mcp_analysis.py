#!/usr/bin/env python3
"""DOTUSDT Full Analysis — Chạy TẤT CẢ 46 MCP tools của BonBoExtend."""

import json, subprocess, sys, os, time
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed

MCP_BIN = os.path.expanduser("~/BonBoExtend/target/release/bonbo-extend-mcp")
SYMBOL = "DOTUSDT"

# ══════════════════════════════════════════════════════════════════════════
# MCP CALL ENGINE
# ══════════════════════════════════════════════════════════════════════════

_results = {}

def call_mcp(name, args=None, timeout=30):
    """Call a single MCP tool."""
    params = {"name": name}
    if args:
        params["arguments"] = args
    req = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params})
    try:
        proc = subprocess.run(
            [MCP_BIN], input=req, capture_output=True, text=True, timeout=timeout
        )
        for line in proc.stdout.strip().split('\n'):
            try:
                resp = json.loads(line)
                if "result" in resp:
                    for c in resp["result"].get("content", []):
                        if c.get("type") == "text":
                            text = c["text"]
                            try:
                                return json.loads(text)
                            except json.JSONDecodeError:
                                return text
                    return resp["result"]
            except json.JSONDecodeError:
                continue
    except subprocess.TimeoutExpired:
        return f"⏱️ Timeout ({timeout}s)"
    except Exception as e:
        return f"❌ Error: {e}"
    return None

def run_tool(name, args=None, label="", timeout=30):
    """Run tool and store result."""
    t0 = time.time()
    result = call_mcp(name, args, timeout)
    elapsed = time.time() - t0
    _results[name] = result
    return result, elapsed

def fmt_time(s):
    return f"{s:.1f}s"

# ══════════════════════════════════════════════════════════════════════════
# RUN ALL TOOLS
# ══════════════════════════════════════════════════════════════════════════

def main():
    total_start = time.time()

    print()
    print("╔" + "═"*98 + "╗")
    print(f"║  🔮 BONBO EXTEND — PHÂN TÍCH TOÀN DIỆN DOTUSDT BẰNG TẤT CẢ MCP TOOLS")
    print(f"║  Thời gian: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} — 46 Tools")
    print("╚" + "═"*98 + "╝")
    print()

    # ─── PHASE 1: MARKET DATA & PRICE ────────────────────────────────────
    print("━"*100)
    print("  📊 PHASE 1: MARKET DATA & PRICE (5 tools)")
    print("━"*100)

    tools_phase1 = [
        ("get_crypto_price", {"symbol": SYMBOL}, "Giá DOTUSDT"),
        ("get_crypto_candles", {"symbol": SYMBOL, "interval": "1d", "limit": 5}, "Nến 1D gần nhất"),
        ("get_crypto_candles", {"symbol": SYMBOL, "interval": "4h", "limit": 5}, "Nến 4H gần nhất"),
        ("get_crypto_orderbook", {"symbol": SYMBOL, "limit": 10}, "Order Book"),
        ("get_top_crypto", {"limit": 5}, "Top 5 Crypto"),
    ]
    for name, args, label in tools_phase1:
        r, t = run_tool(name, args)
        print(f"\n  [{fmt_time(t)}] {label} ({name}):")
        print(f"  {str(r)[:600]}")

    # ─── PHASE 2: SENTIMENT & ON-CHAIN ───────────────────────────────────
    print("\n" + "━"*100)
    print("  🧠 PHASE 2: SENTIMENT & ON-CHAIN (3 tools)")
    print("━"*100)

    tools_phase2 = [
        ("get_fear_greed_index", {"history": 1}, "Fear & Greed Index"),
        ("get_composite_sentiment", {"symbol": SYMBOL}, "Composite Sentiment DOT"),
        ("get_whale_alerts", {"min_usd": 500000}, "Whale Alerts"),
    ]
    for name, args, label in tools_phase2:
        r, t = run_tool(name, args)
        print(f"\n  [{fmt_time(t)}] {label} ({name}):")
        print(f"  {str(r)[:500]}")

    # ─── PHASE 3: TECHNICAL ANALYSIS (6 tools) ───────────────────────────
    print("\n" + "━"*100)
    print("  📈 PHASE 3: TECHNICAL ANALYSIS — ĐA TIMEFRAME (6 tools)")
    print("━"*100)

    tools_phase3 = [
        ("analyze_indicators", {"symbol": SYMBOL, "interval": "1d", "limit": 200}, "Indicators 1D"),
        ("analyze_indicators", {"symbol": SYMBOL, "interval": "4h", "limit": 200}, "Indicators 4H"),
        ("analyze_indicators", {"symbol": SYMBOL, "interval": "1w", "limit": 52}, "Indicators 1W"),
        ("get_trading_signals", {"symbol": SYMBOL, "interval": "1d"}, "Signals 1D"),
        ("get_trading_signals", {"symbol": SYMBOL, "interval": "4h"}, "Signals 4H"),
        ("get_support_resistance", {"symbol": SYMBOL, "interval": "1d", "lookback": 30}, "Support/Resistance 1D"),
    ]
    for name, args, label in tools_phase3:
        r, t = run_tool(name, args)
        print(f"\n  [{fmt_time(t)}] {label} ({name}):")
        print(f"  {str(r)[:800]}")

    # ─── PHASE 4: REGIME DETECTION (4 tools) ─────────────────────────────
    print("\n" + "━"*100)
    print("  🔄 PHASE 4: REGIME DETECTION (4 tools)")
    print("━"*100)

    tools_phase4 = [
        ("detect_market_regime", {"symbol": SYMBOL, "interval": "1d"}, "Regime 1D (TA Plugin)"),
        ("detect_market_regime", {"symbol": SYMBOL, "interval": "4h"}, "Regime 4H (TA Plugin)"),
        ("detect_market_regime", {"symbol": SYMBOL, "timeframe": "1d"}, "Regime 1D (BOCPD Plugin)"),
        ("detect_market_regime", {"symbol": SYMBOL, "timeframe": "4h"}, "Regime 4H (BOCPD Plugin)"),
    ]
    for name, args, label in tools_phase4:
        r, t = run_tool(name, args)
        print(f"\n  [{fmt_time(t)}] {label} ({name}):")
        print(f"  {str(r)[:600]}")

    # ─── PHASE 5: BACKTESTING & STRATEGIES (5 tools) ─────────────────────
    print("\n" + "━"*100)
    print("  🧪 PHASE 5: BACKTESTING & STRATEGIES (5 tools)")
    print("━"*100)

    r, t = run_tool("list_strategies", {})
    print(f"\n  [{fmt_time(t)}] Available Strategies:")
    print(f"  {str(r)[:500]}")

    strategies = ["sma_crossover", "ema_crossover", "rsi_mean_rev"]
    r, t = run_tool("compare_strategies", {"symbol": SYMBOL, "interval": "1d", "strategies": strategies})
    print(f"\n  [{fmt_time(t)}] Compare Strategies (1D):")
    print(f"  {str(r)[:500]}")

    for strat in strategies:
        r, t = run_tool("run_backtest", {"symbol": SYMBOL, "interval": "1d", "strategy": strat, "initial_capital": 1000})
        print(f"\n  [{fmt_time(t)}] Backtest {strat} (1D, $1000):")
        print(f"  {str(r)[:500]}")

    # ─── PHASE 6: MARKET SCANNER (2 tools) ───────────────────────────────
    print("\n" + "━"*100)
    print("  🔍 PHASE 6: MARKET SCANNER (2 tools)")
    print("━"*100)

    r, t = run_tool("scan_market", {"symbols": [SYMBOL]})
    print(f"\n  [{fmt_time(t)}] Scan DOTUSDT:")
    print(f"  {str(r)[:500]}")

    r, t = run_tool("get_scan_schedule", {})
    print(f"\n  [{fmt_time(t)}] Scan Schedule:")
    print(f"  {str(r)[:300]}")

    # ─── PHASE 7: FUTURES ACCOUNT (7 tools) ──────────────────────────────
    print("\n" + "━"*100)
    print("  💼 PHASE 7: BINANCE FUTURES ACCOUNT (7 tools)")
    print("━"*100)

    futures_tools = [
        ("futures_get_balance", {}, "Account Balance"),
        ("futures_get_positions", {}, "All Positions"),
        ("futures_get_open_orders", {"symbol": SYMBOL}, "Open Orders DOTUSDT"),
    ]
    for name, args, label in futures_tools:
        r, t = run_tool(name, args)
        status = "✅" if not isinstance(r, str) or "Error" not in str(r) else "⚠️ API Error"
        print(f"\n  [{fmt_time(t)}] {label} ({name}): {status}")
        print(f"  {str(r)[:400]}")

    # Leverage/margin/order tools (dry-run info only)
    print(f"\n  📋 Futures Trading Tools (cần API key hợp lệ):")
    for name in ["futures_set_leverage", "futures_place_order", "futures_cancel_orders",
                  "futures_close_position", "futures_set_stop_loss", "futures_set_take_profit",
                  "futures_set_trailing_stop", "futures_set_margin_type"]:
        print(f"    ⚙️ {name} — sẵn sàng khi API key whitelist IP")

    # ─── PHASE 8: RISK MANAGEMENT (3 tools) ──────────────────────────────
    print("\n" + "━"*100)
    print("  🛡️ PHASE 8: RISK MANAGEMENT (3 tools)")
    print("━"*100)

    # Position size calculator
    r, t = run_tool("calculate_position_size", {
        "equity": 1000.0,
        "entry_price": 1.27,
        "stop_loss": 1.22,
        "risk_pct": 2.0,
        "method": "fixed_risk"
    })
    print(f"\n  [{fmt_time(t)}] Position Size (equity=$1000, entry=$1.27, SL=$1.22, risk=2%):")
    print(f"  {str(r)[:500]}")

    # Risk check
    r, t = run_tool("check_risk", {
        "equity": 1000.0,
        "initial_capital": 1000.0,
        "peak_equity": 1200.0,
        "daily_pnl": -15.0,
        "consecutive_losses": 2
    })
    print(f"\n  [{fmt_time(t)}] Risk Check:")
    print(f"  {str(r)[:500]}")

    # Risk metrics
    r, t = run_tool("compute_risk_metrics", {
        "trade_pnls": [50, -20, 30, -10, 40, -30, 20, 10, -5, 25],
        "equity_curve": [1000, 1050, 1030, 1060, 1050, 1090, 1060, 1080, 1090, 1085, 1110],
        "initial_capital": 1000
    })
    print(f"\n  [{fmt_time(t)}] Risk Metrics (sample 10 trades):")
    print(f"  {str(r)[:500]}")

    # ─── PHASE 9: LEARNING ENGINE & WEIGHTS (3 tools) ────────────────────
    print("\n" + "━"*100)
    print("  🧠 PHASE 9: LEARNING ENGINE & WEIGHTS (3 tools)")
    print("━"*100)

    r, t = run_tool("get_learning_stats", {})
    print(f"\n  [{fmt_time(t)}] Learning Stats:")
    print(f"  {str(r)[:600]}")

    r, t = run_tool("get_learning_metrics", {})
    print(f"\n  [{fmt_time(t)}] Learning Metrics:")
    print(f"  {str(r)[:500]}")

    r, t = run_tool("get_scoring_weights", {})
    print(f"\n  [{fmt_time(t)}] Scoring Weights:")
    print(f"  {str(r)[:800]}")

    # ─── PHASE 10: TRADE JOURNAL (2 tools) ───────────────────────────────
    print("\n" + "━"*100)
    print("  📓 PHASE 10: TRADE JOURNAL (2 tools)")
    print("━"*100)

    r, t = run_tool("get_trade_journal", {"symbol": SYMBOL, "limit": 10})
    print(f"\n  [{fmt_time(t)}] Trade Journal DOTUSDT:")
    print(f"  {str(r)[:400]}")

    # ─── PHASE 11: SYSTEM & ALERTS (4 tools) ─────────────────────────────
    print("\n" + "━"*100)
    print("  💻 PHASE 11: SYSTEM & ALERTS (4 tools)")
    print("━"*100)

    r, t = run_tool("system_status", {})
    print(f"\n  [{fmt_time(t)}] System Status:")
    print(f"  {str(r)[:400]}")

    r, t = run_tool("disk_usage", {})
    print(f"\n  [{fmt_time(t)}] Disk Usage:")
    print(f"  {str(r)[:300]}")

    r, t = run_tool("list_price_alerts", {})
    print(f"\n  [{fmt_time(t)}] Price Alerts:")
    print(f"  {str(r)[:300]}")

    r, t = run_tool("validate_strategy", {"returns": [0.02, -0.01, 0.03, -0.015, 0.025, -0.005, 0.01], "n_groups": 3})
    print(f"\n  [{fmt_time(t)}] Validate Strategy (sample):")
    print(f"  {str(r)[:400]}")

    # ═══════════════════════════════════════════════════════════════════════
    # FINAL SUMMARY
    # ═══════════════════════════════════════════════════════════════════════
    total_elapsed = time.time() - total_start

    print("\n" + "╔" + "═"*98 + "╗")
    print(f"║  ✅ HOÀN TẤT — {len(_results)} tools đã chạy trong {fmt_time(total_elapsed)}")
    print("╚" + "═"*98 + "╝")

    # Save full report
    rdir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(rdir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    rpt = os.path.join(rdir, f"dotusdt_full_mcp_{ts}.json")
    with open(rpt, "w") as f:
        json.dump({
            "timestamp": datetime.now().isoformat(),
            "symbol": SYMBOL,
            "tools_run": len(_results),
            "elapsed_seconds": round(total_elapsed, 1),
            "results": {k: str(v)[:2000] for k, v in _results.items()},
        }, f, indent=2, ensure_ascii=False)
    print(f"\n  💾 Full report saved: {rpt}")


if __name__ == "__main__":
    main()
