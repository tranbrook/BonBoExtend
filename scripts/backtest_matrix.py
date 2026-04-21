#!/usr/bin/env python3
"""Multi-strategy backtest matrix for top crypto picks using all strategies including FH."""
import urllib.request
import json
import re
import sys

MCP = "http://localhost:9876/mcp"

ALL_STRATEGIES = [
    "sma_crossover", "rsi_mean_reversion", "bollinger_bands",
    "momentum", "breakout", "macd_crossover",
    # Financial-Hacker strategies
    "alma_crossover", "laguerre_rsi", "cmo_momentum",
    "fh_composite", "ehlers_trend", "enhanced_mean_reversion",
]

FH_TAG = {
    "alma_crossover": "FH", "laguerre_rsi": "FH", "cmo_momentum": "FH",
    "fh_composite": "FH", "ehlers_trend": "FH", "enhanced_mean_reversion": "FH",
}


def mcp_bt(sym, strat):
    """Run backtest via MCP and parse results."""
    payload = json.dumps({
        "jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "run_backtest",
                   "arguments": {"symbol": sym, "interval": "1h", "strategy": strat}},
        "id": "1"
    }).encode()
    req = urllib.request.Request(
        MCP, data=payload,
        headers={"Content-Type": "application/json"}, method="POST"
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode())
    t = data["result"]["content"][0]["text"]
    if "Error" in t and "Return" not in t:
        return None
    ret = wr = sh = dd = tr = 0.0
    m = re.search(r"Total\s+Return[^:]*:\s*([-\d.]+)%", t)
    if m: ret = float(m.group(1))
    m = re.search(r"Win\s+Rate[^:]*:\s*([\d.]+)%", t)
    if m: wr = float(m.group(1))
    m = re.search(r"Sharpe[^:]*:\s*([-\d.]+)", t)
    if m: sh = float(m.group(1))
    m = re.search(r"Max\s*Drawdown[^:]*:\s*([-\d.]+)%", t)
    if m: dd = float(m.group(1))
    m = re.search(r"Total\s*Trades[^:]*:\s*(\d+)", t)
    if m: tr = int(m.group(1))
    return {"ret": ret, "wr": wr, "sh": sh, "dd": dd, "tr": tr}


def main():
    coins = sys.argv[1:] if len(sys.argv) > 1 else [
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "UNIUSDT", "AAVEUSDT",
        "DYDXUSDT", "CRVUSDT", "ENAUSDT", "AVAXUSDT", "LTCUSDT",
    ]

    print()
    print("=" * 120)
    print("  MULTI-STRATEGY BACKTEST MATRIX (Traditional + Financial-Hacker)")
    print("=" * 120)

    best_per_coin = {}
    best_per_strat = {}

    for sym in coins:
        print()
        print("  {}:".format(sym))
        fmt = "    {:<28} {:>8} {:>6} {:>8} {:>8} {:>6}"
        print(fmt.format("Strategy", "Return", "WR%", "Sharpe", "MaxDD%", "Trades"))
        print("    " + "-" * 70)

        best_ret = -999.0
        best_strat = ""

        for s in ALL_STRATEGIES:
            tag = FH_TAG.get(s, "  ")
            r = mcp_bt(sym, s)
            if r is None:
                print("    {} {:<26} ERROR".format(tag, s))
                continue

            marker = "[+]" if r["ret"] > 0 else "[-]"
            print(fmt.format(
                "{} {} {}".format(marker, tag, s),
                "{:+.2f}%".format(r["ret"]),
                "{:.0f}".format(r["wr"]),
                "{:.2f}".format(r["sh"]),
                "{:.1f}%".format(abs(r["dd"])),
                r["tr"],
            ))

            if r["ret"] > best_ret:
                best_ret = r["ret"]
                best_strat = s

            if s not in best_per_strat or r["ret"] > best_per_strat[s]["ret"]:
                best_per_strat[s] = {"sym": sym, "ret": r["ret"], "wr": r["wr"], "sh": r["sh"]}

        best_per_coin[sym] = {"strat": best_strat, "ret": best_ret}

    # Summary
    print()
    print("=" * 120)
    print("  BEST STRATEGY PER COIN")
    print("=" * 120)
    for sym, b in sorted(best_per_coin.items(), key=lambda x: x[1]["ret"], reverse=True):
        marker = "[+]" if b["ret"] > 0 else "[-]"
        tag = FH_TAG.get(b["strat"], "  ")
        print("  {} {} {:<13} -> {} (Return: {:+.2f}%)".format(
            marker, tag, sym, b["strat"], b["ret"]))

    print()
    print("=" * 120)
    print("  BEST COIN PER FH STRATEGY")
    print("=" * 120)
    for s in ALL_STRATEGIES:
        if s in best_per_strat:
            b = best_per_strat[s]
            tag = FH_TAG.get(s, "  ")
            marker = "[+]" if b["ret"] > 0 else "[-]"
            print("  {} {} {:<26} -> {} (Return: {:+.2f}%, WR: {:.0f}%, Sharpe: {:.2f})".format(
                marker, tag, s, b["sym"], b["ret"], b["wr"], b["sh"]))

    print("=" * 120)


if __name__ == "__main__":
    main()
