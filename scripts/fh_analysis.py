#!/usr/bin/env python3
"""BonBoExtend — Full Market Analysis with Financial-Hacker Indicators
Uses Hurst, ALMA, SuperSmoother, CMO, LaguerreRSI for regime-aware scoring.

NOTE: For comprehensive top-100 analysis with async, DMA scoring, multi-TF,
      and automated multi-strategy backtesting, use analyze_top100.py instead.
      This script is kept for quick single-coin or targeted-list analysis.

See: scripts/analyze_top100.py --top-n 100 --intervals 1h,4h --output all
"""
import urllib.request, json, time, re, sys

MCP_URL = "http://localhost:9876/mcp"

def mcp_call(tool, args=None, timeout=30):
    if args is None: args = {}
    payload = json.dumps({"jsonrpc":"2.0","method":"tools/call","params":{"name":tool,"arguments":args},"id":"1"}).encode()
    req = urllib.request.Request(MCP_URL, data=payload, headers={"Content-Type":"application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode())
            return data.get("result",{}).get("content",[{}])[0].get("text","")
    except Exception as e:
        return f"Error: {e}"

# ── Targets: mix of majors, DeFi, gainers, oversold ──
targets = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT",
    "AAVEUSDT", "DOGEUSDT", "LINKUSDT", "AVAXUSDT",
    "ARBUSDT", "XRPUSDT", "ADAUSDT", "DOTUSDT",
    "ORDIUSDT", "SUIUSDT", "NEARUSDT", "APTUSDT",
    "DYDXUSDT", "COMPUSDT", "UNIUSDT", "MKRUSDT",
]

results = []

for i, symbol in enumerate(targets):
    print(f"[{i+1}/{len(targets)}] {symbol}...", file=sys.stderr, flush=True)

    # Fetch 200 candles for Hurst warmup
    ind_text = mcp_call("analyze_indicators", {"symbol": symbol, "interval": "1h", "limit": 200})
    sig_text = mcp_call("get_trading_signals", {"symbol": symbol, "interval": "1h"})
    reg_text = mcp_call("detect_market_regime", {"symbol": symbol, "interval": "1h"})

    # ── Parse Traditional Indicators ──
    price = 0; rsi = 50; macd_hist = 0; bb_pb = 0.5
    sma20 = 0; ema12 = 0; ema26 = 0

    for line in ind_text.split("\n"):
        if "Price" in line and "$" in line and price == 0:
            m = re.search(r'\$([\d,.]+)', line)
            if m:
                try: price = float(m.group(1).replace(",",""))
                except: pass
        if "RSI(14)" in line:
            m = re.search(r'RSI\(14\)\*\*:\s*([\d.]+)', line)
            if not m: m = re.search(r'([\d.]+)', line.split("RSI(14)")[-1])
            if m:
                try: rsi = float(m.group(1))
                except: pass
        if "hist=" in line:
            m = re.search(r'hist=([-\d.]+)', line)
            if m:
                try: macd_hist = float(m.group(1))
                except: pass
        if "%B=" in line:
            m = re.search(r'%B=([\d.]+)', line)
            if m:
                try: bb_pb = float(m.group(1))
                except: pass
        if "SMA(20)" in line and sma20 == 0:
            m = re.search(r'\$([\d,.]+)', line)
            if m:
                try: sma20 = float(m.group(1).replace(",",""))
                except: pass
        if "EMA(12)" in line and ema12 == 0:
            m = re.search(r'\$([\d,.]+)', line)
            if m:
                try: ema12 = float(m.group(1).replace(",",""))
                except: pass
        if "EMA(26)" in line and ema26 == 0:
            m = re.search(r'\$([\d,.]+)', line)
            if m:
                try: ema26 = float(m.group(1).replace(",",""))
                except: pass

    # ── Parse Financial-Hacker Indicators ──
    hurst = None; alma_diff_pct = 0; ss_slope = 0; cmo = 0; laguerre_rsi = 0.5

    # Hurst
    m = re.search(r'Hurst\(100\)\*\*:\s*([\d.]+)', ind_text)
    if not m: m = re.search(r'Hurst.*?([\d.]+)', ind_text)
    if m:
        try: hurst = float(m.group(1))
        except: pass

    # ALMA diff
    m = re.search(r'ALMA.*?([+-][\d.]+)%', ind_text)
    if m:
        try: alma_diff_pct = float(m.group(1))
        except: pass

    # SuperSmoother slope
    m = re.search(r'SuperSmoother.*?slope:\s*([+-][\d.]+)', ind_text)
    if m:
        try: ss_slope = float(m.group(1))
        except: pass

    # CMO
    m = re.search(r'CMO\(14\)\*\*:\s*([-\d.]+)', ind_text)
    if not m: m = re.search(r'CMO.*?([-\d.]+)', ind_text)
    if m:
        try: cmo = float(m.group(1))
        except: pass

    # LaguerreRSI
    m = re.search(r'LaguerreRSI.*?([\d.]+)', ind_text)
    if m:
        try: laguerre_rsi = float(m.group(1))
        except: pass

    # ── Parse Signals ──
    buy_signals = sig_text.count("\U0001f7e2")
    sell_signals = sig_text.count("\U0001f534")
    market_char = "Unknown"
    if "TRENDING" in sig_text.upper():
        market_char = "Trending"
    elif "MEAN-REVERTING" in sig_text.upper():
        market_char = "Mean-Revert"
    elif "RANDOM WALK" in sig_text.upper():
        market_char = "RandomWalk"

    # ── Parse Regime ──
    regime = "Unknown"
    for r in ["Trending Up", "Trending Down", "Ranging", "Volatile", "Quiet"]:
        if r.lower() in reg_text.lower():
            regime = r; break

    # Parse regime Hurst + strategy
    regime_hurst = None
    m = re.search(r'Hurst\(100\):\s*([\d.]+)', reg_text)
    if m:
        try: regime_hurst = float(m.group(1))
        except: pass
    if hurst is None and regime_hurst is not None:
        hurst = regime_hurst

    # ── Scoring (Financial-Hacker methodology) ──
    score = 50.0

    # RSI scoring
    if rsi < 25: score += 16
    elif rsi < 30: score += 12
    elif rsi < 40: score += 6
    elif rsi > 75: score -= 16
    elif rsi > 70: score -= 12
    elif rsi > 60: score -= 4

    # MACD histogram
    if macd_hist > 0: score += 6
    else: score -= 4

    # Bollinger Bands position
    if bb_pb < 0.05: score += 12
    elif bb_pb < 0.15: score += 7
    elif bb_pb > 0.95: score -= 12
    elif bb_pb > 0.85: score -= 7

    # Signal balance
    score += (buy_signals - sell_signals) * 3

    # ── Financial-Hacker scoring ──

    # Hurst regime bonus (most important FH factor)
    if hurst is not None:
        if hurst > 0.55:
            # Trending → reward trend-aligned signals
            score += 8
            if macd_hist > 0: score += 5  # momentum aligned
            if alma_diff_pct > 0: score += 5  # ALMA confirms trend
        elif hurst < 0.45:
            # Mean-reverting → reward oversold bounces
            score += 6
            if bb_pb < 0.2: score += 8  # BB lower in mean-revert = strong buy
            if rsi < 35: score += 6  # RSI oversold in mean-revert
        else:
            # Random walk → penalize (no edge)
            score -= 6

    # ALMA crossover (better than EMA crossover)
    if alma_diff_pct > 0.5: score += 6
    elif alma_diff_pct < -0.5: score -= 6

    # SuperSmoother slope
    if ss_slope > 0.05: score += 4
    elif ss_slope < -0.05: score -= 4

    # CMO momentum
    if cmo < -50: score += 8   # extremely oversold
    elif cmo < -20: score += 4
    elif cmo > 50: score -= 8
    elif cmo > 20: score -= 4

    # LaguerreRSI (adaptive)
    if laguerre_rsi < 0.2: score += 8
    elif laguerre_rsi < 0.3: score += 4
    elif laguerre_rsi > 0.8: score -= 8
    elif laguerre_rsi > 0.7: score -= 4

    # Price vs SMA20
    if price > 0 and sma20 > 0:
        pct = (price - sma20) / sma20 * 100
        if pct < -3: score += 3  # oversold vs SMA
        elif pct > 3: score -= 3

    score = max(0, min(100, score))

    if score >= 70: rec = "STRONG_BUY"
    elif score >= 60: rec = "BUY"
    elif score >= 45: rec = "HOLD"
    elif score >= 35: rec = "SELL"
    else: rec = "STRONG_SELL"

    results.append({
        "symbol": symbol, "price": price,
        "rsi": rsi, "macd_hist": macd_hist, "bb_pb": bb_pb,
        "hurst": hurst, "alma_diff": alma_diff_pct,
        "ss_slope": ss_slope, "cmo": cmo, "laguerre_rsi": laguerre_rsi,
        "buy_sigs": buy_signals, "sell_sigs": sell_signals,
        "market_char": market_char, "regime": regime,
        "sma20": sma20,
        "score": score, "rec": rec,
    })
    time.sleep(0.15)

# Sort by score
results.sort(key=lambda x: x["score"], reverse=True)

# ═══════════════════════════════════════════════════════════════════
# REPORT
# ═══════════════════════════════════════════════════════════════════

print()
print("=" * 130)
print("  BONBOEXTEND — PHAN TICH GIAO DICH TOI NHAT HIEN TAI (Financial-Hacker Enhanced)")
print(f"  {time.strftime('%Y-%m-%d %H:%M:%S')} | 1H TF | {len(results)} coins | Hurst + ALMA + SuperSmoother + CMO + LaguerreRSI")
print("=" * 130)

# Market overview first
print()
buys = sum(1 for r in results if r["score"] >= 60)
holds = sum(1 for r in results if 40 <= r["score"] < 60)
sells = sum(1 for r in results if r["score"] < 40)
avg_score = sum(r["score"] for r in results) / len(results) if results else 0
avg_hurst = sum(r["hurst"] for r in results if r["hurst"] is not None) / max(1, sum(1 for r in results if r["hurst"] is not None))
trending_count = sum(1 for r in results if r["market_char"] == "Trending")
mr_count = sum(1 for r in results if r["market_char"] == "Mean-Revert")
rw_count = sum(1 for r in results if r["market_char"] == "RandomWalk")

print(f"  TONG QUAN: {buys} BUY | {holds} HOLD | {sells} SELL | Avg Score: {avg_score:.0f}/100")
print(f"  HURST REGIME: {trending_count} Trending | {mr_count} Mean-Revert | {rw_count} RandomWalk | Avg H={avg_hurst:.2f}")
print()

# Main table
hdr = f"{'#':<3} {'Symbol':<12} {'Price':>11} {'RSI':>6} {'MACD':>7} {'BB%B':>5} {'Hurst':>6} {'ALMA%':>6} {'SS%':>7} {'CMO':>6} {'LagRSI':>6} {'Char':<11} {'Score':>6} {'Rec':<12}"
print(hdr)
print("-" * len(hdr))

for i, r in enumerate(results):
    hurst_s = f"{r['hurst']:.2f}" if r["hurst"] is not None else "  -"
    rsi_flag = "OS" if r["rsi"] < 30 else "OB" if r["rsi"] > 70 else "  "
    macd_s = f"{r['macd_hist']:>+.1f}" if abs(r["macd_hist"]) < 1000 else f"{r['macd_hist']:>+.0f}"
    score_bar = "+" if r["score"] >= 60 else "-" if r["score"] < 40 else " "

    print(f"{i+1:<3} {r['symbol']:<12} ${r['price']:>9.4f} {rsi_flag}{r['rsi']:>4.0f} {macd_s:>7} {r['bb_pb']:>5.2f} {hurst_s:>6} {r['alma_diff']:>+5.1f} {r['ss_slope']:>+6.3f} {r['cmo']:>+5.0f} {r['laguerre_rsi']:>5.3f} {r['market_char']:<11} {score_bar}{r['score']:>4.0f} {r['rec']:<12}")

# ── TOP 3 DETAILED ──
print()
print("=" * 130)
print("  TOP 3 CO HOI GIAO DICH TOT NHAT (Financial-Hacker Analysis)")
print("=" * 130)

for i, r in enumerate(results[:3]):
    if r["price"] == 0: continue

    print()
    print(f"  {'='*70}")
    print(f"  #{i+1} {r['symbol']} — Score: {r['score']:.0f}/100 — {r['rec']}")
    print(f"  {'='*70}")

    sl = r["price"] * 0.97
    tp1 = r["price"] * 1.05
    tp2 = r["price"] * 1.10
    rr = (tp1 - r["price"]) / (r["price"] - sl) if r["price"] > sl else 0

    print(f"  Price: ${r['price']:.4f}")
    print()

    # Traditional
    print(f"  TRADITIONAL:")
    print(f"    RSI(14): {r['rsi']:.1f} {'OVERSOLD' if r['rsi'] < 30 else 'OVERBOUGHT' if r['rsi'] > 70 else 'Neutral'}")
    print(f"    MACD histogram: {r['macd_hist']:+.4f} {'Bullish' if r['macd_hist'] > 0 else 'Bearish'}")
    print(f"    BB %B: {r['bb_pb']:.2f} {'Near lower band' if r['bb_pb'] < 0.2 else 'Near upper band' if r['bb_pb'] > 0.8 else 'Mid-range'}")

    # Financial-Hacker
    print(f"\n  FINANCIAL-HACKER:")
    if r["hurst"] is not None:
        char_desc = "TRENDING" if r["hurst"] > 0.55 else "MEAN-REVERTING" if r["hurst"] < 0.45 else "RANDOM WALK"
        print(f"    Hurst(100): {r['hurst']:.3f} → {char_desc}")
        if r["hurst"] > 0.55:
            print(f"    → Strategy: Trend-following (ALMA crossover, SuperSmoother slope)")
        elif r["hurst"] < 0.45:
            print(f"    → Strategy: Mean-reversion (BB bounce, RSI/LaguerreRSI extreme)")
        else:
            print(f"    → Strategy: CAUTION — no statistical edge detected")
    else:
        print(f"    Hurst(100): N/A")

    print(f"    ALMA crossover: {r['alma_diff']:+.2f}% {'Bullish' if r['alma_diff'] > 0 else 'Bearish'}")
    print(f"    SuperSmoother slope: {r['ss_slope']:+.4f}%")
    print(f"    CMO(14): {r['cmo']:+.1f} {'Oversold' if r['cmo'] < -50 else 'Overbought' if r['cmo'] > 50 else 'Neutral'}")
    print(f"    LaguerreRSI(0.8): {r['laguerre_rsi']:.3f} {'OVERSOLD' if r['laguerre_rsi'] < 0.2 else 'OVERBOUGHT' if r['laguerre_rsi'] > 0.8 else 'Neutral'}")

    # Trade setup
    if r["score"] >= 55:
        action = "LONG" if r["score"] >= 60 else "WATCH"
        print()
        print(f"  THIET LAP LENH:")
        print(f"    Action: {action}")
        print(f"    Entry:  ${r['price']:.4f}")
        print(f"    SL:     ${sl:.4f} (-3.0%)")
        print(f"    TP1:    ${tp1:.4f} (+5.0%)")
        print(f"    TP2:    ${tp2:.4f} (+10.0%)")
        print(f"    R:R     1:{rr:.1f}")

        reasons = []
        if r["rsi"] < 30: reasons.append(f"RSI oversold ({r['rsi']:.0f})")
        if r["laguerre_rsi"] < 0.2: reasons.append(f"LaguerreRSI oversold ({r['laguerre_rsi']:.3f})")
        if r["bb_pb"] < 0.15: reasons.append(f"BB lower band (%B={r['bb_pb']:.2f})")
        if r["cmo"] < -50: reasons.append(f"CMO extreme oversold ({r['cmo']:+.0f})")
        if r["hurst"] is not None and r["hurst"] > 0.55 and r["alma_diff"] > 0:
            reasons.append(f"Hurst trending ({r['hurst']:.2f}) + ALMA bullish")
        if r["hurst"] is not None and r["hurst"] < 0.45 and r["rsi"] < 35:
            reasons.append(f"Hurst mean-revert ({r['hurst']:.2f}) + RSI low = bounce likely")
        if r["buy_sigs"] > r["sell_sigs"]:
            reasons.append(f"Signals: {r['buy_sigs']} Buy vs {r['sell_sigs']} Sell")
        if r["alma_diff"] > 0.5:
            reasons.append(f"ALMA bullish cross ({r['alma_diff']:+.1f}%)")
        if r["ss_slope"] > 0:
            reasons.append(f"SuperSmoother uptrend ({r['ss_slope']:+.3f}%)")

        if reasons:
            print(f"\n  LY DO:")
            for reason in reasons:
                print(f"    + {reason}")
    elif r["score"] < 40:
        print(f"\n  AVOID — Bearish signals dominant")

# ── S/R for top 3 ──
print()
print("=" * 130)
print("  SUPPORT / RESISTANCE — TOP 3 PICKS")
print("=" * 130)

for r in results[:3]:
    if r["price"] == 0: continue
    sr_text = mcp_call("get_support_resistance", {"symbol": r["symbol"], "interval": "1h", "lookback": 60})
    print(f"\n  {r['symbol']}:")
    for line in sr_text.split("\n"):
        if line.strip():
            print(f"    {line.strip()}")

# ── Backtest top 5 ──
print()
print("=" * 130)
print("  BACKTEST CONFIRMATION — TOP 5 vs SMA Crossover (1H, 30 days)")
print("=" * 130)

for r in results[:5]:
    bt_text = mcp_call("run_backtest", {"symbol": r["symbol"], "interval": "1h", "strategy": "sma_crossover", "period": 30})
    ret = 0; wr = 0; sharpe = 0
    for line in bt_text.split("\n"):
        if "Total Return" in line:
            m = re.search(r"([-\d.]+)%", line)
            if m: ret = float(m.group(1))
        if "Win Rate" in line:
            m = re.search(r"([\d.]+)%", line)
            if m: wr = float(m.group(1))
        if "Sharpe" in line:
            m = re.search(r"([-\d.]+)", line)
            if m: sharpe = float(m.group(1))
    emoji = "[+]" if ret > 0 else "[-]"
    print(f"  {emoji} {r['symbol']:<12} Return: {ret:>+7.1f}% | WinRate: {wr:>5.1f}% | Sharpe: {sharpe:>5.2f}")

# ── Sentiment ──
print()
print("=" * 130)
print("  MARKET SENTIMENT")
print("=" * 130)
sent = mcp_call("get_composite_sentiment", {})
fg = mcp_call("get_fear_greed_index", {})
print(f"  {fg.strip()}")
print(f"  {sent.strip()}")

# Final verdict
print()
print("=" * 130)
print("  KET LUAN")
print("=" * 130)
if avg_score >= 55:
    print("  THI TRUONG TICH CUC — Co hoi LONG o cac coin score >= 60")
elif avg_score >= 45:
    print("  THI TRUONG TRUNG TINH — Cho xac nhan, chi trade coin score >= 60")
else:
    print("  THI TRUONG TIEU CUC — Bao ve von, tranh LONG tru khi co tin hieu manh")

if avg_hurst > 0.55:
    print(f"  Hurst trung binh {avg_hurst:.2f} > 0.55 → Thien huong TREND-FOLLOWING")
elif avg_hurst < 0.45:
    print(f"  Hurst trung binh {avg_hurst:.2f} < 0.45 → Thien huong MEAN-REVERSION")
else:
    print(f"  Hurst trung binh {avg_hurst:.2f} ≈ 0.5 → CAN THAN, random walk")

print()
print("  DISCLAIMER: Phan tich tu dong bang AI + Financial-Hacker methodology.")
print("  KHONG phai loi khuyen dau tu. Luon DYOR va quan ly rui ro.")
print("=" * 130)
