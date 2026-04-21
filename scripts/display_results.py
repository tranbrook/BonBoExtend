#!/usr/bin/env python3
"""Display BonBo analysis results from JSON — with Financial-Hacker indicators."""
import json, sys

path = sys.argv[1] if len(sys.argv) > 1 else ""
if not path:
    import glob
    files = sorted(glob.glob("/home/tranbrook/.bonbo/reports/top100_*.json"))
    path = files[-1] if files else ""
if not path:
    print("No report found"); sys.exit(1)

with open(path) as f:
    data = json.load(f)

# ── Table 1: Overview with FH indicators ──
print()
print("=" * 155)
print("  {:<3} {:<13} {:>11} {:>6} {:>7} {:>6} {:>5} {:>5} {:>5} {:>6} {:>6} {:<14} {:>7} {:>6} {:<13}".format(
    "#", "Symbol", "Price", "RSI", "LagRSI", "Hurst", "CMO", "ALMA", "SS%", "BuyW", "SellW", "Regime", "BT%", "Score", "Rec"))
print("-" * 155)

for r in data:
    ind = r["indicators"].get("1h", {})
    sig = r["signals"].get("1h", {})
    reg = r["regimes"].get("1h", {})
    price = ind.get("price", 0)
    rsi = ind.get("rsi", 50)
    lag_rsi = ind.get("laguerre_rsi", 0)
    hurst = ind.get("hurst", 0)
    cmo = ind.get("cmo", 0)
    alma_sig = ind.get("alma_signal", "")
    ss_slope = ind.get("super_smoother_slope", 0)
    buy_w = sig.get("weighted_buy_score", 0)
    sell_w = sig.get("weighted_sell_score", 0)
    regime = reg.get("regime", "?")
    bt = r["backtest"]["total_return"]
    score = r["score"]
    rec = r["recommendation"]

    price_s = "${:,.4f}".format(price) if price > 0 else "N/A"

    # ALMA indicator
    alma_s = "B" if alma_sig == "Bullish" else "S" if alma_sig == "Bearish" else "-"
    # Hurst indicator
    hurst_s = "{:.2f}".format(hurst) if hurst > 0 else "-"
    # CMO
    cmo_s = "{:+.0f}".format(cmo) if cmo != 0 else "-"
    # SuperSmoother slope
    ss_s = "{:+.2f}".format(ss_slope) if ss_slope != 0 else "-"
    # LaguerreRSI
    lag_s = "{:.2f}".format(lag_rsi) if lag_rsi > 0 else "-"

    # Rec marker
    if "STRONG_BUY" in rec: e = "++"
    elif "BUY" in rec: e = "+ "
    elif "SELL" in rec and "STRONG" in rec: e = "--"
    elif "SELL" in rec: e = "- "
    else: e = "~ "

    print("  {:<3} {:<13} {:>11} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:<14} {:>+6.1f}% {:>5.0f} {}{}".format(
        r["rank"], r["symbol"], price_s, rsi, lag_s, hurst_s, cmo_s, alma_s, ss_s,
        buy_w, sell_w, regime, bt, score, e, rec))

# ── Top 5 Detail ──
print()
print("=" * 155)
print("  TOP 5 CO HOI GIAO DICH TOT NHAT (Financial-Hacker Enhanced)")
print("=" * 155)

for r in data[:5]:
    ind = r["indicators"].get("1h", {})
    sig = r["signals"].get("1h", {})
    adv = r.get("position_advice", {})
    reg = r["regimes"].get("1h", {})

    print()
    print("  #{} {} -- Score: {}/100 -- {}".format(r["rank"], r["symbol"], r["score"], r["recommendation"]))

    # Traditional
    print("  [Traditional] Price: ${:,.4f} | RSI: {:.1f} | MACD: {:.4f} | BB%B: {:.2f}".format(
        ind.get("price", 0), ind.get("rsi", 50), ind.get("macd_hist", 0), ind.get("bb_pctb", 0.5)))

    # Financial-Hacker
    print("  [FH] Hurst: {:.3f} ({}) | LaguerreRSI: {:.3f} | CMO: {:.1f}".format(
        ind.get("hurst", 0), reg.get("hurst_regime", "?"),
        ind.get("laguerre_rsi", 0), ind.get("cmo", 0)))
    print("  [FH] ALMA(10/30): {} ({:+.2f}%) | SuperSmoother slope: {:+.4f}%".format(
        ind.get("alma_signal", "-"), ind.get("alma_pct", 0),
        ind.get("super_smoother_slope", 0)))

    # Weighted signals
    buy_w = sig.get("weighted_buy_score", 0)
    sell_w = sig.get("weighted_sell_score", 0)
    net = buy_w - sell_w
    print("  [FH Signals] Weighted Buy: {} | Sell: {} | Net: {} | Consensus: {}".format(
        buy_w, sell_w, net, "BULLISH" if net > 0 else "BEARISH" if net < 0 else "NEUTRAL"))

    # Regime + Strategy hint
    print("  [Regime] {} | Hurst regime: {} | Hint: {}".format(
        reg.get("regime", "?"), reg.get("hurst_regime", "?"),
        reg.get("strategy_hint", "none")))

    # FH signal details
    items = sig.get("signal_items", [])
    if items:
        print("  [Signal Details]:")
        for item in items[:8]:
            direction = item.get("direction", "")
            name = item.get("name", "")
            conf = item.get("confidence", 0)
            detail = item.get("detail", "")
            marker = "+" if direction == "Buy" else "-" if direction == "Sell" else "~"
            print("    {} {} (w:{}) - {}".format(marker, name, conf, detail))

    # Backtest
    print("  [Backtest] Return={:+.1f}% | WR={:.0f}% | Sharpe={:.2f} | Trades={} | Best Strat: {}".format(
        r["backtest"]["total_return"], r["backtest"]["win_rate"],
        r["backtest"]["sharpe"], r["backtest"]["total_trades"],
        r["backtest"].get("best_strategy", "?")))

    # Position advice
    if adv and adv.get("action") not in ("HOLD", None):
        print("  >> Action: {} (confidence: {})".format(adv["action"], adv.get("confidence", "?")))
        print("  >> Entry: ${:,.4f} | SL: ${:,.4f} | TP1: ${:,.4f} | TP2: ${:,.4f}".format(
            adv.get("entry", 0), adv.get("stop_loss", 0),
            adv.get("take_profit_1", 0), adv.get("take_profit_2", 0)))
        print("  >> R:R = 1:{:.1f} | Position: {}% equity".format(
            adv.get("risk_reward", 0), adv.get("position_size_pct", 0)))
        for note in adv.get("notes", []):
            print("  >> {}".format(note))

# ── Summary ──
print()
print("=" * 155)
buys = sum(1 for r in data if "BUY" in r["recommendation"])
holds = sum(1 for r in data if r["recommendation"] == "HOLD")
sells = sum(1 for r in data if "SELL" in r["recommendation"])
avg_score = sum(r["score"] for r in data) / max(len(data), 1)
avg_rsi = sum(r["indicators"].get("1h", {}).get("rsi", 50) for r in data) / max(len(data), 1)
avg_hurst = sum(r["indicators"].get("1h", {}).get("hurst", 0) for r in data if r["indicators"].get("1h", {}).get("hurst", 0) > 0)
n_hurst = sum(1 for r in data if r["indicators"].get("1h", {}).get("hurst", 0) > 0)
avg_hurst = avg_hurst / max(n_hurst, 1)
avg_lag = sum(r["indicators"].get("1h", {}).get("laguerre_rsi", 0) for r in data if r["indicators"].get("1h", {}).get("laguerre_rsi", 0) > 0)
n_lag = sum(1 for r in data if r["indicators"].get("1h", {}).get("laguerre_rsi", 0) > 0)
avg_lag = avg_lag / max(n_lag, 1)

print("  MARKET SUMMARY: {} coins".format(len(data)))
print("  Buy: {} | Hold: {} | Sell: {}".format(buys, holds, sells))
print("  Avg Score: {:.1f} | Avg RSI: {:.1f} | Avg Hurst: {:.3f} | Avg LaguerreRSI: {:.3f}".format(
    avg_score, avg_rsi, avg_hurst, avg_lag))
if avg_hurst > 0.55:
    print("  >> Hurst > 0.55: Thi truong dang TRENDING → uu tien trend-following")
elif avg_hurst < 0.45:
    print("  >> Hurst < 0.45: Thi truong dang MEAN-REVERTING → uu tien mean-reversion")
else:
    print("  >> Hurst ~ 0.50: Thi truong RANDOM WALK → can than trong")
print("=" * 155)
