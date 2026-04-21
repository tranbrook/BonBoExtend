#!/usr/bin/env python3
"""BonBoExtend — SHORT SIGNAL SCANNER (Financial-Hacker Enhanced)
Tìm coin có tín hiệu SHORT tốt nhất: overbought + bearish momentum + Hurst trending down
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

# ── Wide scan: top market cap + recently pumped coins + known weak ──
targets = [
    # Majors
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT",
    # DeFi
    "AAVEUSDT", "UNIUSDT", "COMPUSDT", "MKRUSDT", "DYDXUSDT", "LINKUSDT",
    # L1s
    "AVAXUSDT", "ADAUSDT", "DOTUSDT", "NEARUSDT", "APTUSDT", "SUIUSDT", "ARBUSDT",
    # Meme / volatile
    "DOGEUSDT", "ORDIUSDT",
    # Recently pumped (potential short on exhaustion)
    "GUNUSDT", "SPKUSDT", "BOMEUSDT", "HIGHUSDT", "THETAUSDT", "DEXEUSDT",
    "ENAUSDT", "PENDLEUSDT", "SKLUSDT", "ONTUSDT", "WLDUSDT",
    # Extra majors
    "LTCUSDT", "ATOMUSDT", "FILUSDT", "OPUSDT", "MATICUSDT",
]

results = []

for i, symbol in enumerate(targets):
    print(f"[{i+1}/{len(targets)}] {symbol}...", file=sys.stderr, flush=True)

    ind_text = mcp_call("analyze_indicators", {"symbol": symbol, "interval": "1h", "limit": 200})
    sig_text = mcp_call("get_trading_signals", {"symbol": symbol, "interval": "1h"})
    reg_text = mcp_call("detect_market_regime", {"symbol": symbol, "interval": "1h"})

    # ── Parse Traditional ──
    price = 0; rsi = 50; macd_hist = 0; bb_pb = 0.5; sma20 = 0

    for line in ind_text.split("\n"):
        if "Price" in line and "$" in line and price == 0:
            m = re.search(r'\$([\d,.]+)', line)
            if m:
                try: price = float(m.group(1).replace(",",""))
                except: pass
        if "RSI(14)" in line and rsi == 50:
            m = re.search(r'([\d.]+)', line.split("RSI(14)")[-1].split("\n")[0])
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

    # ── Parse Financial-Hacker ──
    hurst = None; alma_diff_pct = 0; ss_slope = 0; cmo = 0; laguerre_rsi = 0.5

    # Hurst
    for pat in [r'Hurst\(100\)\*\*:\s*([\d.]+)', r'Hurst\(100\).*?([\d.]+)', r'Hurst.*?([\d.]+)']:
        m = re.search(pat, ind_text)
        if m:
            try: hurst = float(m.group(1)); break
            except: pass
    if hurst is None:
        m = re.search(r'Hurst\(100\):\s*([\d.]+)', reg_text)
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
    m = re.search(r'CMO.*?([-\d.]+)', ind_text)
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

    # ============================================================
    # SHORT SCORING (inverse of long scoring)
    # Higher score = better short opportunity
    # ============================================================
    score = 50.0

    # RSI overbought → good for short
    if rsi > 75: score += 16
    elif rsi > 70: score += 12
    elif rsi > 60: score += 5
    elif rsi < 30: score -= 16    # oversold = bad for short
    elif rsi < 40: score -= 6

    # MACD bearish → good for short
    if macd_hist < 0: score += 7
    else: score -= 4

    # BB upper band → overbought area
    if bb_pb > 0.95: score += 14
    elif bb_pb > 0.85: score += 8
    elif bb_pb > 0.75: score += 3
    elif bb_pb < 0.15: score -= 12  # lower band = bounce risk
    elif bb_pb < 0.3: score -= 5

    # Sell signals dominate
    score += (sell_signals - buy_signals) * 4

    # Price above SMA20 → potential pullback
    if price > 0 and sma20 > 0:
        pct_above = (price - sma20) / sma20 * 100
        if pct_above > 5: score += 6   # overextended
        elif pct_above > 3: score += 3
        elif pct_above < -3: score -= 5  # already dropped

    # ── Financial-Hacker SHORT scoring ──

    # Hurst regime
    if hurst is not None:
        if hurst > 0.55:
            # Trending → short only if trend is DOWN
            score += 4  # trending market, but direction matters
            if alma_diff_pct < -0.5: score += 8   # ALMA confirms downtrend
            if ss_slope < -0.05: score += 5         # SS confirms downtrend
        elif hurst < 0.45:
            # Mean-reverting → short at resistance
            score += 5
            if bb_pb > 0.8: score += 8   # near upper BB in mean-revert = sell
            if rsi > 65: score += 5       # overbought in mean-revert
        else:
            # Random walk → risky to short
            score -= 4

    # ALMA bearish crossover (stronger than EMA)
    if alma_diff_pct < -0.5: score += 6
    elif alma_diff_pct < -0.2: score += 3
    elif alma_diff_pct > 0.5: score -= 5

    # SuperSmoother bearish slope
    if ss_slope < -0.05: score += 5
    elif ss_slope < -0.02: score += 2
    elif ss_slope > 0.05: score -= 5

    # CMO overbought
    if cmo > 50: score += 8    # extremely overbought → short
    elif cmo > 30: score += 4
    elif cmo < -50: score -= 8  # extremely oversold → bounce risk
    elif cmo < -20: score -= 4

    # LaguerreRSI overbought
    if laguerre_rsi > 0.85: score += 10  # extreme overbought
    elif laguerre_rsi > 0.75: score += 5
    elif laguerre_rsi < 0.2: score -= 10  # oversold → bounce risk
    elif laguerre_rsi < 0.3: score -= 4

    # Regime bonus for short
    if regime == "Trending Down": score += 8
    elif regime == "Volatile": score += 3
    elif regime == "Trending Up": score -= 8  # uptrend = bad for short

    score = max(0, min(100, score))

    if score >= 70: rec = "STRONG_SHORT"
    elif score >= 60: rec = "SHORT"
    elif score >= 45: rec = "NEUTRAL"
    elif score >= 35: rec = "COVER"
    else: rec = "STRONG_COVER"  # don't short, long signal

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
    time.sleep(0.12)

# Sort by short score (highest = best short)
results.sort(key=lambda x: x["score"], reverse=True)

# ═══════════════════════════════════════════════════════════════
# REPORT
# ═══════════════════════════════════════════════════════════════

print()
print("=" * 135)
print("  BONBOEXTEND — TIM TIN HIEU SHORT TOT NHAT HIEN TAI")
print(f"  {time.strftime('%Y-%m-%d %H:%M:%S')} | 1H TF | {len(results)} coins | Short Scoring (Financial-Hacker)")
print("=" * 135)

# Filter: only show candidates with SHORT potential (score >= 45)
short_candidates = [r for r in results if r["score"] >= 50]
avoid_short = [r for r in results if r["score"] < 50]

print()
print(f"  SHORT CANDIDATES: {len(short_candidates)} | AVOID SHORT: {len(avoid_short)}")
print()

# Main table — sorted by short score
hdr = f"{'#':<3} {'Symbol':<12} {'Price':>11} {'RSI':>6} {'MACD':>7} {'BB%B':>5} {'Hurst':>6} {'ALMA%':>6} {'SS%':>7} {'CMO':>6} {'LagRSI':>6} {'Char':<11} {'Regime':<14} {'Score':>6} {'Rec':<14}"
print(hdr)
print("-" * len(hdr))

for i, r in enumerate(results):
    hurst_s = f"{r['hurst']:.2f}" if r["hurst"] is not None else "  -"
    rsi_flag = "OB" if r["rsi"] > 65 else "OS" if r["rsi"] < 35 else "  "
    macd_s = f"{r['macd_hist']:>+.1f}" if abs(r["macd_hist"]) < 1000 else f"{r['macd_hist']:>+.0f}"
    score_bar = "!!" if r["score"] >= 70 else " +" if r["score"] >= 60 else "  " if r["score"] >= 45 else " -"

    print(f"{i+1:<3} {r['symbol']:<12} ${r['price']:>9.4f} {rsi_flag}{r['rsi']:>4.0f} {macd_s:>7} {r['bb_pb']:>5.2f} {hurst_s:>6} {r['alma_diff']:>+5.1f} {r['ss_slope']:>+6.3f} {r['cmo']:>+5.0f} {r['laguerre_rsi']:>5.3f} {r['market_char']:<11} {r['regime']:<14} {score_bar}{r['score']:>4.0f} {r['rec']:<14}")

# ── TOP 3 SHORT SETUPS ──
print()
print("=" * 135)
print("  TOP 3 TIN HIEU SHORT TOT NHAT — CHI TIET")
print("=" * 135)

for i, r in enumerate(results[:3]):
    if r["price"] == 0: continue
    print()
    print(f"  {'='*70}")
    print(f"  #{i+1} {r['symbol']} — Short Score: {r['score']:.0f}/100 — {r['rec']}")
    print(f"  {'='*70}")

    entry = r["price"]
    sl = entry * 1.03        # SL 3% above entry for short
    tp1 = entry * 0.95       # TP1 5% below
    tp2 = entry * 0.90       # TP2 10% below
    rr = (entry - tp1) / (sl - entry) if sl > entry else 0

    print(f"  Price: ${r['price']:.4f}")
    print()

    # Traditional
    print(f"  TRADITIONAL:")
    rsi_label = "OVERBOUGHT" if r["rsi"] > 70 else "APPROACHING OB" if r["rsi"] > 60 else "Neutral" if r["rsi"] > 40 else "OVERSOLD (risk)"
    print(f"    RSI(14): {r['rsi']:.1f} — {rsi_label}")
    macd_label = "Bearish" if r["macd_hist"] < 0 else "Bullish (risk)"
    print(f"    MACD histogram: {r['macd_hist']:+.4f} — {macd_label}")
    bb_label = "UPPER BAND" if r["bb_pb"] > 0.8 else "Near upper" if r["bb_pb"] > 0.6 else "Mid" if r["bb_pb"] > 0.4 else "LOWER BAND (risk)"
    print(f"    BB %B: {r['bb_pb']:.2f} — {bb_label}")
    if r["sma20"] > 0 and r["price"] > 0:
        pct = (r["price"] - r["sma20"]) / r["sma20"] * 100
        print(f"    vs SMA(20): {pct:+.2f}% {'(above = resistance test)' if pct > 0 else '(below = bearish)'}")

    # Financial-Hacker
    print(f"\n  FINANCIAL-HACKER:")
    if r["hurst"] is not None:
        if r["hurst"] > 0.55:
            char_desc = "TRENDING"
            if r["alma_diff"] < 0:
                strat = "Trend-following SHORT confirmed (ALMA bearish)"
            else:
                strat = "Trend-following but ALMA bullish — CAUTION"
        elif r["hurst"] < 0.45:
            char_desc = "MEAN-REVERTING"
            strat = "Short at resistance (BB upper / RSI overbought)"
        else:
            char_desc = "RANDOM WALK"
            strat = "Risky short — no statistical edge"
        print(f"    Hurst(100): {r['hurst']:.3f} → {char_desc}")
        print(f"    → {strat}")
    else:
        print(f"    Hurst: N/A")

    alma_label = "BEARISH cross" if r["alma_diff"] < -0.3 else "bullish (risk)" if r["alma_diff"] > 0.3 else "neutral"
    print(f"    ALMA crossover: {r['alma_diff']:+.2f}% — {alma_label}")
    ss_label = "Bearish slope" if r["ss_slope"] < -0.02 else "Bullish (risk)" if r["ss_slope"] > 0.02 else "Flat"
    print(f"    SuperSmoother: {r['ss_slope']:+.4f}% — {ss_label}")
    cmo_label = "OVERBOUGHT" if r["cmo"] > 40 else "Overbought zone" if r["cmo"] > 20 else "Oversold (risk)" if r["cmo"] < -40 else "Neutral"
    print(f"    CMO(14): {r['cmo']:+.1f} — {cmo_label}")
    lag_label = "OVERBOUGHT" if r["laguerre_rsi"] > 0.8 else "Near OB" if r["laguerre_rsi"] > 0.7 else "OVERSOLD (risk)" if r["laguerre_rsi"] < 0.2 else "Neutral"
    print(f"    LaguerreRSI: {r['laguerre_rsi']:.3f} — {lag_label}")

    # Trade setup
    if r["score"] >= 55:
        action = "SHORT" if r["score"] >= 60 else "WATCH (approaching short signal)"
        print()
        print(f"  THIET LAP LENH SHORT:")
        print(f"    Action:  {action}")
        print(f"    Entry:   ${entry:.4f}")
        print(f"    SL:      ${sl:.4f} (+3.0%)")
        print(f"    TP1:     ${tp1:.4f} (-5.0%)")
        print(f"    TP2:     ${tp2:.4f} (-10.0%)")
        print(f"    R:R      1:{rr:.1f}")

        reasons = []
        if r["rsi"] > 65: reasons.append(f"RSI overbought ({r['rsi']:.0f})")
        if r["laguerre_rsi"] > 0.75: reasons.append(f"LaguerreRSI overbought ({r['laguerre_rsi']:.3f})")
        if r["bb_pb"] > 0.8: reasons.append(f"BB upper band (%B={r['bb_pb']:.2f})")
        if r["cmo"] > 40: reasons.append(f"CMO overbought ({r['cmo']:+.0f})")
        if r["alma_diff"] < -0.3: reasons.append(f"ALMA bearish cross ({r['alma_diff']:+.1f}%)")
        if r["ss_slope"] < -0.02: reasons.append(f"SuperSmoother bearish ({r['ss_slope']:+.3f}%)")
        if r["hurst"] is not None and r["hurst"] > 0.55 and r["alma_diff"] < 0:
            reasons.append(f"Hurst trending ({r['hurst']:.2f}) + ALMA bearish = trend SHORT")
        if r["hurst"] is not None and r["hurst"] < 0.45 and r["bb_pb"] > 0.75:
            reasons.append(f"Hurst mean-revert ({r['hurst']:.2f}) + BB upper = fade SHORT")
        if r["sell_sigs"] > r["buy_sigs"]:
            reasons.append(f"Signals: {r['sell_sigs']} Sell vs {r['buy_sigs']} Buy")
        if r["regime"] == "Trending Down":
            reasons.append("Regime: Trending Down confirmed")
        if r["macd_hist"] < 0:
            reasons.append(f"MACD bearish ({r['macd_hist']:+.4f})")

        risks = []
        if r["rsi"] < 40: risks.append(f"RSI low ({r['rsi']:.0f}) — bounce risk")
        if r["laguerre_rsi"] < 0.3: risks.append(f"LaguerreRSI low ({r['laguerre_rsi']:.3f}) — oversold bounce")
        if r["bb_pb"] < 0.2: risks.append("BB lower band — mean reversion up risk")
        if r["alma_diff"] > 0.3: risks.append("ALMA still bullish — wait for cross down")
        if r["regime"] == "Trending Up": risks.append("Regime is TRENDING UP — fighting the trend!")

        if reasons:
            print(f"\n  LY DO SHORT:")
            for reason in reasons:
                print(f"    ▼ {reason}")

        if risks:
            print(f"\n  RUI RO:")
            for risk in risks:
                print(f"    ⚠ {risk}")

# ── S/R for top 3 short ──
print()
print("=" * 135)
print("  SUPPORT / RESISTANCE — TOP 3 SHORT PICKS")
print("=" * 135)

for r in results[:3]:
    if r["price"] == 0: continue
    sr_text = mcp_call("get_support_resistance", {"symbol": r["symbol"], "interval": "1h", "lookback": 60})
    print(f"\n  {r['symbol']}:")
    for line in sr_text.split("\n"):
        if line.strip():
            print(f"    {line.strip()}")

# ── Sentiment ──
print()
print("=" * 135)
print("  MARKET SENTIMENT")
print("=" * 135)
sent = mcp_call("get_composite_sentiment", {})
fg = mcp_call("get_fear_greed_index", {})
print(f"  {fg.strip()}")
print(f"  {sent.strip()}")

# ── Summary ──
print()
print("=" * 135)
print("  KET LUAN — TIN HIEU SHORT")
print("=" * 135)

strong_shorts = [r for r in results if r["score"] >= 70]
shorts = [r for r in results if 60 <= r["score"] < 70]
neutrals = [r for r in results if 45 <= r["score"] < 60]
covers = [r for r in results if r["score"] < 45]

print(f"\n  STRONG SHORT (>=70): {len(strong_shorts)} coins")
for r in strong_shorts[:5]:
    h = f"H={r['hurst']:.2f}" if r['hurst'] else "H=-"
    print(f"    ▼▼ {r['symbol']:<12} Score:{r['score']:.0f} | {h} | ALMA:{r['alma_diff']:+.1f}% | RSI:{r['rsi']:.0f} | {r['rec']}")

print(f"\n  SHORT (60-69): {len(shorts)} coins")
for r in shorts[:5]:
    h = f"H={r['hurst']:.2f}" if r['hurst'] else "H=-"
    print(f"    ▼ {r['symbol']:<12} Score:{r['score']:.0f} | {h} | ALMA:{r['alma_diff']:+.1f}% | RSI:{r['rsi']:.0f} | {r['rec']}")

print(f"\n  NEUTRAL (45-59): {len(neutrals)} coins")
print(f"  AVOID SHORT (<45): {len(covers)} coins")
for r in covers[:3]:
    h = f"H={r['hurst']:.2f}" if r['hurst'] else "H=-"
    print(f"    ⚡ {r['symbol']:<12} Score:{r['score']:.0f} | {h} | {r['rec']} (NOT good for short)")

avg_short_score = sum(r["score"] for r in results) / len(results) if results else 50
print(f"\n  Average Short Score: {avg_short_score:.0f}/100")

if avg_short_score >= 55:
    print("  → THI TRUONG CO TIN HIEU SHORT — Chon coins score >= 60, Hurst trending + ALMA bearish")
elif avg_short_score >= 45:
    print("  → THI TRUONG TRUNG TINH — Short tin can, chi short coin score >= 65 voi Hurst confirm")
else:
    print("  → THI TRUONG KHONG THUAN CHO SHORT — Dominant bullish, tranh short tru khi co tin hieu manh")

print()
print("  DISCLAIMER: Phan tich tu dong bang AI + Financial-Hacker methodology.")
print("  SHORT co rui ro VO HAN — Luon dat stop loss nghiem ngat!")
print("  KHONG phai loi khuyen dau tu. Luon DYOR va quan ly rui ro.")
print("=" * 135)
