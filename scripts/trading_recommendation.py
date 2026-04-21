#!/usr/bin/env python3
"""Final trading recommendation from BonBo analysis."""
import json, sys, urllib.request

REPORT = sys.argv[1] if len(sys.argv) > 1 else "/home/tranbrook/.bonbo/reports/top100_20260421_185450.json"
with open(REPORT) as f:
    data = json.load(f)

# Get sentiment
MCP = "http://localhost:9876/mcp"
def mcp_call(tool, args=None):
    if args is None: args = {}
    payload = json.dumps({"jsonrpc":"2.0","method":"tools/call","params":{"name":tool,"arguments":args},"id":"1"}).encode()
    req = urllib.request.Request(MCP, data=payload, headers={"Content-Type":"application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read().decode())["result"]["content"][0]["text"]
    except Exception:
        return "N/A"

fg = mcp_call("get_fear_greed_index")

print()
print("=" * 90)
print("  KET LUAN: GIAO DICH TOT NHAT HIEN TAI")
print("=" * 90)
print()
print("  SENTIMENT:", fg[:100] if fg else "N/A")
print()

# Top 3
print("  " + "=" * 86)
print("  TOP 3 GIAO DICH BUY")
print("  " + "=" * 86)

for r in data[:3]:
    ind = r["indicators"].get("1h", {})
    sig = r["signals"].get("1h", {})
    reg = r["regimes"].get("1h", {})
    adv = r.get("position_advice", {})
    bt = r["backtest"]

    print()
    print("  #{} {} -- Score: {}/100 -- {}".format(r["rank"], r["symbol"], r["score"], r["recommendation"]))
    print("  Price: ${:,.4f}".format(ind.get("price", 0)))
    print()
    print("  [FH Indicators]")
    print("    Hurst: {:.3f} ({}) | LaguerreRSI: {:.3f} | CMO: {:.1f}".format(
        ind.get("hurst", 0), reg.get("hurst_regime", "?"),
        ind.get("laguerre_rsi", 0), ind.get("cmo", 0)))
    print()
    print("  [FH Weighted Signals]")

    items = sig.get("signal_items", [])
    for item in items[:8]:
        d = item.get("direction", "")
        n = item.get("name", "")
        c = item.get("confidence", 0)
        det = item.get("detail", "")
        marker = "[+]" if d == "Buy" else "[-]" if d == "Sell" else "[~]"
        print("    {} {} (w:{}) {}".format(marker, n, c, det))

    buy_w = sig.get("weighted_buy_score", 0)
    sell_w = sig.get("weighted_sell_score", 0)
    net = buy_w - sell_w
    consensus = "STRONG BULLISH" if net > 200 else "BULLISH" if net > 50 else "NEUTRAL"
    print("    >> Net weighted: {} ({})".format(net, consensus))
    print()
    print("  [Best Backtest] {} | Return: {:+.1f}% | WR: {:.0f}% | Sharpe: {:.2f}".format(
        bt.get("best_strategy", "?"), bt["total_return"], bt["win_rate"], bt["sharpe"]))
    print()

    if adv and adv.get("action") not in ("HOLD", None):
        entry = adv.get("entry", 0)
        sl = adv.get("stop_loss", 0)
        tp1 = adv.get("take_profit_1", 0)
        tp2 = adv.get("take_profit_2", 0)
        rr = adv.get("risk_reward", 0)
        pos = adv.get("position_size_pct", 0)
        print("  [KHUYEN NGHI]")
        print("    >> {} (confidence: {})".format(adv["action"], adv.get("confidence", "?")))
        if entry > 0:
            print("    >> Entry:    ${:,.4f}".format(entry))
            print("    >> Stop Loss: ${:,.4f} ({:+.1f}%)".format(sl, ((sl / entry) - 1) * 100))
            print("    >> TP1:      ${:,.4f} ({:+.1f}%)".format(tp1, ((tp1 / entry) - 1) * 100))
            print("    >> TP2:      ${:,.4f} ({:+.1f}%)".format(tp2, ((tp2 / entry) - 1) * 100))
            print("    >> R:R = 1:{:.1f}".format(rr))
            print("    >> Position: {}% equity".format(pos))

# Market overview
print()
print("  " + "=" * 86)
print("  MARKET OVERVIEW")
buys = sum(1 for r in data if "BUY" in r["recommendation"])
holds = sum(1 for r in data if r["recommendation"] == "HOLD")
sells = sum(1 for r in data if "SELL" in r["recommendation"])
avg_score = sum(r["score"] for r in data) / len(data)

hurst_vals = [r["indicators"].get("1h", {}).get("hurst", 0) for r in data]
hurst_vals = [h for h in hurst_vals if h > 0]
avg_hurst = sum(hurst_vals) / max(len(hurst_vals), 1)

lag_vals = [r["indicators"].get("1h", {}).get("laguerre_rsi", 0) for r in data]
lag_vals = [l for l in lag_vals if l > 0]
avg_lag = sum(lag_vals) / max(len(lag_vals), 1)

print("    {} coins | {} BUY | {} HOLD | {} SELL".format(len(data), buys, holds, sells))
print("    Avg Score: {:.1f}/100 | Avg Hurst: {:.3f} | Avg LagRSI: {:.3f}".format(avg_score, avg_hurst, avg_lag))
print()

if avg_hurst > 0.55:
    print("    >> Hurst={:.3f} > 0.55: THI TRUONG DANG TRENDING".format(avg_hurst))
    print("    >> Uu tien trend-following: ALMA Crossover, EhlersTrend, FH Composite")
else:
    print("    >> Hurst={:.3f} ~ 0.50: THI TRUONG RANDOM WALK".format(avg_hurst))
    print("    >> Can than trong, giam position size")

if avg_lag > 0.7:
    print("    >> LagRSI={:.3f} > 0.7: GAN OVERBOUGHT — can chuyen doi glyc".format(avg_lag))
elif avg_lag < 0.3:
    print("    >> LagRSI={:.3f} < 0.3: OVERSOLD — co hoi mua".format(avg_lag))

print("  " + "=" * 90)
