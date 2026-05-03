#!/usr/bin/env python3
"""BonBo Quick Scanner — Quét nhanh tìm giao dịch tốt nhất ngay bây giờ.

Phase 1: Lấy top futures symbols → tính indicators (4h) → xếp hạng
Phase 2: Deep analysis top 10 → signals + regime + S/R
Phase 3: Recommendation — top 3 cơ hội tốt nhất
"""

import json
import os
import subprocess
import sys
import time
import re
import concurrent.futures
from datetime import datetime

MCP_BIN = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "target/release/bonbo-extend-mcp",
)

# Load env
env_file = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), ".env")
if os.path.isfile(env_file):
    with open(env_file) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, _, val = line.partition("=")
                os.environ[key.strip()] = val.strip()


def call_mcp(tool, args=None, timeout=45):
    """Call MCP tool with retry."""
    if args is None:
        args = {}
    init_req = json.dumps({
        "jsonrpc": "2.0", "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "bonbo-quick-scan", "version": "2.0"},
        },
        "id": "0",
    })
    call_req = json.dumps({
        "jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": tool, "arguments": args},
        "id": "1",
    })
    stdin_data = init_req + "\n" + call_req + "\n"
    env = os.environ.copy()
    env["BINANCE_MARKET_TYPE"] = "futures"
    try:
        p = subprocess.run(
            [MCP_BIN], input=stdin_data,
            capture_output=True, text=True, timeout=timeout, env=env,
        )
        for line in p.stdout.strip().split("\n"):
            try:
                r = json.loads(line)
                if "result" in r and "content" in r["result"]:
                    for c in r["result"]["content"]:
                        if c.get("type") == "text":
                            return c["text"]
            except json.JSONDecodeError:
                continue
    except Exception as e:
        print(f"  ⚠️ MCP error on {tool}: {e}", file=sys.stderr)
    return ""


def parse_score(text):
    """Extract total score from analysis text."""
    m = re.search(r"TOTAL.*?(\d+)\s*/\s*100", text, re.IGNORECASE | re.DOTALL)
    if m:
        return int(m.group(1))
    m = re.search(r"Score:\s*(\d+)/100", text, re.IGNORECASE)
    if m:
        return int(m.group(1))
    m = re.search(r"Score[:\s]+(\d+)", text, re.IGNORECASE)
    if m:
        return int(m.group(1))
    return 0


def parse_price(text):
    """Extract price from text."""
    m = re.search(r"\$([0-9,.]+)", text)
    if m:
        return float(m.group(1).replace(",", ""))
    return 0.0


def parse_verdict(text):
    """Extract overall verdict."""
    if "BULL" in text.upper() and "BEAR" not in text.upper():
        return "BULL"
    if "BEAR" in text.upper() and "BULL" not in text.upper():
        return "BEAR"
    if "BUY" in text.upper():
        return "BUY"
    if "SELL" in text.upper():
        return "SELL"
    return "MIXED"


def parse_regime(text):
    """Extract regime from text."""
    if "Trending Up" in text:
        return "📈 Trending Up"
    if "Trending Down" in text:
        return "📉 Trending Down"
    if "Ranging" in text:
        return "↔️ Ranging"
    if "Volatile" in text:
        return "⚡ Volatile"
    return text[:30] if text else "?"


def scan_symbol(symbol):
    """Quick scan a single symbol."""
    # 4h analysis
    text = call_mcp("analyze_position_full", {"symbol": symbol}, timeout=30)
    if not text:
        return None

    score = parse_score(text)
    verdict = parse_verdict(text)
    price = parse_price(text)

    # Extract key indicators
    hurst_m = re.search(r"Hurst\(100\).*?(\d+\.\d+)", text)
    hurst = float(hurst_m.group(1)) if hurst_m else 0.0

    rsi_m = re.search(r"RSI\(14\).*?(\d+\.\d+)", text)
    rsi = float(rsi_m.group(1)) if rsi_m else 50.0

    alma_m = re.search(r"ALMA.*?([+-]?\d+\.\d+)%", text)
    alma = float(alma_m.group(1)) if alma_m else 0.0

    macd_m = re.search(r"MACD.*?(🟢|🔴|Bullish|Bearish)", text)
    macd_bull = "🟢" in (macd_m.group(1) if macd_m else "")

    regime_m = re.search(r"Regime.*?(📈|📉|↔️|⚡|Trending|Ranging|Volatile)[^\n]*", text)
    regime = regime_m.group(0).strip() if regime_m else "?"

    # Multi-TF verdict
    mtf_m = re.search(r"Total:\s*(\d+)\s*Buy\s*/\s*(\d+)\s*Sell", text)
    buys = int(mtf_m.group(1)) if mtf_m else 0
    sells = int(mtf_m.group(2)) if mtf_m else 0

    # Action recommendation
    action = "HOLD"
    if "GIỮ" in text or "HOLD" in text.upper():
        action = "HOLD"
    if "MUA" in text or "BUY" in text.upper() or "LONG" in text.upper():
        action = "🟢 LONG"
    if "BÁN" in text or "SELL" in text.upper() or "SHORT" in text.upper():
        action = "🔴 SHORT"
    if "ĐÓNG" in text or "CLOSE" in text.upper():
        action = "❌ CLOSE"
    if "GIẢM" in text:
        action = "⚠️ REDUCE"

    return {
        "symbol": symbol,
        "score": score,
        "price": price,
        "verdict": verdict,
        "hurst": hurst,
        "rsi": rsi,
        "alma": alma,
        "macd_bull": macd_bull,
        "regime": regime,
        "buys": buys,
        "sells": sells,
        "action": action,
    }


# ═════════════════════════════════════════════════════════════════
# MAIN
# ═════════════════════════════════════════════════════════════════

def main():
    print("=" * 70)
    print("  🔍 BONBO QUICK SCANNER — Tìm Giao Dịch Tốt Nhất Hiện Tại")
    print(f"  📅 {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("=" * 70)

    # ─── Step 1: Sentiment ───
    print("\n📊 Bước 1: Kiểm tra Sentiment...")
    print("-" * 50)
    sentiment_text = call_mcp("get_composite_sentiment", {})
    fg_text = call_mcp("get_fear_greed_index", {})
    print(sentiment_text or "  (no data)")
    print(fg_text or "  (no data)")

    # ─── Step 2: Get top futures symbols ───
    print("\n💰 Bước 2: Lấy danh sách Futures symbols...")
    print("-" * 50)

    # Top 40 most active futures
    SYMBOLS = [
        "BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT",
        "DOGEUSDT", "ADAUSDT", "AVAXUSDT", "LINKUSDT", "DOTUSDT",
        "MATICUSDT", "LTCUSDT", "UNIUSDT", "APTUSDT", "OPUSDT",
        "ARBUSDT", "NEARUSDT", "ATOMUSDT", "FILUSDT", "AAVEUSDT",
        "CRVUSDT", "SUIUSDT", "SEIUSDT", "PENDLEUSDT", "INJUSDT",
        "TIAUSDT", "STXUSDT", "IMXUSDT", "RUNEUSDT", "GRTUSDT",
        "RNDRUSDT", "WLDUSDT", "JUPUSDT", "WUSDT", "ENAUSDT",
        "ETHFIUSDT", "PEPEUSDT", "FETUSDT", "AGIXUSDT", "RENDERUSDT",
    ]
    print(f"  📋 Scanning {len(SYMBOLS)} symbols...")

    # ─── Step 3: Quick scan all symbols (parallel) ───
    print("\n🔍 Bước 3: Quick scan tất cả symbols...")
    print("-" * 50)

    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        future_map = {pool.submit(scan_symbol, s): s for s in SYMBOLS}
        done_count = 0
        for future in concurrent.futures.as_completed(future_map):
            done_count += 1
            sym = future_map[future]
            try:
                r = future.result()
                if r and r["score"] > 0:
                    results.append(r)
                    emoji = "🟢" if r["score"] >= 65 else "🟡" if r["score"] >= 50 else "⚪"
                    print(f"  {emoji} [{done_count:2d}/{len(SYMBOLS)}] {sym:15s} Score={r['score']:3d} | {r['action']:15s} | RSI={r['rsi']:.0f} | Hurst={r['hurst']:.3f} | {r['verdict']}")
                else:
                    print(f"  ⚪ [{done_count:2d}/{len(SYMBOLS)}] {sym:15s} (no data)")
            except Exception as e:
                print(f"  ❌ [{done_count:2d}/{len(SYMBOLS)}] {sym:15s} error: {e}")

    if not results:
        print("\n❌ Không có kết quả. Thử scan bằng analyze_position_full từng symbol...")

        # Fallback: try analyze_indicators
        for sym in SYMBOLS[:20]:
            text = call_mcp("technical_analysis", {"symbol": sym, "timeframe": "4h"}, timeout=20)
            if text:
                score = parse_score(text)
                price = parse_price(text)
                verdict = parse_verdict(text)
                rsi_m = re.search(r"RSI.*?(\d+\.\d+)", text)
                rsi = float(rsi_m.group(1)) if rsi_m else 50.0
                hurst_m = re.search(r"Hurst.*?(\d+\.\d+)", text)
                hurst = float(hurst_m.group(1)) if hurst_m else 0.0
                results.append({
                    "symbol": sym, "score": max(score, 30),
                    "price": price, "verdict": verdict,
                    "hurst": hurst, "rsi": rsi, "alma": 0,
                    "macd_bull": "🟢" in text,
                    "regime": "?", "buys": 0, "sells": 0,
                    "action": verdict,
                })
                print(f"  {sym:15s} Score={max(score,30):3d} | Price=${price:.4f} | RSI={rsi:.1f}")

    # ─── Step 4: Rank and display ───
    results.sort(key=lambda x: x["score"], reverse=True)

    print("\n" + "=" * 70)
    print("  🏆 BẢNG XẾP HẠNG — TOP CƠ HỘI GIAO DỊCH")
    print("=" * 70)

    for i, r in enumerate(results[:20]):
        rank = i + 1
        if r["score"] >= 70:
            grade = "🟢 STRONG"
        elif r["score"] >= 60:
            grade = "🟡 GOOD"
        elif r["score"] >= 50:
            grade = "⚪ FAIR"
        else:
            grade = "🔴 WEAK"

        action_color = "📈" if "LONG" in r["action"] else "📉" if "SHORT" in r["action"] else "⚪"
        mtf = f"{r['buys']}B/{r['sells']}S" if r['buys'] + r['sells'] > 0 else "-"
        macd = "🟢" if r["macd_bull"] else "🔴"

        print(f"\n  #{rank:2d} {r['symbol']:15s} — Score: {r['score']:3d}/100 {grade}")
        print(f"      💰 Price: ${r['price']:.4f} | {action_color} {r['action']}")
        print(f"      📊 RSI={r['rsi']:.1f} | Hurst={r['hurst']:.3f} | ALMA={r['alma']:+.2f}% | MACD={macd}")
        print(f"      📡 MTF: {mtf} | Regime: {r['regime']}")

    # ─── Step 5: Deep analysis for top 5 ───
    print("\n\n" + "=" * 70)
    print("  🔬 DEEP ANALYSIS — TOP 5 CƠ HỘI TỐT NHẤT")
    print("=" * 70)

    for i, r in enumerate(results[:5]):
        print(f"\n{'─' * 60}")
        print(f"  #{i+1} {r['symbol']} (Score: {r['score']}/100)")
        print(f"{'─' * 60}")

        # Regime detection
        regime_text = call_mcp("detect_market_regime", {"symbol": r["symbol"], "timeframe": "4h"})
        if regime_text:
            # Extract key info
            lines = [l for l in regime_text.split("\n") if l.strip()]
            for line in lines[:6]:
                print(f"  {line}")

        # S/R levels
        sr_text = call_mcp("get_support_resistance", {"symbol": r["symbol"], "timeframe": "4h"})
        if sr_text:
            lines = [l for l in sr_text.split("\n") if l.strip()]
            for line in lines[:8]:
                print(f"  {line}")

    # ─── Summary Recommendation ───
    print("\n\n" + "=" * 70)
    print("  ✅ TÓM TẮT KHUYẾN NGHỊ")
    print("=" * 70)

    # Filter for actionable signals
    longs = [r for r in results if r["score"] >= 60 and ("LONG" in r["action"] or "BUY" in r["action"] or "BULL" in r["verdict"] or r["buys"] > r["sells"])]
    shorts = [r for r in results if r["score"] >= 60 and ("SHORT" in r["action"] or "SELL" in r["action"] or "BEAR" in r["verdict"] or r["sells"] > r["buys"])]

    if longs:
        print(f"\n  📈 TOP LONG CƠ HỘI:")
        for r in longs[:5]:
            print(f"     🟢 {r['symbol']:15s} Score={r['score']:3d} | RSI={r['rsi']:.0f} | Hurst={r['hurst']:.3f} | ${r['price']:.4f}")

    if shorts:
        print(f"\n  📉 TOP SHORT CƠ HỘI:")
        for r in shorts[:3]:
            print(f"     🔴 {r['symbol']:15s} Score={r['score']:3d} | RSI={r['rsi']:.0f} | Hurst={r['hurst']:.3f} | ${r['price']:.4f}")

    # Best overall
    if results:
        best = results[0]
        print(f"\n  ⭐ CƠ HỘI TỐT NHẤT: {best['symbol']}")
        print(f"     Score: {best['score']}/100 | Action: {best['action']}")
        print(f"     Price: ${best['price']:.4f} | RSI: {best['rsi']:.1f} | Hurst: {best['hurst']:.3f}")

        # Position size recommendation
        equity = 390.42  # from balance
        risk_pct = 2.0
        ps_text = call_mcp("calculate_position_size", {
            "equity": equity,
            "entry_price": best["price"],
            "stop_loss": best["price"] * 0.95,  # 5% SL
            "risk_pct": risk_pct,
        })
        if ps_text:
            print(f"\n  📐 Kích thước vị thế đề xuất (risk {risk_pct}%/trade):")
            lines = [l for l in ps_text.split("\n") if l.strip()]
            for line in lines[:5]:
                print(f"     {line}")

    print(f"\n{'=' * 70}")
    print(f"  ✅ Scan hoàn tất! {len(results)} symbols được phân tích.")
    print(f"  📅 {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"{'=' * 70}")


if __name__ == "__main__":
    main()
