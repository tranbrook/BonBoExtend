#!/usr/bin/env python3
"""BonBo Top 100 Crypto Deep Analysis — Find Best Trading Opportunities"""

import urllib.request, json, sys, time

MCP_URL = "http://localhost:9876/mcp"

def mcp_call(tool, args=None):
    if args is None: args = {}
    payload = json.dumps({"jsonrpc":"2.0","method":"tools/call","params":{"name":tool,"arguments":args},"id":"1"}).encode()
    req = urllib.request.Request(MCP_URL, data=payload, headers={"Content-Type":"application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read().decode())
            return data.get("result",{}).get("content",[{}])[0].get("text","")
    except Exception as e:
        return f"Error: {e}"

# Top coins to analyze deeply (mix of gainers + majors + oversold)
targets = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT",
    "ENAUSDT",   # +8.53% DeFi
    "PENDLEUSDT", # +8.09% DeFi yield
    "THETAUSDT", # +19.80% momentum
    "DEXEUSDT",  # +22.08% strong momentum
    "COMPUSDT",  # +4.19% DeFi bluechip
    "DYDXUSDT",  # +6.34% DeFi exchange
    "SKLUSDT",   # +7.84% L2
    "ONTUSDT",   # +10.67% breakout
    "ORDIUSDT",  # -24.39% oversold BRC20
    "WLDUSDT",   # -11.49% oversold AI
    "AVAXUSDT",  # -2.08% L1
    "APTUSDT",   # -4.77% L1 oversold
    "LINKUSDT",  # Oracle
    "NEARUSDT",  # AI L1
    "AAVEUSDT",  # DeFi bluechip
]

results = []

for symbol in targets:
    print(f"Analyzing {symbol}...", file=sys.stderr)
    
    # Get indicators
    ind_text = mcp_call("analyze_indicators", {"symbol": symbol, "interval": "1h", "limit": 100})
    
    # Get signals
    sig_text = mcp_call("get_trading_signals", {"symbol": symbol, "interval": "1h"})
    
    # Get regime
    reg_text = mcp_call("detect_market_regime", {"symbol": symbol, "interval": "1h"})
    
    # Get backtest
    bt_text = mcp_call("run_backtest", {"symbol": symbol, "interval": "1h", "strategy": "sma_crossover", "period": 30})
    
    # Parse indicators
    price = 0; rsi = 50; macd_hist = 0; bb_b = 0.5
    sma20 = 0
    
    for line in ind_text.split("\n"):
        if "Price" in line and "$" in line:
            try: price = float(line.split("$")[1].strip().replace(",",""))
            except: pass
        if "RSI(14)" in line:
            try: rsi = float(line.split("RSI(14)")[1].split(" ")[0].replace(":","").strip())
            except: pass
        if "hist=" in line:
            try: macd_hist = float(line.split("hist=")[1].split(" ")[0])
            except: pass
        if "%B=" in line:
            try: bb_b = float(line.split("%B=")[1].split(" ")[0])
            except: pass
        if "SMA(20)" in line:
            try: sma20 = float(line.split("$")[1].strip().replace(",",""))
            except: pass
    
    # Parse signals
    buy_signals = sig_text.count("\U0001f7e2")
    sell_signals = sig_text.count("\U0001f534")
    
    # Parse regime
    regime = "Unknown"
    for r in ["Trending Up", "Trending Down", "Ranging", "Volatile", "Quiet"]:
        if r.lower() in reg_text.lower():
            regime = r
            break
    
    # Parse backtest
    bt_return = 0; bt_winrate = 0; bt_sharpe = 0
    for line in bt_text.split("\n"):
        if "Total Return" in line:
            try: bt_return = float(line.split("Return")[1].replace(":","").replace("%","").strip())
            except: pass
        if "Win Rate" in line:
            try: bt_winrate = float(line.split("Win Rate")[1].replace(":","").replace("%","").strip())
            except: pass
        if "Sharpe Ratio" in line:
            try: bt_sharpe = float(line.split("Sharpe Ratio")[1].replace(":","").strip())
            except: pass
    
    # Compute composite score
    score = 50  # base
    
    # RSI scoring
    if rsi < 30: score += 15
    elif rsi < 40: score += 8
    elif rsi > 70: score -= 15
    elif rsi > 60: score -= 5
    
    # MACD
    if macd_hist > 0: score += 8
    else: score -= 8
    
    # BB position
    if bb_b < 0.05: score += 12
    elif bb_b < 0.2: score += 6
    elif bb_b > 0.95: score -= 12
    elif bb_b > 0.8: score -= 6
    
    # Signal balance
    if buy_signals > sell_signals: score += buy_signals * 4
    elif sell_signals > buy_signals: score -= sell_signals * 4
    
    # Regime bonus
    if "Trending Up" in regime: score += 5
    elif "Trending Down" in regime: score -= 5
    elif "Volatile" in regime: score -= 8
    
    # Backtest bonus
    if bt_return > 3: score += 10
    elif bt_return > 0: score += 5
    elif bt_return < -3: score -= 10
    elif bt_return < 0: score -= 5
    
    # Price vs SMA20
    if price > 0 and sma20 > 0:
        if price > sma20: score += 3
        else: score -= 3
    
    score = max(0, min(100, score))
    
    # Recommendation
    if score >= 70: rec = "STRONG_BUY"
    elif score >= 60: rec = "BUY"
    elif score >= 45: rec = "HOLD"
    elif score >= 35: rec = "SELL"
    else: rec = "STRONG_SELL"
    
    results.append({
        "symbol": symbol,
        "price": price,
        "rsi": rsi,
        "macd_hist": macd_hist,
        "bb_b": bb_b,
        "buy_signals": buy_signals,
        "sell_signals": sell_signals,
        "regime": regime,
        "bt_return": bt_return,
        "bt_winrate": bt_winrate,
        "bt_sharpe": bt_sharpe,
        "score": score,
        "rec": rec,
    })
    
    time.sleep(0.3)

# Sort by score
results.sort(key=lambda x: x["score"], reverse=True)

print()
print("=" * 95)
print("  BONBO TOP 100 CRYPTO ANALYSIS -- BEST OPPORTUNITIES NOW")
print("=" * 95)

print()
print(f"{'#':<3} {'Symbol':<14} {'Price':>10} {'RSI':>6} {'MACD':>8} {'BB%B':>6} {'Buy':>4} {'Sell':>4} {'Regime':<14} {'BT_Ret':>7} {'Score':>6} {'Rec':<12}")
print("-" * 115)

for i, r in enumerate(results):
    macd_e = "+" if r["macd_hist"] > 0 else "-"
    rsi_e = "OS" if r["rsi"] < 35 else "OB" if r["rsi"] > 65 else "  "
    score_e = "**" if r["score"] >= 60 else "  " if r["score"] >= 40 else "!!"
    
    print(f"{i+1:<3} {r['symbol']:<14} ${r['price']:>8.4f} {rsi_e}{r['rsi']:>4.1f} {macd_e}{r['macd_hist']:>+7.1f} {r['bb_b']:>5.2f} {r['buy_signals']:>3}  {r['sell_signals']:>3}  {r['regime']:<14} {r['bt_return']:>+6.1f}% {score_e}{r['score']:>4.0f} {r['rec']:<12}")

# Top 3 detailed analysis
print()
print("=" * 95)
print("  TOP 3 CO HOI GIAO DICH TOT NHAT")
print("=" * 95)

for i, r in enumerate(results[:3]):
    print()
    print("=" * 60)
    print(f"  #{i+1} {r['symbol']} -- Score: {r['score']}/100 -- {r['rec']}")
    print("=" * 60)
    
    sl = r['price'] * 0.97
    tp1 = r['price'] * 1.05
    tp2 = r['price'] * 1.10
    rr = (tp1 - r['price']) / (r['price'] - sl) if r['price'] > sl else 0
    
    print(f"  Price: ${r['price']:.4f}")
    print(f"  RSI: {r['rsi']:.1f} | MACD Hist: {r['macd_hist']:+.1f} | BB%B: {r['bb_b']:.2f}")
    print(f"  Signals: {r['buy_signals']} Buy / {r['sell_signals']} Sell")
    print(f"  Regime: {r['regime']}")
    print(f"  Backtest: {r['bt_return']:+.1f}% return | Sharpe: {r['bt_sharpe']:.2f} | Win Rate: {r['bt_winrate']:.0f}%")
    
    if r['score'] >= 55:
        action = "LONG" if r['score'] >= 60 else "WATCH"
        print()
        print(f"  >> {action} ENTRY: ${r['price']:.4f}")
        print(f"  >> Stop Loss:  ${sl:.4f} (-3.0%)")
        print(f"  >> Target 1:  ${tp1:.4f} (+5.0%)")
        print(f"  >> Target 2:  ${tp2:.4f} (+10.0%)")
        print(f"  >> Risk:Reward = 1:{rr:.1f}")
        
        notes = []
        if r['rsi'] < 35:
            notes.append(f"RSI oversold ({r['rsi']:.1f}) -- potential reversal bounce")
        if r['bb_b'] < 0.05:
            notes.append(f"BB lower band touch (%B={r['bb_b']:.2f}) -- mean reversion setup")
        if r['buy_signals'] > r['sell_signals']:
            notes.append(f"More buy signals ({r['buy_signals']}) than sell ({r['sell_signals']})")
        if r['bt_return'] > 0:
            notes.append(f"Backtest confirms profitability (+{r['bt_return']:.1f}%)")
        for note in notes:
            print(f"  >> {note}")
    elif r['score'] < 40:
        print()
        print(f"  >> AVOID/SHORT -- Bearish signals dominant")

# Market summary
print()
print("=" * 95)
print("  MARKET SUMMARY")
print("=" * 95)
buys = sum(1 for r in results if r['score'] >= 60)
holds = sum(1 for r in results if 40 <= r['score'] < 60)
sells = sum(1 for r in results if r['score'] < 40)
avg_score = sum(r['score'] for r in results) / len(results) if results else 0
avg_rsi = sum(r['rsi'] for r in results) / len(results) if results else 50

print(f"  Buy signals:     {buys} coins")
print(f"  Hold:            {holds} coins")
print(f"  Sell signals:    {sells} coins")
print(f"  Average Score:   {avg_score:.1f}/100")
print(f"  Average RSI:     {avg_rsi:.1f}")
print(f"  Fear & Greed:    26/100 (Extreme Fear)")
print()
print(f"  THI TRUONG DANG TRANG THAI FEAR -- Co hoi mua o vung ho tro cho")
print(f"  cac coin co fundamentals tot. Cho xac nhan RSI divergence + BB bounce.")
print("=" * 95)
