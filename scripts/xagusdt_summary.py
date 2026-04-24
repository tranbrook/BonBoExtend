#!/usr/bin/env python3
"""XAGUSDT Quick Summary — key metrics + forecast"""
import json, os, sys
from datetime import datetime, timedelta
sys.path.insert(0, os.path.join(os.path.dirname(__file__)))
from xagusdt_analysis import *

data = fetch_all_timeframes()
ind_1d = compute_indicators(data["1d"])

price = data["1d"]["close"].iloc[-1]
forecast = forecast_30days(data["1d"], ind_1d)
pivots = compute_pivot_points(data["1d"])
key_levels = find_key_levels(data["1d"])

rsi = ind_1d["rsi_14"].iloc[-1]
adx = ind_1d["adx"].iloc[-1]
macd_h = ind_1d["macd_hist"].iloc[-1]
bb = ind_1d["bb_pct"].iloc[-1]
sma50 = ind_1d["sma_50"].iloc[-1]
sma200_val = ind_1d["sma_200"].iloc[-1]
hurst = ind_1d["hurst"].iloc[-1] if not pd.isna(ind_1d["hurst"].iloc[-1]) else 0.5
lag = ind_1d["lag_rsi"].iloc[-1]
cmo = ind_1d["cmo"].iloc[-1]
atr = ind_1d["atr_14"].iloc[-1]
sar = ind_1d["sar"].iloc[-1]

print()
print("=" * 100)
print("  XAGUSDT (SILVER) — KEY METRICS + 30-DAY FORECAST")
print("=" * 100)
print()
print(f"  Current Price:    ${price:.3f}")
print(f"  SMA50:            ${sma50:.3f}  (price {'ABOVE' if price > sma50 else 'BELOW'})")
if not pd.isna(sma200_val):
    print(f"  SMA200:           ${sma200_val:.3f}  (price {'ABOVE' if price > sma200_val else 'BELOW'})")
print(f"  RSI(14):          {rsi:.1f}  {'OVERBOUGHT' if rsi > 70 else 'OVERSOLD' if rsi < 30 else 'NEUTRAL'}")
print(f"  MACD Hist:        {macd_h:.4f}  {'BULLISH' if macd_h > 0 else 'BEARISH'}")
print(f"  ADX:              {adx:.1f}  {'TRENDING' if adx > 25 else 'RANGING'}")
print(f"  BB %B:            {bb:.3f}")
print(f"  ATR(14):          {atr:.3f} ({atr/price*100:.2f}%)")
print(f"  SAR:              ${sar:.3f}  {'BULLISH' if sar < price else 'BEARISH'}")
print(f"  LaguerreRSI:      {lag:.3f}")
print(f"  CMO:              {cmo:.1f}")
print(f"  Hurst:            {hurst:.3f}  {'TRENDING' if hurst > 0.55 else 'MEAN-REV' if hurst < 0.45 else 'RANDOM'}")
print()

print("-" * 100)
print("  30-DAY FORECAST")
print("-" * 100)

sc = forecast["scenarios"]
pr = forecast["probabilities"]
ev = (pr["bullish"] * sc["bullish"]["change"] +
      pr["base"] * sc["base"]["change"] +
      pr["bearish"] * sc["bearish"]["change"])

print()
print(f"  Bullish ({pr['bullish']*100:.0f}%):  ${sc['bullish']['target']:.3f}  ({sc['bullish']['change']:+.2f}%)")
print(f"  Base    ({pr['base']*100:.0f}%):  ${sc['base']['target']:.3f}  ({sc['base']['change']:+.2f}%)")
print(f"  Bearish ({pr['bearish']*100:.0f}%):  ${sc['bearish']['target']:.3f}  ({sc['bearish']['change']:+.2f}%)")
print()
print(f"  Expected return: {ev:+.2f}%")
print(f"  Expected price:  ${price * (1 + ev/100):.3f}")
print()

print("-" * 100)
print("  WEEKLY PROJECTIONS")
print("-" * 100)
for wk in range(1, 5):
    frac = wk / 4.3
    bull_w = price + (sc["bullish"]["target"] - price) * frac
    base_w = price + (sc["base"]["target"] - price) * frac
    bear_w = price + (sc["bearish"]["target"] - price) * frac
    dt = datetime.now() + timedelta(weeks=wk)
    print(f"  Week {wk} ({dt.strftime('%b %d')}):  Bull ${bull_w:.3f}  Base ${base_w:.3f}  Bear ${bear_w:.3f}")

print()

print("-" * 100)
print("  SUPPORT / RESISTANCE")
print("-" * 100)
print(f"  Pivot: ${pivots['pivot']:.3f}")
for name, key in [("R3","r3"),("R2","r2"),("R1","r1"),("S1","s1"),("S2","s2"),("S3","s3")]:
    val = pivots["classic"][key]
    print(f"  {name}: ${val:.3f}  ({(val/price-1)*100:+.2f}%)")

if key_levels["resistances"]:
    res_str = ", ".join(f"${r:.3f}" for r in key_levels["resistances"][:3])
    print(f"  Key Resistance (swing): {res_str}")
if key_levels["supports"]:
    sup_str = ", ".join(f"${s:.3f}" for s in key_levels["supports"][:3])
    print(f"  Key Support (swing): {sup_str}")

print()

# Trade plan
regime = detect_regime(hurst, adx, atr, ind_1d["atr_14"].mean())
plan = generate_trade_plan(price, forecast, pivots, regime, rsi, adx)

print("-" * 100)
print("  TRADE PLAN")
print("-" * 100)
print(f"  Direction: {plan['direction']}")
if plan["direction"] != "WAIT":
    print(f"  Entry:     ${plan['entry']:.3f}")
    print(f"  Stop Loss: ${plan['stop_loss']:.3f}  (risk: {plan['risk_pct']:.2f}%)")
    print(f"  TP1:       ${plan['tp1']:.3f}  (R:R = 1:{plan['rr1']:.1f})")
    print(f"  TP2:       ${plan['tp2']:.3f}  (R:R = 1:{plan['rr2']:.1f})")
    print(f"  TP3:       ${plan['tp3']:.3f}  (full target)")
    print(f"  Confidence: {plan['confidence']:.0f}%")
else:
    print(f"  Reason: {plan['reason']}")

print()
print(f"  Regime: {regime['regime']}")
print(f"  Best strategies: {regime['strategy']}")
print()
print("=" * 100)

# Save
rdir = os.path.expanduser("~/.bonbo/reports")
os.makedirs(rdir, exist_ok=True)
ts = datetime.now().strftime("%Y%m%d_%H%M%S")
rpt = os.path.join(rdir, f"xagusdt_{ts}.json")
with open(rpt, "w") as f:
    json.dump({
        "timestamp": datetime.now().isoformat(),
        "symbol": "XAGUSDT",
        "price": price,
        "rsi": rsi, "adx": adx, "hurst": hurst,
        "lag_rsi": lag, "cmo": cmo, "bb_pct": bb,
        "forecast": sc, "probabilities": pr,
        "expected_return": ev,
        "regime": regime,
        "trade_plan": plan,
    }, f, indent=2, default=str)
print(f"  Report: {rpt}")
