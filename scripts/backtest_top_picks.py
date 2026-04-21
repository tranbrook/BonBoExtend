#!/usr/bin/env python3
"""BonBoExtend — Backtest confirmation for top picks"""
import urllib.request, json, time, re

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

# Backtest top picks
top_picks = ["AAVEUSDT", "BTCUSDT", "ETHUSDT", "DOGEUSDT", "SOLUSDT", "TAOUSDT"]
strategies = ["sma_crossover", "rsi_reversal", "bollinger_bands", "macd_crossover"]

print("=" * 95)
print("  BACKTEST CONFIRMATION - TOP PICKS vs 4 STRATEGIES")
print("=" * 95)

bt_results = []

for symbol in top_picks:
    print(f"\n  {symbol}", end="")
    sym_bests = {"symbol": symbol}

    for strat in strategies:
        result = mcp_call("run_backtest", {"symbol": symbol, "interval": "1h", "strategy": strat, "period": 30})

        ret = 0; wr = 0; sharpe = 0; trades = 0
        for line in result.split("\n"):
            if "Total Return" in line:
                m = re.search(r"([-\d.]+)%", line)
                if m: ret = float(m.group(1))
            if "Win Rate" in line:
                m = re.search(r"([\d.]+)%", line)
                if m: wr = float(m.group(1))
            if "Sharpe" in line:
                m = re.search(r"([-\d.]+)", line)
                if m: sharpe = float(m.group(1))
            if "Total Trades" in line:
                m = re.search(r"(\d+)", line)
                if m: trades = int(m.group(1))

        strat_name = strat.replace("_", " ").title()
        emoji = "Y" if ret > 0 else "N"
        print(f"\n     [{emoji}] {strat_name:<20} Return: {ret:>+7.1f}% | WR: {wr:>5.1f}% | Sharpe: {sharpe:>5.2f} | Trades: {trades}", end="")

        if ret > sym_bests.get("best_ret", -999):
            sym_bests = {
                "symbol": symbol,
                "best_strat": strat_name,
                "best_ret": ret,
                "best_wr": wr,
                "best_sharpe": sharpe,
                "best_trades": trades,
            }

    bt_results.append(sym_bests)
    print()
    time.sleep(0.3)

# Summary table
print()
print("=" * 95)
print("  BEST STRATEGY PER COIN (sorted by return)")
print("=" * 95)

bt_results.sort(key=lambda x: x.get("best_ret", -999), reverse=True)
for r in bt_results:
    if "best_strat" not in r: continue
    emoji = "[+]" if r["best_ret"] > 0 else "[-]"
    print(f"  {emoji} {r['symbol']:<12} {r['best_strat']:<20} {r['best_ret']:>+7.1f}%  WR:{r['best_wr']:>5.1f}%  Sharpe:{r['best_sharpe']:>5.2f}  Trades:{r['best_trades']:>4}")

# Get price + S/R for top 3
print()
print("=" * 95)
print("  CHI TIET TOP 3 PICKS")
print("=" * 95)

for r in bt_results[:3]:
    if "best_strat" not in r: continue
    sym = r["symbol"]

    price_data = mcp_call("get_crypto_price", {"symbol": sym})
    sr_data = mcp_call("get_support_resistance", {"symbol": sym, "interval": "1h"})

    print(f"\n  {sym}")
    print(f"  {price_data.strip()}")
    print(f"  {sr_data.strip()}")

    price = 0
    # Try multiple patterns
    m = re.search(r'\$([\d,.]+)', price_data)
    if m:
        price = float(m.group(1).replace(",",""))
    else:
        m = re.search(r'Price.*?([\d,.]+)', price_data)
        if m:
            price = float(m.group(1).replace(",",""))

    if price > 0:
        sl = price * 0.97
        tp1 = price * 1.05
        tp2 = price * 1.10
        rr = (tp1 - price) / (price - sl) if price > sl else 0
        print(f"  Best strat: {r['best_strat']} (Ret: {r['best_ret']:+.1f}%, WR: {r['best_wr']:.0f}%)")
        print(f"  Entry: ${price:.4f} | SL: ${sl:.4f} | TP1: ${tp1:.4f} | TP2: ${tp2:.4f} | R:R = 1:{rr:.1f}")

print()
print("=" * 95)
print("  DISCLAIMER: Day la phan tich tu dong. KHONG phai loi khuyen dau tu.")
print("=" * 95)
