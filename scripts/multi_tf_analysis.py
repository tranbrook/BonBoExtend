#!/usr/bin/env python3
"""
BonBo Multi-Timeframe Deep Analysis v2 — Tìm giao dịch tốt nhất ngay lúc này

MCP tools return markdown text, not JSON. This version parses text correctly.

Phân tích:
  - Top coins by volume từ Binance
  - Multi-timeframe: 15m, 1h, 4h, 1d
  - Financial-Hacker indicators (Hurst, ALMA, SuperSmoother, LaguerreRSI, CMO)
  - Market regime detection
  - Support/Resistance levels
  - Sentiment (Fear & Greed)
  - Composite scoring → Rank → TOP trades
"""

import json
import os
import re
import subprocess
import sys
import time
from datetime import datetime

MCP_BIN = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                        "target/release/bonbo-extend-mcp")

TIMEFRAMES = ["15m", "1h", "4h", "1d"]
TF_WEIGHTS = {"15m": 0.10, "1h": 0.30, "4h": 0.35, "1d": 0.25}

_seq = 0


def mcp_call(tool, args=None, timeout=30):
    """Call single MCP tool via stdio. Returns raw text."""
    global _seq
    _seq += 1
    if args is None:
        args = {}

    # Build multi-line JSON-RPC: init + call
    init_req = json.dumps({
        "jsonrpc": "2.0", "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "bonbo-mtf", "version": "2.0"}},
        "id": "0"
    })
    call_req = json.dumps({
        "jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": tool, "arguments": args},
        "id": str(_seq)
    })
    stdin_data = init_req + "\n" + call_req + "\n"

    try:
        p = subprocess.run([MCP_BIN], input=stdin_data,
                           capture_output=True, text=True, timeout=timeout)
        for line in p.stdout.strip().split('\n'):
            try:
                r = json.loads(line)
                if "result" in r and "content" in r["result"]:
                    for c in r["result"]["content"]:
                        if c.get("type") == "text":
                            return c["text"]
            except json.JSONDecodeError:
                continue
    except subprocess.TimeoutExpired:
        return ""
    except Exception:
        return ""
    return ""


# ── Text Parsers ───────────────────────────────────────────────────

def parse_price(text):
    """Extract price from various MCP responses."""
    m = re.search(r'\$([0-9,.]+)', text)
    if m:
        return float(m.group(1).replace(",", ""))
    return 0.0


def parse_hurst(text):
    """Extract Hurst exponent from text."""
    m = re.search(r'Hurst[^:]*:\s*([0-9.]+)', text)
    if m:
        return float(m.group(1))
    return 0.0


def parse_rsi(text):
    """Extract RSI value."""
    m = re.search(r'RSI[^:]*:\s*([0-9.]+)', text)
    if m:
        return float(m.group(1))
    return 50.0


def parse_laguerre_rsi(text):
    """Extract LaguerreRSI value."""
    m = re.search(r'LaguerreRSI[^:]*:\s*([0-9.]+)', text)
    if m:
        return float(m.group(1))
    return 0.5


def parse_cmo(text):
    """Extract CMO value."""
    m = re.search(r'CMO[^:]*:\s*([-0-9.]+)', text)
    if m:
        return float(m.group(1))
    return 0.0


def parse_alma_signal(text):
    """Extract ALMA signal info."""
    m = re.search(r'ALMA[^→]*→\s*(🟢|🔴)\s*(\w+)\s*\(?([+-0-9.]+)%?\)?', text)
    if m:
        return m.group(0).strip()
    m = re.search(r'(ALMA.*?)(?:\n|$)', text)
    return m.group(0).strip() if m else ""


def parse_supersmoother(text):
    """Extract SuperSmoother info."""
    m = re.search(r'(SuperSmoother.*?)(?:\n|$)', text)
    return m.group(0).strip() if m else ""


def parse_macd(text):
    """Extract MACD info."""
    m = re.search(r'(MACD.*?)(?:\n|$)', text)
    return m.group(0).strip() if m else ""


def parse_bollinger(text):
    """Extract Bollinger %B."""
    m = re.search(r'%B=([0-9.]+)', text)
    return float(m.group(1)) if m else 0.5


def parse_signals(text):
    """Parse trading signals from text. Returns list of {direction, name, confidence, detail}."""
    signals = []
    # Match lines like: 🟢 **Buy** [MACD(12,26,9)] (60%)
    pattern = r'(🟢|🔴)\s+\*\*(Buy|Sell)\*\*\s+\[([^\]]+)\]\s+\(([0-9]+)%\)'
    for m in re.finditer(pattern, text):
        direction = m.group(2).upper()
        name = m.group(3)
        confidence = int(m.group(4))
        signals.append({
            "direction": direction,
            "name": name,
            "confidence": confidence,
        })
    return signals


def parse_regime(text):
    """Parse market regime."""
    # Look for regime keywords
    text_lower = text.lower()
    hurst = 0.0
    m = re.search(r'Hurst[^:]*:\s*([0-9.]+)', text)
    if m:
        hurst = float(m.group(1))

    if "trending" in text_lower:
        regime = "TRENDING"
    elif "quiet" in text_lower:
        regime = "QUIET"
    elif "volatile" in text_lower:
        regime = "VOLATILE"
    elif "mean-revert" in text_lower:
        regime = "MEAN_REVERTING"
    elif "random" in text_lower:
        regime = "RANDOM_WALK"
    else:
        regime = "UNKNOWN"

    return {"regime": regime, "hurst": hurst, "raw": text}


def parse_support_resistance(text):
    """Parse S/R levels."""
    supports = []
    resistances = []
    for m in re.finditer(r'(S\d?):\s*\$([0-9,.]+)\s*\([-0-9.]+%\)', text):
        supports.append(float(m.group(2).replace(",", "")))
    for m in re.finditer(r'(R\d?):\s*\$([0-9,.]+)\s*\([+0-9.]+%\)', text):
        resistances.append(float(m.group(2).replace(",", "")))
    return {"supports": supports, "resistances": resistances}


def parse_sma(text):
    m = re.search(r'SMA\(20\)[^:]*:\s*\$([0-9,.]+)', text)
    return float(m.group(1).replace(",", "")) if m else 0.0


def parse_ema12(text):
    m = re.search(r'EMA\(12\)[^:]*:\s*\$([0-9,.]+)', text)
    return float(m.group(1).replace(",", "")) if m else 0.0


def parse_ema26(text):
    m = re.search(r'EMA\(26\)[^:]*:\s*\$([0-9,.]+)', text)
    return float(m.group(1).replace(",", "")) if m else 0.0


# ── Data Fetchers ──────────────────────────────────────────────────

def get_top_coins(n=30):
    """Get top N coins by volume."""
    text = mcp_call("get_top_crypto", {"limit": n, "sort_by": "volume"})
    symbols = []
    # Parse symbols from text like "1. BTCUSDT — $87,123 ..."
    for m in re.finditer(r'(?:\d+\.\s+)([A-Z]+USDT)', text):
        symbols.append(m.group(1))
    # Fallback if no matches
    if not symbols:
        for m in re.finditer(r'([A-Z]{2,}USDT)', text):
            sym = m.group(1)
            if sym not in symbols:
                symbols.append(sym)
    if not symbols:
        symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT",
                   "DOGEUSDT", "ADAUSDT", "AVAXUSDT", "LINKUSDT", "SUIUSDT",
                   "DOTUSDT", "NEARUSDT", "APTUSDT", "ARBUSDT", "UNIUSDT"][:n]
    return symbols[:n]


# ── Scoring Engine ─────────────────────────────────────────────────

def score_timeframe(ind_text, sig_text, reg_text, sr_text):
    """Score a single timeframe. Returns (score_0_100, details_dict)."""
    details = {}

    price = parse_price(ind_text)
    hurst = parse_hurst(ind_text) or parse_hurst(reg_text)
    lag_rsi = parse_laguerre_rsi(ind_text)
    cmo = parse_cmo(ind_text)
    rsi = parse_rsi(ind_text)
    bb_pct = parse_bollinger(ind_text)
    alma = parse_alma_signal(ind_text)
    ss = parse_supersmoother(ind_text)
    macd_text = parse_macd(ind_text)
    signals = parse_signals(sig_text)
    regime = parse_regime(reg_text)
    sr = parse_support_resistance(sr_text)

    details["price"] = price
    details["hurst"] = hurst
    details["hurst_regime"] = regime["regime"]
    details["laguerre_rsi"] = lag_rsi
    details["cmo"] = cmo
    details["rsi"] = rsi
    details["bb_pct"] = bb_pct
    details["signals"] = signals
    details["regime"] = regime
    details["sr"] = sr
    details["alma"] = alma
    details["ss"] = ss

    total_score = 0.0
    total_weight = 0.0

    # ── 1. Hurst Regime (0-20 pts) ──
    if hurst > 0.60:
        h_score = 20.0
    elif hurst > 0.55:
        h_score = 16.0
    elif hurst > 0.50:
        h_score = 10.0
    elif hurst > 0.0:
        h_score = 4.0
    else:
        h_score = 5.0  # Unknown
    total_score += h_score
    total_weight += 20.0
    details["hurst_score"] = h_score

    # ── 2. Signal Confluence (0-30 pts) ──
    buy_signals = [s for s in signals if s["direction"] == "BUY"]
    sell_signals = [s for s in signals if s["direction"] == "SELL"]

    if not signals:
        sig_score = 5.0
    elif len(buy_signals) > len(sell_signals):
        avg_conf = sum(s["confidence"] for s in buy_signals) / len(buy_signals)
        sig_score = 10.0 + (len(buy_signals) / len(signals)) * 15.0 + (avg_conf / 100.0) * 5.0
    elif len(sell_signals) > len(buy_signals):
        avg_conf = sum(s["confidence"] for s in sell_signals) / len(sell_signals)
        sig_score = 5.0 + (len(sell_signals) / len(signals)) * 10.0 + (avg_conf / 100.0) * 5.0
    else:
        sig_score = 8.0

    sig_score = min(sig_score, 30.0)
    total_score += sig_score
    total_weight += 30.0
    details["signal_score"] = sig_score

    # ── 3. RSI + LaguerreRSI (0-15 pts) ──
    if lag_rsi < 0.2:
        rsi_score = 15.0  # Oversold
    elif lag_rsi < 0.35:
        rsi_score = 11.0
    elif lag_rsi < 0.65:
        rsi_score = 7.0
    elif lag_rsi < 0.8:
        rsi_score = 5.0
    else:
        rsi_score = 3.0  # Overbought

    total_score += rsi_score
    total_weight += 15.0
    details["rsi_score"] = rsi_score

    # ── 4. CMO Momentum (0-15 pts) ──
    if abs(cmo) > 40:
        cmo_score = 13.0
    elif abs(cmo) > 20:
        cmo_score = 9.0
    elif abs(cmo) > 10:
        cmo_score = 6.0
    else:
        cmo_score = 3.0
    total_score += cmo_score
    total_weight += 15.0
    details["cmo_score"] = cmo_score

    # ── 5. Trend ALMA/SS alignment (0-20 pts) ──
    alma_bull = "Bullish" in alma or "🟢" in alma
    ss_bull = "positive" in ss.lower() or "🟢" in ss
    alma_bear = "Bearish" in alma or "🔴" in alma
    ss_bear = "negative" in ss.lower() or "🔴" in ss

    if alma_bull and ss_bull:
        trend_score = 20.0
    elif alma_bull or ss_bull:
        trend_score = 13.0
    elif alma_bear and ss_bear:
        trend_score = 8.0  # Bearish = could short
    else:
        trend_score = 6.0

    total_score += trend_score
    total_weight += 20.0
    details["trend_score"] = trend_score

    # Normalize
    final_score = (total_score / total_weight * 100) if total_weight > 0 else 0
    return min(final_score, 100.0), details


def compute_composite_score(tf_results):
    """Compute weighted multi-timeframe composite score."""
    weighted = 0.0
    tf_scores = {}
    tf_details = {}

    for tf in TIMEFRAMES:
        if tf not in tf_results:
            continue
        score, details = tf_results[tf]
        weight = TF_WEIGHTS.get(tf, 0.25)
        weighted += score * weight
        tf_scores[tf] = score
        tf_details[tf] = details

    return weighted, tf_scores, tf_details


def compute_confluence(tf_scores, tf_details):
    """Check if timeframes agree on direction."""
    directions = {}
    for tf in TIMEFRAMES:
        if tf not in tf_details:
            continue
        signals = tf_details[tf].get("signals", [])
        buys = sum(1 for s in signals if s["direction"] == "BUY")
        sells = sum(1 for s in signals if s["direction"] == "SELL")
        if buys > sells:
            directions[tf] = "BUY"
        elif sells > buys:
            directions[tf] = "SELL"
        else:
            directions[tf] = "NEUTRAL"

    buy_count = sum(1 for v in directions.values() if v == "BUY")
    sell_count = sum(1 for v in directions.values() if v == "SELL")
    total = max(len(directions), 1)

    if buy_count >= 3:
        return "STRONG_BUY", directions, (buy_count / total) * 100
    elif buy_count >= 2:
        return "BUY", directions, (buy_count / total) * 100
    elif sell_count >= 3:
        return "STRONG_SELL", directions, (sell_count / total) * 100
    elif sell_count >= 2:
        return "SELL", directions, (sell_count / total) * 100
    else:
        return "MIXED", directions, 50.0


# ── Display Helpers ────────────────────────────────────────────────

def C(code, text):
    """Color text with ANSI code."""
    return f"\033[{code}m{text}\033[0m"

def BOLD(text): return C("1", text)
def GREEN(text): return C("32", text)
def BGREEN(text): return C("1;32", text)
def RED(text): return C("31", text)
def BRED(text): return C("1;31", text)
def YELLOW(text): return C("33", text)
def CYAN(text): return C("36", text)
def BCYAN(text): return C("1;36", text)
def DIM(text): return C("2", text)

def fmt_score(score):
    if score >= 70: return BGREEN(f"{score:.1f}")
    elif score >= 55: return GREEN(f"{score:.1f}")
    elif score >= 40: return YELLOW(f"{score:.1f}")
    else: return RED(f"{score:.1f}")

def fmt_dir(direction):
    if direction == "BUY": return BGREEN("▲ BUY")
    elif direction == "SELL": return BRED("▼ SELL")
    elif direction == "STRONG_BUY": return BGREEN("▲▲ STRONG BUY")
    elif direction == "STRONG_SELL": return BRED("▼▼ STRONG SELL")
    else: return YELLOW("● MIXED")

def fmt_hurst(h):
    if h > 0.55: return GREEN(f"{h:.3f} Trending")
    elif h > 0.45: return YELLOW(f"{h:.3f} Random")
    else: return RED(f"{h:.3f} MeanRev") if h > 0 else DIM("N/A")


# ── Main ───────────────────────────────────────────────────────────

def main():
    quick = "--quick" in sys.argv
    full = "--full" in sys.argv
    top_n = 30

    if quick:
        top_n = 15
        timeframes = ["1h", "4h"]
    elif full:
        top_n = 50
        timeframes = TIMEFRAMES
    else:
        timeframes = TIMEFRAMES
        top_n = 25

    print()
    print(BOLD("=" * 100))
    print(BOLD("  🔥 BONBO MULTI-TIMEFRAME DEEP ANALYSIS — TÌM GIAO DỊCH TỐT NHẤT LÚC NÀY"))
    print(f"  📅 {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} | TFs: {', '.join(timeframes)} | Coins: {top_n}")
    print(BOLD("=" * 100))
    print()

    # ── 1. SENTIMENT ──
    print(DIM("━" * 100))
    print(BOLD("  PHẦN 1: MARKET SENTIMENT"))
    print(DIM("━" * 100))

    fg_text = mcp_call("get_fear_greed_index", {"history": 1})
    # Extract Fear/Greed value
    fg_match = re.search(r'(\d+)/100', fg_text)
    fg_val = fg_match.group(1) if fg_match else "?"
    fg_label = "Fear" if "Fear" in fg_text and "Greed" not in fg_text else \
               "Greed" if "Greed" in fg_text else "Neutral"
    print(f"  Fear & Greed Index: {fg_val}/100 ({fg_label})")

    # Color the sentiment
    if fg_label == "Fear":
        print(f"  → " + RED("Thị trường SỢ HÃI — có thể là cơ hội mua (contrarian)"))
    elif fg_label == "Greed":
        print(f"  → " + GREEN("Thị trường THAM LAM — cẩn thận đảo chiều"))
    else:
        print(f"  → " + YELLOW("Thị trường TRUNG TÍNH"))

    sentiment_text = mcp_call("get_composite_sentiment", {"symbol": "BTCUSDT"})
    s_match = re.search(r'Score:\s*([-0-9.]+)', sentiment_text)
    s_val = float(s_match.group(1)) if s_match else 0
    print(f"  Composite Sentiment: {s_val:.2f}")
    print()

    # ── 2. GET TOP COINS ──
    print(DIM("━" * 100))
    print(BOLD("  PHẦN 2: QUÉT THỊ TRƯỜNG + MULTI-TIMEFRAME ANALYSIS"))
    print(DIM("━" * 100))

    symbols = get_top_coins(top_n)
    print(f"  Phân tích {len(symbols)} coins: {', '.join(symbols[:8])}{'...' if len(symbols) > 8 else ''}")
    print()

    # ── 3. ANALYZE EACH COIN ──
    all_results = []
    start_time = time.time()

    for i, symbol in enumerate(symbols):
        elapsed = time.time() - start_time
        eta = (elapsed / max(i, 1)) * (len(symbols) - i) if i > 0 else 0
        print(f"\r  [{i+1}/{len(symbols)}] {symbol:15s} — {elapsed:.0f}s elapsed — ETA: {eta:.0f}s   ",
              end="", flush=True)

        tf_results = {}
        for tf in timeframes:
            ind_text = mcp_call("analyze_indicators", {"symbol": symbol, "interval": tf, "limit": 200})
            sig_text = mcp_call("get_trading_signals", {"symbol": symbol, "interval": tf})
            reg_text = mcp_call("detect_market_regime", {"symbol": symbol, "interval": tf})
            sr_text = mcp_call("get_support_resistance", {"symbol": symbol, "interval": tf})

            score, details = score_timeframe(ind_text, sig_text, reg_text, sr_text)
            tf_results[tf] = (score, details)

        composite, tf_scores, tf_details = compute_composite_score(tf_results)
        confluence, directions, conf_pct = compute_confluence(tf_scores, tf_details)

        # Get price
        price = 0
        for tf in timeframes:
            p = tf_details.get(tf, {}).get("price", 0)
            if p > 0:
                price = p
                break

        all_results.append({
            "symbol": symbol,
            "price": price,
            "composite_score": composite,
            "tf_scores": tf_scores,
            "tf_details": tf_details,
            "confluence": confluence,
            "directions": directions,
            "conf_pct": conf_pct,
        })

    print()
    total_time = time.time() - start_time
    print(f"  ✅ Hoàn thành {len(symbols)} coins trong {total_time:.1f}s")
    print()

    # Sort by score
    all_results.sort(key=lambda x: x["composite_score"], reverse=True)

    # ── 4. SUMMARY TABLE ──
    print()
    print(DIM("━" * 100))
    print(BOLD("  PHẦN 3: XẾP HẠNG COMPOSITE SCORE"))
    print(DIM("━" * 100))
    print()

    header = f"  {'#':>3}  {'Symbol':15s}  {'Price':>12s}  {'Score':>8s}"
    for tf in timeframes:
        header += f"  {tf:>6s}"
    header += f"  {'Signal':20s}"
    print(header)
    sep = f"  {'─'*3}  {'─'*15}  {'─'*12}  {'─'*8}"
    for tf in timeframes:
        sep += f"  {'─'*6}"
    sep += f"  {'─'*20}"
    print(sep)

    for i, r in enumerate(all_results[:20]):
        line = f"  {i+1:>3}  {r['symbol']:15s}  ${r['price']:>10,.2f}  {fmt_score(r['composite_score']):>14s}"
        for tf in timeframes:
            s = r['tf_scores'].get(tf, 0)
            line += f"  {fmt_score(s):>14s}" if s > 0 else f"  {'—':>6s}"
        line += f"  {fmt_dir(r['confluence']):>20s}"
        print(line)

    # ── 5. DETAILED TOP 5 ──
    print()
    print()
    print(BOLD("═" * 100))
    print(BOLD("  🔥 PHẦN 4: CHI TIẾT TOP 5 GIAO DỊCH TỐT NHẤT"))
    print(BOLD("═" * 100))

    for rank, r in enumerate(all_results[:5]):
        sym = r["symbol"]
        print()
        print(f"  ┌{'─'*98}┐")
        price_str = f"${r['price']:,.2f}" if r['price'] > 0 else "N/A"
        print(f"  │ #{rank+1}  {BOLD(sym):15s}  Score: {fmt_score(r['composite_score'])}/100  "
              f"Signal: {fmt_dir(r['confluence'])}  Price: {price_str}  ")
        print(f"  └{'─'*98}┘")

        for tf in timeframes:
            if tf not in r["tf_details"]:
                continue
            d = r["tf_details"][tf]
            score = r["tf_scores"].get(tf, 0)

            hurst = d.get("hurst", 0)
            regime = d.get("hurst_regime", "?")
            lag_rsi = d.get("laguerre_rsi", 0.5)
            cmo = d.get("cmo", 0)
            rsi = d.get("rsi", 50)
            signals = d.get("signals", [])
            alma = d.get("alma", "")
            ss = d.get("ss", "")
            sr = d.get("sr", {})

            print(f"    │")
            print(f"    ├── [{tf:>3s}] Score: {fmt_score(score)}  │  "
                  f"Hurst: {fmt_hurst(hurst)}  │  Regime: {regime}")

            print(f"    │   RSI: {rsi:.1f}  LaguerreRSI: {lag_rsi:.3f}  "
                  f"CMO: {cmo:.1f}  BB%B: {d.get('bb_pct', 0):.2f}")

            if alma:
                print(f"    │   ALMA: {alma[:50]}")
            if ss:
                print(f"    │   SS: {ss[:50]}")

            # Signals
            buys = [s for s in signals if s["direction"] == "BUY"]
            sells = [s for s in signals if s["direction"] == "SELL"]
            if buys:
                names = ", ".join([f"{s['name']}({s['confidence']}%)" for s in buys])
                print(f"    │   {GREEN('🟢 Buy')}: {names}")
            if sells:
                names = ", ".join([f"{s['name']}({s['confidence']}%)" for s in sells])
                print(f"    │   {RED('🔴 Sell')}: {names}")

            # S/R
            supports = sr.get("supports", [])
            resistances = sr.get("resistances", [])
            if supports:
                print(f"    │   Support: {', '.join([f'${s:,.0f}' for s in supports[:3]])}")
            if resistances:
                print(f"    │   Resistance: {', '.join([f'${s:,.0f}' for s in resistances[:3]])}")

        # Recommendation
        print(f"    │")
        direction = "LONG" if "BUY" in r["confluence"] else "SHORT" if "SELL" in r["confluence"] else "WAIT"

        if direction == "LONG":
            print(f"    ├── {BGREEN('💡 KHUYẾN NGHỊ: LONG')} — Multiple TFs đồng thuận mua ↑")
            if r["price"] > 0:
                sl = r["price"] * 0.97
                tp1 = r["price"] * 1.03
                tp2 = r["price"] * 1.06
                print(f"    │   Entry: ${r['price']:,.2f} | SL: ${sl:,.2f} (-3%) | "
                      f"TP1: ${tp1:,.2f} (+3%) | TP2: ${tp2:,.2f} (+6%)")
        elif direction == "SHORT":
            print(f"    ├── {BRED('💡 KHUYẾN NGHỊ: SHORT')} — Multiple TFs đồng thuận bán ↓")
            if r["price"] > 0:
                sl = r["price"] * 1.03
                tp1 = r["price"] * 0.97
                tp2 = r["price"] * 0.94
                print(f"    │   Entry: ${r['price']:,.2f} | SL: ${sl:,.2f} (+3%) | "
                      f"TP1: ${tp1:,.2f} (-3%) | TP2: ${tp2:,.2f} (-6%)")
        else:
            print(f"    ├── {YELLOW('💡 KHUYẾN NGHỊ: WAIT')} — Tín hiệu lẫn lộn, đợi rõ hơn")
        print(f"    └{'─'*98}┘")

    # ── 6. SUMMARY ──
    print()
    print(BOLD("═" * 100))
    print(BOLD("  📋 TÓM TẮT KẾT QUẢ"))
    print(BOLD("═" * 100))
    print()

    buys = [r for r in all_results if "BUY" in r["confluence"]]
    sells = [r for r in all_results if "SELL" in r["confluence"]]
    mixed = [r for r in all_results if r["confluence"] == "MIXED"]

    print(f"  Tổng phân tích:  {len(all_results)} coins")
    print(f"  {GREEN('Tín hiệu BUY')}:  {len(buys)} coins")
    print(f"  {RED('Tín hiệu SELL')}:  {len(sells)} coins")
    print(f"  {YELLOW('Tín hiệu MIXED')}:  {len(mixed)} coins")
    print()

    if buys:
        print(f"  {BGREEN('🟢 TOP BUY PICKS:')}")
        for r in buys[:5]:
            tf_parts = [f"{tf}:{r['tf_scores'].get(tf, 0):.0f}" for tf in timeframes if tf in r['tf_scores']]
            print(f"    • {r['symbol']:15s}  Score: {r['composite_score']:.1f}  "
                  f"[{' | '.join(tf_parts)}]  {r['confluence']}")
    print()

    if sells:
        print(f"  {BRED('🔴 TOP SHORT PICKS:')}")
        for r in sells[:3]:
            tf_parts = [f"{tf}:{r['tf_scores'].get(tf, 0):.0f}" for tf in timeframes if tf in r['tf_scores']]
            print(f"    • {r['symbol']:15s}  Score: {r['composite_score']:.1f}  "
                  f"[{' | '.join(tf_parts)}]  {r['confluence']}")
    print()

    # Save JSON report
    report_dir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(report_dir, exist_ok=True)
    ts_str = datetime.now().strftime("%Y%m%d_%H%M%S")
    report_file = os.path.join(report_dir, f"mtf_{ts_str}.json")

    json_out = []
    for r in all_results:
        json_out.append({
            "symbol": r["symbol"],
            "price": r["price"],
            "composite_score": round(r["composite_score"], 2),
            "tf_scores": {k: round(v, 2) for k, v in r["tf_scores"].items()},
            "confluence": r["confluence"],
            "directions": r["directions"],
        })

    with open(report_file, "w") as f:
        json.dump({
            "timestamp": datetime.now().isoformat(),
            "timeframes": timeframes,
            "sentiment": {"fear_greed": f"{fg_val}/100 ({fg_label})", "composite": s_val},
            "results": json_out,
        }, f, indent=2)

    print(f"  📁 Report: {report_file}")
    print()
    print(BOLD("=" * 100))
    print()


if __name__ == "__main__":
    main()
