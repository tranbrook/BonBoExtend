#!/usr/bin/env python3
"""
BonBo Self-Learning Loop — Autonomous AI Trading Learning Cycle

Chạy tự động:
  1. SCAN: Quét top crypto → phân tích → chấm điểm
  2. ANALYZE: Phân tích kỹ thuật chi tiết top picks
  3. JOURNAL: Ghi nhận vào trade journal
  4. BACKTEST: Validate strategies
  5. REVIEW: Kiểm tra past predictions vs outcomes
  6. LEARN: Tune weights dựa trên accuracy

Usage:
  python3 self_learn.py [--interval 300] [--top-n 10] [--once]
  
  --interval SECONDS   Thời gian giữa các cycle (default: 300 = 5 phút)
  --top-n N            Số coins để scan (default: 10)
  --once               Chỉ chạy 1 cycle rồi thoát
"""

import json
import sys
import time
import argparse
import urllib.request
import uuid
from datetime import datetime
from pathlib import Path

MCP_URL = "http://localhost:9876/mcp"
JOURNAL_DIR = Path.home() / ".bonbo" / "self_learning"
LOG_FILE = JOURNAL_DIR / "learning_log.txt"


def mcp_call(tool_name: str, arguments: dict = None) -> dict:
    """Call MCP tool and return parsed result."""
    if arguments is None:
        arguments = {}
    
    payload = json.dumps({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        },
        "id": str(uuid.uuid4())[:8]
    }).encode()
    
    req = urllib.request.Request(
        MCP_URL,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST"
    )
    
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.loads(resp.read().decode())
            content = data.get("result", {}).get("content", [])
            if content:
                return {"text": content[0].get("text", ""), "error": False}
            return {"text": "No content", "error": False}
    except Exception as e:
        return {"text": str(e), "error": True}


def log(msg: str):
    """Log message with timestamp."""
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    line = f"[{timestamp}] {msg}"
    print(line)
    JOURNAL_DIR.mkdir(parents=True, exist_ok=True)
    with open(LOG_FILE, "a") as f:
        f.write(line + "\n")


def parse_scan_results(text: str) -> list:
    """Parse scan_market results to extract symbols and scores."""
    results = []
    for line in text.split("\n"):
        if "USDT" in line and "Score:" in line:
            parts = line.strip()
            # Extract symbol
            if "⚪" in parts or "🟢" in parts or "🔴" in parts:
                symbol_start = parts.find("USDT")
                if symbol_start > 0:
                    symbol_end = symbol_start + 4
                    # Find the symbol (go backwards to find start)
                    s = parts[:symbol_end]
                    tokens = s.split()
                    symbol = tokens[-1] if tokens else ""
                    
                    # Extract score
                    score = 0
                    if "Score:" in parts:
                        score_str = parts.split("Score:")[1].split("|")[0].strip().split()[0]
                        try:
                            score = float(score_str)
                        except:
                            score = 0
                    
                    # Extract price
                    price = 0
                    if "$" in parts:
                        price_str = parts.split("$")[1].split("|")[0].strip().replace(",", "")
                        try:
                            price = float(price_str)
                        except:
                            price = 0
                    
                    if symbol and "USDT" in symbol:
                        results.append({
                            "symbol": symbol,
                            "score": score,
                            "price": price
                        })
    return results


def run_scan_cycle(top_n: int) -> list:
    """Step 1: Scan market and return top picks."""
    log(f"🔍 Step 1: Scanning top {top_n} cryptos...")
    result = mcp_call("scan_market", {"top_n": top_n})
    
    if result["error"]:
        log(f"  ❌ Scan failed: {result['text']}")
        return []
    
    picks = parse_scan_results(result["text"])
    log(f"  ✅ Scanned {len(picks)} symbols")
    
    for p in picks[:5]:
        emoji = "🟢" if p["score"] >= 60 else "🟡" if p["score"] >= 50 else "🔴"
        log(f"  {emoji} {p['symbol']} — Score: {p['score']:.0f} @ ${p['price']:.2f}")
    
    return picks


def run_analysis(symbol: str) -> dict:
    """Step 2: Deep analysis of a symbol."""
    log(f"📊 Step 2: Analyzing {symbol}...")
    
    # Get indicators
    indicators = mcp_call("analyze_indicators", {
        "symbol": symbol, "interval": "1h", "limit": 100
    })
    
    # Get signals
    signals = mcp_call("get_trading_signals", {
        "symbol": symbol, "interval": "1h"
    })
    
    # Get regime
    regime = mcp_call("detect_market_regime", {
        "symbol": symbol, "interval": "1h"
    })
    
    # Get sentiment
    sentiment = mcp_call("get_composite_sentiment", {})
    
    # Get fear/greed
    fear_greed = mcp_call("get_fear_greed_index", {})
    
    return {
        "symbol": symbol,
        "indicators": indicators["text"],
        "signals": signals["text"],
        "regime": regime["text"],
        "sentiment": sentiment["text"],
        "fear_greed": fear_greed["text"],
    }


def run_backtest(symbol: str) -> dict:
    """Step 3: Backtest validation."""
    log(f"📈 Step 3: Backtesting {symbol}...")
    
    strategies = ["sma_crossover", "rsi_reversal", "bb_bounce", "macd_crossover"]
    results = {}
    
    for strategy in strategies:
        result = mcp_call("run_backtest", {
            "symbol": symbol,
            "interval": "1h",
            "strategy": strategy,
            "period": 30
        })
        results[strategy] = result["text"]
    
    # Find best strategy
    best_strategy = "sma_crossover"
    best_return = -999
    for name, text in results.items():
        if "Total Return" in text:
            try:
                ret_str = text.split("Total Return")[1].split("%")[0].strip().replace(":", "").strip()
                ret = float(ret_str)
                if ret > best_return:
                    best_return = ret
                    best_strategy = name
            except:
                pass
    
    log(f"  📊 Best strategy for {symbol}: {best_strategy} (return: {best_return:.2f}%)")
    return {"best_strategy": best_strategy, "best_return": best_return, "all": results}


def journal_entry(analysis: dict, score: float, backtest: dict, price: float) -> str:
    """Step 4: Record to journal."""
    symbol = analysis["symbol"]
    
    # Parse indicator values
    rsi = 50.0
    if "RSI" in analysis["indicators"]:
        try:
            rsi_str = analysis["indicators"].split("RSI(14)")[1].split("⚪")[0].split("🔴")[0].split("🟢")[0].strip().split()[0].replace(":", "").strip()
            rsi = float(rsi_str)
        except:
            pass
    
    macd_h = 0.0
    if "hist=" in analysis["indicators"]:
        try:
            macd_str = analysis["indicators"].split("hist=")[1].split(" ")[0]
            macd_h = float(macd_str)
        except:
            pass
    
    bb_b = 0.5
    if "%B=" in analysis["indicators"]:
        try:
            bb_str = analysis["indicators"].split("%B=")[1].split(" ")[0]
            bb_b = float(bb_str)
        except:
            pass
    
    # Determine recommendation
    if score >= 70:
        rec = "STRONG_BUY"
    elif score >= 60:
        rec = "BUY"
    elif score >= 40:
        rec = "HOLD"
    elif score >= 30:
        rec = "SELL"
    else:
        rec = "STRONG_SELL"
    
    # Calculate stop loss and target
    stop_loss = round(price * 0.97, 2)
    target = round(price * 1.05, 2)
    
    result = mcp_call("journal_trade_entry", {
        "symbol": symbol,
        "price": price,
        "recommendation": rec,
        "quant_score": score,
        "stop_loss": stop_loss,
        "target_price": target,
        "rsi": rsi,
        "macd_histogram": macd_h,
        "bb_percent_b": bb_b,
        "buy_signals": 1 if score >= 55 else 0,
        "sell_signals": 1 if score < 45 else 0,
    })
    
    # Extract entry ID
    entry_id = ""
    if "ID:" in result["text"]:
        try:
            entry_id = result["text"].split("ID:")[1].split("\n")[0].strip().strip("`")
        except:
            pass
    
    log(f"  📝 Journal entry: {symbol} {rec} Score={score:.0f} ID={entry_id[:8]}...")
    return entry_id


def review_past_predictions() -> dict:
    """Step 5: Review past predictions and check outcomes."""
    log("📚 Step 5: Reviewing past predictions...")
    
    result = mcp_call("get_trade_journal", {"limit": 50})
    metrics = mcp_call("get_learning_metrics", {})
    
    log(f"  📊 {metrics['text'][:200]}")
    return {"journal": result["text"], "metrics": metrics["text"]}


def check_learning() -> dict:
    """Step 6: Check learning weights and stats."""
    log("🧠 Step 6: Checking learning weights...")
    
    weights = mcp_call("get_scoring_weights", {})
    stats = mcp_call("get_learning_stats", {})
    
    log(f"  ⚖️ Weights loaded")
    return {"weights": weights["text"], "stats": stats["text"]}


def run_full_cycle(top_n: int = 10) -> dict:
    """Run one complete self-learning cycle."""
    cycle_start = time.time()
    log("=" * 60)
    log(f"🔄 STARTING SELF-LEARNING CYCLE")
    log("=" * 60)
    
    # Step 1: Scan
    picks = run_scan_cycle(top_n)
    if not picks:
        log("⚠️ No picks found, skipping cycle")
        return {"status": "no_picks"}
    
    # Focus on top 3 picks
    top_picks = sorted(picks, key=lambda x: x["score"], reverse=True)[:3]
    
    entries = []
    for pick in top_picks:
        symbol = pick["symbol"]
        price = pick["price"]
        score = pick["score"]
        
        log(f"\n{'─'*40}")
        log(f"🎯 Processing {symbol} (Score: {score:.0f})")
        log(f"{'─'*40}")
        
        # Step 2: Analysis
        analysis = run_analysis(symbol)
        
        # Step 3: Backtest
        backtest = run_backtest(symbol)
        
        # Step 4: Journal
        entry_id = journal_entry(analysis, score, backtest, price)
        entries.append({"symbol": symbol, "entry_id": entry_id, "score": score})
    
    # Step 5: Review
    review = review_past_predictions()
    
    # Step 6: Check learning
    learning = check_learning()
    
    cycle_time = time.time() - cycle_start
    log(f"\n{'='*60}")
    log(f"✅ CYCLE COMPLETE in {cycle_time:.1f}s")
    log(f"   Entries: {len(entries)}")
    for e in entries:
        log(f"   📝 {e['symbol']} — Score: {e['score']:.0f}")
    log(f"{'='*60}\n")
    
    return {
        "status": "success",
        "picks": picks,
        "entries": entries,
        "cycle_time": cycle_time,
    }


def main():
    parser = argparse.ArgumentParser(description="BonBo Self-Learning Loop")
    parser.add_argument("--interval", type=int, default=300, help="Seconds between cycles (default: 300)")
    parser.add_argument("--top-n", type=int, default=10, help="Number of coins to scan (default: 10)")
    parser.add_argument("--once", action="store_true", help="Run only one cycle")
    args = parser.parse_args()
    
    log("🚀 BonBo Self-Learning Loop Starting")
    log(f"   Interval: {args.interval}s | Top-N: {args.top_n}")
    log(f"   MCP URL: {MCP_URL}")
    
    # Verify MCP server is running
    test = mcp_call("system_status", {})
    if test["error"]:
        log(f"❌ MCP server not responding at {MCP_URL}")
        log(f"   Start it: cargo run --release -p bonbo-extend-mcp -- --http --port 9876")
        sys.exit(1)
    
    log("✅ MCP server connected")
    
    cycle_count = 0
    while True:
        cycle_count += 1
        log(f"\n🔄 Cycle #{cycle_count}")
        
        try:
            result = run_full_cycle(args.top_n)
        except Exception as e:
            log(f"❌ Cycle failed: {e}")
            result = {"status": "error"}
        
        if args.once:
            log("🏁 Single cycle mode — exiting")
            break
        
        log(f"😴 Sleeping {args.interval}s until next cycle...")
        time.sleep(args.interval)


if __name__ == "__main__":
    main()
