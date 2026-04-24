#!/usr/bin/env python3
"""
BonBo Top 100 Multi-TF + Full Backtest Scanner v4.0

3-Phase Architecture:
  Phase 1: Quick scan 100 coins (1h+4h indicators, parallel workers) → rank → top 30
  Phase 2: Deep analysis top 30 (15m+1h+4h+1d indicators + signals + regime + S/R)
  Phase 3: Multi-strategy backtest top 20 across 3 timeframes → find best trades

13 Strategies Tested:
  Traditional: sma_crossover, ema_crossover, rsi_mean_reversion, bollinger_bands,
               momentum, breakout, macd_crossover
  Financial-Hacker: alma_crossover, laguerre_rsi, cmo_momentum,
                    fh_composite, ehlers_trend, enhanced_mean_reversion

Usage:
  python3 scripts/top100_full_scan.py                   # Full run
  python3 scripts/top100_full_scan.py --quick            # 20 coins, fast mode
  python3 scripts/top100_full_scan.py --coins BTCUSDT ETHUSDT SOLUSDT  # Specific coins
  python3 scripts/top100_full_scan.py --top-n 50         # Analyze top 50
  python3 scripts/top100_full_scan.py --no-backtest      # Skip backtest phase
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
import threading
import queue
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed

# ════════════════════════════════════════════════════════════════════
# CONFIG
# ════════════════════════════════════════════════════════════════════

MCP_BIN = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "target/release/bonbo-extend-mcp",
)

NUM_WORKERS = 6  # parallel MCP workers

SKIP_SYMBOLS = {
    "USDCUSDT", "USDTUSDT", "BUSDUSDT", "DAIUSDT", "TUSDUSDT",
    "FDUSDUSDT", "RLUSDUSDT", "EURUSDT", "PAXGUSDT", "USDPUSDT",
    "IDRTUSDT", "BIDRUSDT", "USDSBUSDT", "USDSUSDT",
    "PYUSDUSDT", "FIRSTUSDT", "LDBNBUSDT", "LDBTCUSDT",
    "BTCDOMUSDT", "DEFIUSDT", "NFTUSDT",
}

ALL_STRATEGIES = [
    # Traditional
    "sma_crossover", "ema_crossover", "rsi_mean_reversion",
    "bollinger_bands", "momentum", "breakout", "macd_crossover",
    # Financial-Hacker
    "alma_crossover", "laguerre_rsi", "cmo_momentum",
    "fh_composite", "ehlers_trend", "enhanced_mean_reversion",
]

FH_TAG = {
    "alma_crossover": "FH", "laguerre_rsi": "FH", "cmo_momentum": "FH",
    "fh_composite": "FH", "ehlers_trend": "FH", "enhanced_mean_reversion": "FH",
}

REGIME_BEST_STRATEGY = {
    "TRENDING": ["alma_crossover", "fh_composite", "ehlers_trend", "sma_crossover"],
    "MEAN_REV": ["rsi_mean_reversion", "bollinger_bands", "enhanced_mean_reversion", "laguerre_rsi"],
    "VOLATILE": ["breakout", "momentum", "cmo_momentum"],
    "QUIET": ["macd_crossover", "ema_crossover"],
    "RANDOM": ["fh_composite", "bollinger_bands"],
}

BACKTEST_INTERVALS = ["1h", "4h", "1d"]

FALLBACK_COINS = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "DOGEUSDT",
    "ADAUSDT", "AVAXUSDT", "LINKUSDT", "DOTUSDT", "NEARUSDT", "APTUSDT",
    "ARBUSDT", "UNIUSDT", "AAVEUSDT", "MKRUSDT", "LTCUSDT", "ATOMUSDT",
    "FILUSDT", "OPUSDT", "INJUSDT", "SUIUSDT", "SEIUSDT", "FETUSDT",
    "RENDERUSDT", "PEPEUSDT", "WIFUSDT", "ORDIUSDT", "STXUSDT", "TIAUSDT",
    "PENDLEUSDT", "ENAUSDT", "TONUSDT", "KASUSDT", "JUPUSDT", "WLDUSDT",
    "CKBUSDT", "COREUSDT", "IMXUSDT", "RUNEUSDT", "APEUSDT", "DYDXUSDT",
    "COMPUSDT", "CRVUSDT", "LDOUSDT", "SNXUSDT", "1INCHUSDT", "GRTUSDT",
    "SHIBUSDT", "TRXUSDT", "ETCUSDT", "BCHUSDT", "ALGOUSDT", "VETUSDT",
    "ICPUSDT", "FILUSDT", "HBARUSDT", "SANDUSDT", "MANAUSDT", "AXSUSDT",
    "THETAUSDT", "FTMUSDT", "EGLDUSDT", "FLOWUSDT", "XTZUSDT", "NEOUSDT",
    "GALAUSDT", "ROSEUSDT", "ZILUSDT", "ONTUSDT", "IOSTUSDT", "ZECUSDT",
    "DASHUSDT", "WAVESUSDT", "NEARUSDT", "KLAYUSDT", "IOTAUSDT",
    "MINAUSDT", "CFXUSDT", "ACHUSDT", "PEOPLEUSDT", "WOOUSDT",
    "DYMOUSDT", "LEVERUSDT", "AMBUSDT", "PHBUSDT", "HOOKUSDT",
]


# ════════════════════════════════════════════════════════════════════
# COLOR HELPERS
# ════════════════════════════════════════════════════════════════════

def C(c, t):
    return f"\033[{c}m{t}\033[0m"

def BOLD(t): return C("1", t)
def GR(t): return C("32", t)
def BGR(t): return C("1;32", t)
def RD(t): return C("31", t)
def BRD(t): return C("1;31", t)
def YL(t): return C("33", t)
def BL(t): return C("34", t)
def MG(t): return C("35", t)
def CY(t): return C("36", t)
def DM(t): return C("2", t)
def BW(t): return C("1;37", t)

SEP = "═" * 120
LINE = "─" * 120


# ════════════════════════════════════════════════════════════════════
# MCP CLIENT POOL
# ════════════════════════════════════════════════════════════════════

class MCPClient:
    """Thread-safe MCP stdio client."""

    def __init__(self, bin_path):
        self.bin_path = bin_path
        self._lock = threading.Lock()
        self._seq = 0

    def call(self, tool, args=None, timeout=45):
        with self._lock:
            self._seq += 1
            if args is None:
                args = {}
            init_req = json.dumps({
                "jsonrpc": "2.0", "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "bonbo-full-scan", "version": "4.0"},
                },
                "id": "0",
            })
            call_req = json.dumps({
                "jsonrpc": "2.0", "method": "tools/call",
                "params": {"name": tool, "arguments": args},
                "id": str(self._seq),
            })
            stdin_data = init_req + "\n" + call_req + "\n"
            try:
                p = subprocess.run(
                    [self.bin_path], input=stdin_data,
                    capture_output=True, text=True, timeout=timeout,
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
            except subprocess.TimeoutExpired:
                pass
            except Exception:
                pass
            return ""


# Pool of MCP clients for parallel work
_client_pool = queue.Queue()


def init_pool(workers=NUM_WORKERS):
    for _ in range(workers):
        _client_pool.put(MCPClient(MCP_BIN))


def get_cli():
    return _client_pool.get()


def put_cli(c):
    _client_pool.put(c)


# ════════════════════════════════════════════════════════════════════
# PARSERS — extract data from MCP markdown text
# ════════════════════════════════════════════════════════════════════

def p_price(t):
    m = re.search(r"\$([0-9,.]+)", t)
    return float(m.group(1).replace(",", "")) if m else 0.0


def p_hurst(t):
    m = re.search(r"Hurst[^:]*:\s*([0-9.]+)", t)
    return float(m.group(1)) if m else 0.0


def p_lag(t):
    m = re.search(r"LaguerreRSI[^:]*:\s*([0-9.]+)", t)
    return float(m.group(1)) if m else 0.5


def p_cmo(t):
    m = re.search(r"CMO[^:]*:\s*([-0-9.]+)", t)
    return float(m.group(1)) if m else 0.0


def p_rsi(t):
    m = re.search(r"RSI[^:]*:\s*([0-9.]+)", t)
    return float(m.group(1)) if m else 50.0


def p_bb(t):
    m = re.search(r"%B=([0-9.]+)", t)
    return float(m.group(1)) if m else 0.5


def p_macd(t):
    m = re.search(r"MACD[^:]*:\s*([-0-9.]+)", t)
    return float(m.group(1)) if m else 0.0


def p_volume(t):
    m = re.search(r"Volume[^:]*:\s*([0-9,.]+)", t)
    return float(m.group(1).replace(",", "")) if m else 0.0


def p_signals(t):
    sigs = []
    for m in re.finditer(
        r"(🟢|🔴)\s+\*\*(Buy|Sell)\*\*\s+\[([^\]]+)\]\s+\(([0-9]+)%\)", t
    ):
        sigs.append({
            "direction": m.group(2).upper(),
            "name": m.group(3),
            "confidence": int(m.group(4)),
        })
    return sigs


def p_regime(t):
    tl = t.lower()
    h = p_hurst(t)
    if "trending" in tl:
        rg = "TRENDING"
    elif "quiet" in tl:
        rg = "QUIET"
    elif "volatile" in tl:
        rg = "VOLATILE"
    elif "mean-revert" in tl:
        rg = "MEAN_REV"
    elif "random" in tl:
        rg = "RANDOM"
    else:
        rg = "UNKNOWN"
    return {"regime": rg, "hurst": h}


def p_sr(t):
    sup, res = [], []
    for m in re.finditer(r"(S\d?):\s*\$([0-9,.]+)", t):
        sup.append(float(m.group(2).replace(",", "")))
    for m in re.finditer(r"(R\d?):\s*\$([0-9,.]+)", t):
        res.append(float(m.group(2).replace(",", "")))
    return {"supports": sup, "resistances": res}


def p_alma(t):
    m = re.search(r"(ALMA.*?)(?:\n|$)", t)
    return m.group(0).strip() if m else ""


def p_ss(t):
    m = re.search(r"(SuperSmoother.*?)(?:\n|$)", t)
    return m.group(0).strip() if m else ""


def parse_backtest(text):
    """Parse backtest report text → dict of metrics."""
    if not text or ("Error" in text and "Return" not in text):
        return None
    ret = wr = sh = dd = mdd = 0.0
    tr = 0
    m = re.search(r"Total\s+Return[^:]*:\s*([-\d.]+)%", text)
    if m:
        ret = float(m.group(1))
    m = re.search(r"Win\s+Rate[^:]*:\s*([\d.]+)%", text)
    if m:
        wr = float(m.group(1))
    m = re.search(r"Sharpe[^:]*:\s*([-\d.]+)", text)
    if m:
        sh = float(m.group(1))
    m = re.search(r"Max\s*[Dd]rawdown[^:]*:\s*([-\d.]+)%", text)
    if m:
        dd = float(m.group(1))
        mdd = abs(dd)
    m = re.search(r"Total\s*Trades[^:]*:\s*(\d+)", text)
    if m:
        tr = int(m.group(1))
    # Profit factor
    pf = 0.0
    m = re.search(r"Profit\s*Factor[^:]*:\s*([-\d.]+)", text)
    if m:
        pf = float(m.group(1))

    return {
        "ret": ret, "wr": wr, "sh": sh, "dd": dd,
        "mdd": mdd, "tr": tr, "pf": pf,
    }


# ════════════════════════════════════════════════════════════════════
# SCORING ENGINE
# ════════════════════════════════════════════════════════════════════

def score_timeframe(ind_text, sig_text, reg_text, sr_text=""):
    """Score a single timeframe: returns (score 0-100, details dict)."""
    price = p_price(ind_text)
    hurst = p_hurst(ind_text) or p_hurst(reg_text)
    lag = p_lag(ind_text)
    cmo = p_cmo(ind_text)
    rsi = p_rsi(ind_text)
    bb = p_bb(ind_text)
    alma = p_alma(ind_text)
    ss = p_ss(ind_text)
    signals = p_signals(sig_text)
    regime = p_regime(reg_text)
    srlvls = p_sr(sr_text)

    details = {
        "price": price, "hurst": hurst, "regime": regime["regime"],
        "lag": lag, "cmo": cmo, "rsi": rsi, "bb": bb,
        "alma": alma, "ss": ss, "signals": signals, "sr": srlvls,
    }

    total, weight = 0.0, 0.0

    # ── Hurst (0-20): trending markets get higher scores ──
    if hurst > 0.6:
        h_score = 20.0
    elif hurst > 0.55:
        h_score = 17.0
    elif hurst > 0.5:
        h_score = 12.0
    elif hurst > 0:
        h_score = 5.0
    else:
        h_score = 5.0
    total += h_score
    weight += 20

    # ── Signals (0-30): buy signals boost, sell signals penalize ──
    buys = [s for s in signals if s["direction"] == "BUY"]
    sells = [s for s in signals if s["direction"] == "SELL"]
    n_total = len(signals) if signals else 1
    if not signals:
        sig_score = 5.0
    elif len(buys) > len(sells):
        avg_conf = sum(s["confidence"] for s in buys) / len(buys)
        sig_score = min(10 + (len(buys) / n_total) * 15 + (avg_conf / 100) * 5, 30.0)
    elif len(sells) > len(buys):
        avg_conf = sum(s["confidence"] for s in sells) / len(sells)
        sig_score = min(5 + (len(sells) / n_total) * 10 + (avg_conf / 100) * 5, 30.0)
    else:
        sig_score = 8.0
    total += sig_score
    weight += 30

    # ── LaguerreRSI (0-15): oversold = bullish signal ──
    if lag < 0.2:
        lag_score = 15.0
    elif lag < 0.35:
        lag_score = 12.0
    elif lag < 0.65:
        lag_score = 7.0
    elif lag < 0.8:
        lag_score = 5.0
    else:
        lag_score = 3.0
    total += lag_score
    weight += 15

    # ── CMO momentum (0-15): strong momentum = good ──
    if abs(cmo) > 40:
        cmo_score = 13.0
    elif abs(cmo) > 20:
        cmo_score = 9.0
    elif abs(cmo) > 10:
        cmo_score = 6.0
    else:
        cmo_score = 3.0
    total += cmo_score
    weight += 15

    # ── Trend alignment (0-20): ALMA + SuperSmoother ──
    alma_bull = "Bullish" in alma or "🟢" in alma
    ss_bull = "positive" in ss.lower() or "🟢" in ss
    if alma_bull and ss_bull:
        trend_score = 20.0
    elif alma_bull or ss_bull:
        trend_score = 13.0
    else:
        trend_score = 6.0
    total += trend_score
    weight += 20

    final = min((total / weight * 100) if weight > 0 else 0, 100.0)
    return final, details


def compute_confluence(tf_data):
    """Compute multi-TF confluence direction."""
    dirs = {}
    for tf in ("15m", "1h", "4h", "1d"):
        if tf in tf_data:
            buys = sum(1 for x in tf_data[tf][1].get("signals", []) if x["direction"] == "BUY")
            sells = sum(1 for x in tf_data[tf][1].get("signals", []) if x["direction"] == "SELL")
            dirs[tf] = "BUY" if buys > sells else "SELL" if sells > buys else "NEUTRAL"

    buy_count = sum(1 for v in dirs.values() if v == "BUY")
    sell_count = sum(1 for v in dirs.values() if v == "SELL")

    if buy_count >= 3:
        return "STRONG_BUY", dirs
    elif buy_count >= 2:
        return "BUY", dirs
    elif sell_count >= 3:
        return "STRONG_SELL", dirs
    elif sell_count >= 2:
        return "SELL", dirs
    return "MIXED", dirs


def compute_backtest_score(bt_result):
    """Score a backtest result: higher return + win rate + sharpe, lower drawdown."""
    if bt_result is None:
        return 0.0
    ret_score = max(min(bt_result["ret"] / 5.0, 20.0), -20.0)  # ±20
    wr_score = (bt_result["wr"] / 100.0) * 15.0  # 0-15
    sh_score = max(min(bt_result["sh"] * 5.0, 15.0), -10.0)  # ±15
    dd_penalty = max(bt_result["mdd"] / 2.0, 0) * 0.5  # penalty
    return max(ret_score + wr_score + sh_score - dd_penalty, 0.0)


# ════════════════════════════════════════════════════════════════════
# PHASE 1: QUICK SCAN — 100 coins, 1h + 4h
# ════════════════════════════════════════════════════════════════════

def phase1_quick_scan(symbols):
    """Quick scan all coins on 1h + 4h. Returns ranked list."""
    results = {}
    total = len(symbols)
    t0 = time.time()

    def work(sym):
        cli = get_cli()
        try:
            tf_data = {}
            for tf in ("1h", "4h"):
                ind = cli.call("analyze_indicators", {"symbol": sym, "interval": tf, "limit": 200})
                sig = cli.call("get_trading_signals", {"symbol": sym, "interval": tf})
                reg = cli.call("detect_market_regime", {"symbol": sym, "interval": tf})
                sc, det = score_timeframe(ind, sig, reg)
                tf_data[tf] = (sc, det)
            return sym, tf_data
        except Exception:
            return sym, {}
        finally:
            put_cli(cli)

    done = 0
    with ThreadPoolExecutor(max_workers=NUM_WORKERS) as pool:
        futs = {pool.submit(work, s): s for s in symbols}
        for f in as_completed(futs):
            done += 1
            sym = futs[f]
            try:
                s, td = f.result()
                if td:
                    weights = {"1h": 0.45, "4h": 0.55}
                    comp = sum(td[tf][0] * w for tf, w in weights.items() if tf in td)
                    results[s] = {"tf_data": td, "score": comp}
                else:
                    results[sym] = {"tf_data": {}, "score": 0}
            except Exception:
                results[sym] = {"tf_data": {}, "score": 0}
            elapsed = time.time() - t0
            eta = (elapsed / done) * (total - done) if done > 0 else 0
            pct = done / total * 100
            sys.stdout.write(
                f"\r  {CY('Phase 1')} [{done}/{total}] {pct:5.1f}% | "
                f"{elapsed:5.0f}s elapsed, ~{eta:4.0f}s remaining"
                f"{' ' * 10}"
            )
            sys.stdout.flush()

    print(f"\n  {BGR('✅')} Phase 1 done: {total} coins in {time.time() - t0:.1f}s")
    return sorted(results.items(), key=lambda x: x[1]["score"], reverse=True)


# ════════════════════════════════════════════════════════════════════
# PHASE 2: DEEP ANALYSIS — Top 30, 4 timeframes + S/R
# ════════════════════════════════════════════════════════════════════

def phase2_deep_analysis(ranked, top_n=30):
    """Deep multi-TF analysis for top coins."""
    targets = ranked[:top_n]
    all_tfs = ("15m", "1h", "4h", "1d")
    tf_weights = {"15m": 0.10, "1h": 0.30, "4h": 0.35, "1d": 0.25}
    results = {}
    total = len(targets)
    t0 = time.time()

    def work(sym):
        cli = get_cli()
        try:
            td = {}
            for tf in all_tfs:
                ind = cli.call("analyze_indicators", {"symbol": sym, "interval": tf, "limit": 200})
                sig = cli.call("get_trading_signals", {"symbol": sym, "interval": tf})
                reg = cli.call("detect_market_regime", {"symbol": sym, "interval": tf})
                sr = cli.call("get_support_resistance", {"symbol": sym, "interval": tf})
                sc, det = score_timeframe(ind, sig, reg, sr)
                td[tf] = (sc, det)
            return sym, td
        except Exception:
            return sym, {}
        finally:
            put_cli(cli)

    done = 0
    with ThreadPoolExecutor(max_workers=NUM_WORKERS) as pool:
        futs = {pool.submit(work, s): s for s, _ in targets}
        for f in as_completed(futs):
            done += 1
            sym = futs[f]
            try:
                s, td = f.result()
                if td:
                    comp = sum(td[tf][0] * tf_weights[tf] for tf in all_tfs if tf in td)
                    confluence, directions = compute_confluence(td)
                    results[s] = {
                        "tf_data": td, "score": comp,
                        "confluence": confluence, "directions": directions,
                    }
                else:
                    results[sym] = {
                        "tf_data": {}, "score": 0,
                        "confluence": "MIXED", "directions": {},
                    }
            except Exception:
                results[sym] = {
                    "tf_data": {}, "score": 0,
                    "confluence": "MIXED", "directions": {},
                }
            elapsed = time.time() - t0
            eta = (elapsed / done) * (total - done) if done > 0 else 0
            pct = done / total * 100
            sys.stdout.write(
                f"\r  {CY('Phase 2')} [{done}/{total}] {pct:5.1f}% | "
                f"{elapsed:5.0f}s elapsed, ~{eta:4.0f}s remaining"
                f"{' ' * 10}"
            )
            sys.stdout.flush()

    print(f"\n  {BGR('✅')} Phase 2 done: {total} coins in {time.time() - t0:.1f}s")
    return sorted(results.items(), key=lambda x: x[1]["score"], reverse=True)


# ════════════════════════════════════════════════════════════════════
# PHASE 3: MULTI-STRATEGY BACKTEST — Top 20 × 13 strategies × 3 TFs
# ════════════════════════════════════════════════════════════════════

def phase3_backtest(deep_results, top_n=20):
    """Run all strategies on top coins across multiple timeframes."""
    targets = deep_results[:top_n]
    bt_data = {}  # sym → { (strategy, interval): result }
    total = len(targets) * len(ALL_STRATEGIES) * len(BACKTEST_INTERVALS)
    done = 0
    t0 = time.time()

    def run_one(sym, strategy, interval):
        cli = get_cli()
        try:
            text = cli.call(
                "run_backtest",
                {"symbol": sym, "interval": interval, "strategy": strategy},
                timeout=60,
            )
            return sym, strategy, interval, parse_backtest(text)
        except Exception:
            return sym, strategy, interval, None
        finally:
            put_cli(cli)

    with ThreadPoolExecutor(max_workers=NUM_WORKERS) as pool:
        futs = []
        for sym, _ in targets:
            for strat in ALL_STRATEGIES:
                for iv in BACKTEST_INTERVALS:
                    futs.append(pool.submit(run_one, sym, strat, iv))

        for f in as_completed(futs):
            done += 1
            sym, strat, iv, result = f.result()
            if sym not in bt_data:
                bt_data[sym] = {}
            bt_data[sym][(strat, iv)] = result

            elapsed = time.time() - t0
            eta = (elapsed / done) * (total - done) if done > 0 else 0
            pct = done / total * 100
            sys.stdout.write(
                f"\r  {MG('Phase 3')} [{done}/{total}] {pct:5.1f}% | "
                f"{elapsed:5.0f}s elapsed, ~{eta:4.0f}s remaining"
                f"{' ' * 10}"
            )
            sys.stdout.flush()

    print(f"\n  {BGR('✅')} Phase 3 done: {total} backtests in {time.time() - t0:.1f}s")

    # ── Aggregate: find best strategy per coin ──
    best_per_coin = {}
    best_per_strategy = {}
    all_trades = []

    for sym, tests in bt_data.items():
        best_score = -1
        best_key = None
        best_result = None
        for (strat, iv), result in tests.items():
            if result is None:
                continue
            sc = compute_backtest_score(result)
            tag = FH_TAG.get(strat, "TR")

            all_trades.append({
                "symbol": sym, "strategy": strat, "interval": iv,
                "tag": tag, "score": round(sc, 2),
                **result,
            })

            if sc > best_score:
                best_score = sc
                best_key = (strat, iv)
                best_result = result

            if strat not in best_per_strategy or result["ret"] > best_per_strategy[strat]["ret"]:
                best_per_strategy[strat] = {
                    "sym": sym, "iv": iv, **result,
                }

        if best_key:
            best_per_coin[sym] = {
                "strategy": best_key[0], "interval": best_key[1],
                "score": round(best_score, 2), **best_result,
            }

    # Sort all trades by backtest score
    all_trades.sort(key=lambda x: x["score"], reverse=True)

    return bt_data, best_per_coin, best_per_strategy, all_trades


# ════════════════════════════════════════════════════════════════════
# DISPLAY FUNCTIONS
# ════════════════════════════════════════════════════════════════════

def fmt_score(s):
    if s >= 70:
        return BGR(f"{s:.1f}")
    if s >= 55:
        return GR(f"{s:.1f}")
    if s >= 40:
        return YL(f"{s:.1f}")
    return RD(f"{s:.1f}")


def fmt_dir(d):
    if d == "STRONG_BUY":
        return BGR("▲▲ S.BUY")
    if d == "BUY":
        return GR("▲ BUY  ")
    if d == "STRONG_SELL":
        return BRD("▼▼ S.SELL")
    if d == "SELL":
        return RD("▼ SELL ")
    return YL("● MIXED ")


def fmt_hurst(h):
    if h > 0.55:
        return GR(f"{h:.3f} Trend")
    if h > 0.45:
        return YL(f"{h:.3f} Random")
    if h > 0:
        return RD(f"{h:.3f} MeanRev")
    return DM("N/A")


def fmt_ret(r):
    if r > 10:
        return BGR(f"{r:+.2f}%")
    if r > 0:
        return GR(f"{r:+.2f}%")
    if r > -10:
        return RD(f"{r:+.2f}%")
    return BRD(f"{r:+.2f}%")


def fmt_wr(w):
    if w >= 60:
        return GR(f"{w:.0f}%")
    if w >= 45:
        return YL(f"{w:.0f}%")
    return RD(f"{w:.0f}%")


def display_phase1_summary(ranked):
    """Display Phase 1 quick scan top 30."""
    print()
    print(f"  {BOLD(LINE)}")
    print(f"  {BOLD('PHASE 1 RESULTS: Quick Top 30 (1h + 4h)')}")
    print(f"  {BOLD(LINE)}")
    print()
    print(f"  {'#':>3}  {'Symbol':15s}  {'Score':>10s}  {'1h':>8s}  {'4h':>8s}  {'Price':>14s}")
    print(f"  {'─'*3}  {'─'*15}  {'─'*10}  {'─'*8}  {'─'*8}  {'─'*14}")

    for i, (sym, d) in enumerate(ranked[:30]):
        td = d.get("tf_data", {})
        price = 0
        for tf in ("1h", "4h"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break
        s1h = fmt_score(td["1h"][0]) if "1h" in td else DM("  —")
        s4h = fmt_score(td["4h"][0]) if "4h" in td else DM("  —")
        print(
            f"  {i+1:>3}  {sym:15s}  {fmt_score(d['score']):>18s}  {s1h:>14s}  {s4h:>14s}  ${price:>12,.4f}"
        )
    print()


def display_deep_table(deep):
    """Display Phase 2 deep results table."""
    print()
    print(f"  {BOLD(SEP)}")
    print(f"  {BOLD('🔥 PHASE 2: DEEP ANALYSIS — Top 30 (4 Timeframes)')}")
    print(f"  {BOLD(SEP)}")
    print()

    hdr = (
        f"  {'#':>3}  {'Symbol':15s}  {'Price':>12s}  {'Score':>8s}  "
        f"{'15m':>6s}  {'1h':>6s}  {'4h':>6s}  {'1d':>6s}  "
        f"{'Hurst(4h)':>14s}  {'Regime':>10s}  {'Signal':20s}"
    )
    print(hdr)
    print(f"  {'─'*3}  {'─'*15}  {'─'*12}  {'─'*8}  {'─'*6}  {'─'*6}  {'─'*6}  {'─'*6}  {'─'*14}  {'─'*10}  {'─'*20}")

    for i, (sym, d) in enumerate(deep):
        td = d.get("tf_data", {})
        price = 0
        for tf in ("15m", "1h", "4h", "1d"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break

        scores = {tf: td[tf][0] for tf in ("15m", "1h", "4h", "1d") if tf in td}
        s15 = fmt_score(scores["15m"]) if "15m" in scores else DM("  —")
        s1h = fmt_score(scores["1h"]) if "1h" in scores else DM("  —")
        s4h = fmt_score(scores["4h"]) if "4h" in scores else DM("  —")
        s1d = fmt_score(scores["1d"]) if "1d" in scores else DM("  —")

        hurst_4h = td.get("4h", (0, {}))[1].get("hurst", 0)
        regime_4h = td.get("4h", (0, {}))[1].get("regime", "?")

        print(
            f"  {i+1:>3}  {sym:15s}  ${price:>10,.4f}  {fmt_score(d['score']):>14s}  "
            f"{s15:>14s}  {s1h:>14s}  {s4h:>14s}  {s1d:>14s}  "
            f"{fmt_hurst(hurst_4h):>22s}  {regime_4h:>10s}  {fmt_dir(d.get('confluence', ''))}"
        )


def display_top10_detail(deep, bt_data=None, best_per_coin=None):
    """Display detailed breakdown of top 10."""
    print()
    print(f"  {BOLD(SEP)}")
    print(f"  {BOLD('💎 CHI TIẾT TOP 10 + BACKTEST TỐT NHẤT')}")
    print(f"  {BOLD(SEP)}")

    for rank, (sym, d) in enumerate(deep[:10]):
        td = d.get("tf_data", {})
        price = 0
        for tf in ("15m", "1h", "4h", "1d"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break
        conf = d.get("confluence", "MIXED")

        print()
        print(f"  ┌{'─'*116}┐")
        print(
            f"  │ #{rank+1}  {BOLD(sym):15s}  Score: {fmt_score(d['score'])}  "
            f"Signal: {fmt_dir(conf)}  Price: ${price:,.4f}"
        )
        print(f"  └{'─'*116}┘")

        for tf in ("15m", "1h", "4h", "1d"):
            if tf not in td:
                continue
            sc, det = td[tf]
            hurst = det.get("hurst", 0)
            regime = det.get("regime", "?")
            lag = det.get("lag", 0.5)
            cmo = det.get("cmo", 0)
            rsi = det.get("rsi", 50)
            bb = det.get("bb", 0.5)
            signals = det.get("signals", [])
            sr = det.get("sr", {})
            alma = det.get("alma", "")
            ss = det.get("ss", "")

            print(f"    │")
            print(
                f"    ├── [{tf:>3s}] {fmt_score(sc)}  "
                f"Hurst: {fmt_hurst(hurst)}  Regime: {regime}"
            )
            print(
                f"    │   RSI:{rsi:.0f}  LagRSI:{lag:.3f}  CMO:{cmo:.1f}  BB%B:{bb:.2f}  "
                f"ALMA:{'🟢' if 'Bullish' in alma else '🔴' if 'Bearish' in alma else '⚪'}  "
                f"SS:{'🟢' if 'positive' in ss.lower() else '🔴'}"
            )

            buys = [s for s in signals if s["direction"] == "BUY"]
            sells = [s for s in signals if s["direction"] == "SELL"]
            if buys:
                bstr = ", ".join(f"{s['name']}({s['confidence']}%)" for s in buys[:5])
                print(f"    │   {GR('🟢 Buy')}: {bstr}")
            if sells:
                sstr = ", ".join(f"{s['name']}({s['confidence']}%)" for s in sells[:5])
                print(f"    │   {RD('🔴 Sell')}: {sstr}")

            sups = sr.get("supports", [])
            ress = sr.get("resistances", [])
            if sups:
                print(f"    │   Support: {', '.join(f'${s:,.2f}' for s in sups[:3])}")
            if ress:
                print(f"    │   Resistance: {', '.join(f'${s:,.2f}' for s in ress[:3])}")

        # Best backtest
        if best_per_coin and sym in best_per_coin:
            bt = best_per_coin[sym]
            tag = FH_TAG.get(bt["strategy"], "TR")
            print(f"    │")
            print(
                f"    ├── {BW('📊 Best Backtest')}: [{tag}] {bt['strategy']} ({bt['interval']})  "
                f"Return: {fmt_ret(bt['ret'])}  WR: {fmt_wr(bt['wr'])}  "
                f"Sharpe: {bt['sh']:.2f}  MaxDD: {bt['mdd']:.1f}%  Trades: {bt['tr']}"
            )

        print(f"    │")
        if "BUY" in conf:
            print(f"    ├── {BGR('💡 LONG ↑')}")
            if price > 0:
                sl = price * 0.97
                tp1 = price * 1.03
                tp2 = price * 1.06
                print(f"    │   Entry:${price:,.4f} SL:${sl:,.4f} TP1:${tp1:,.4f} TP2:${tp2:,.4f}")
        elif "SELL" in conf:
            print(f"    ├── {BRD('💡 SHORT ↓')}")
            if price > 0:
                sl = price * 1.03
                tp1 = price * 0.97
                tp2 = price * 0.94
                print(f"    │   Entry:${price:,.4f} SL:${sl:,.4f} TP1:${tp1:,.4f} TP2:${tp2:,.4f}")
        else:
            print(f"    ├── {YL('💡 WAIT — Không có tín hiệu rõ ràng')}")
        print(f"    └{'─'*116}┘")


def display_backtest_matrix(bt_data, best_per_coin, best_per_strategy, all_trades):
    """Display Phase 3 backtest results."""
    print()
    print(f"  {BOLD(SEP)}")
    print(f"  {BOLD('📊 PHASE 3: MULTI-STRATEGY BACKTEST MATRIX')}")
    print(f"  {BOLD(f'13 Strategies × 3 Timeframes per coin')}")
    print(f"  {BOLD(SEP)}")

    # ── Best trade opportunities (top 20) ──
    print()
    print(f"  {BOLD('🏆 TOP 20 GIAO DICH TỐT NHẤT (Backtest Score)')}")
    print(f"  {BOLD(LINE)}")
    print()

    fmt = "  {:>3}  {:12s} {:<24s} {:<4s} {:>10s} {:>6s} {:>8s} {:>7s} {:>6s} {:>6s}"
    print(fmt.format(
        "#", "Symbol", "Strategy", "TF", "Return", "WR%", "Sharpe", "MaxDD%", "PF", "Trades"
    ))
    print(f"  {'─'*3}  {'─'*12} {'─'*24} {'─'*4} {'─'*10} {'─'*6} {'─'*8} {'─'*7} {'─'*6} {'─'*6}")

    for i, t in enumerate(all_trades[:20]):
        tag = FH_TAG.get(t["strategy"], "TR")
        print(fmt.format(
            i + 1,
            t["symbol"],
            f"[{tag}] {t['strategy']}",
            t["interval"],
            fmt_ret(t["ret"]),
            fmt_wr(t["wr"]),
            f"{t['sh']:.2f}",
            f"{t['mdd']:.1f}",
            f"{t['pf']:.2f}" if t["pf"] else "—",
            str(t["tr"]),
        ))

    # ── Best strategy per coin ──
    print()
    print(f"  {BOLD(LINE)}")
    print(f"  {BOLD('📈 BEST STRATEGY PER COIN')}")
    print(f"  {BOLD(LINE)}")
    print()

    sorted_coins = sorted(best_per_coin.items(), key=lambda x: x[1]["ret"], reverse=True)
    for sym, b in sorted_coins:
        tag = FH_TAG.get(b["strategy"], "TR")
        marker = BGR("[+]") if b["ret"] > 0 else BRD("[-]")
        print(
            f"  {marker} {tag} {sym:13s} → [{b['interval']}] {b['strategy']:<26s}  "
            f"Ret: {fmt_ret(b['ret'])}  WR: {fmt_wr(b['wr'])}  Sharpe: {b['sh']:.2f}  "
            f"DD: {b['mdd']:.1f}%  Trades: {b['tr']}"
        )

    # ── Best coin per strategy ──
    print()
    print(f"  {BOLD(LINE)}")
    print(f"  {BOLD('🧠 BEST COIN PER STRATEGY')}")
    print(f"  {BOLD(LINE)}")
    print()

    for s in ALL_STRATEGIES:
        if s in best_per_strategy:
            b = best_per_strategy[s]
            tag = FH_TAG.get(s, "TR")
            marker = BGR("[+]") if b["ret"] > 0 else BRD("[-]")
            print(
                f"  {marker} {tag} {s:<28s} → {b['sym']:13s} [{b['iv']}]  "
                f"Ret: {fmt_ret(b['ret'])}  WR: {fmt_wr(b['wr'])}  Sharpe: {b['sh']:.2f}"
            )

    # ── Traditional vs FH comparison ──
    print()
    print(f"  {BOLD(LINE)}")
    print(f"  {BOLD('⚔️  TRADITIONAL vs FINANCIAL-HACKER')}")
    print(f"  {BOLD(LINE)}")
    print()

    tr_trades = [t for t in all_trades if t["tag"] == "TR"]
    fh_trades = [t for t in all_trades if t["tag"] == "FH"]

    def avg_stat(trades, key):
        vals = [t[key] for t in trades if t[key] is not None]
        return sum(vals) / len(vals) if vals else 0

    def positive_pct(trades):
        profitable = sum(1 for t in trades if t["ret"] > 0)
        return profitable / len(trades) * 100 if trades else 0

    print(f"  {'Metric':20s}  {'Traditional':>15s}  {'FH Strategies':>15s}  {'Winner':>10s}")
    print(f"  {'─'*20}  {'─'*15}  {'─'*15}  {'─'*10}")

    metrics = [
        ("Avg Return", "ret"),
        ("Avg Win Rate", "wr"),
        ("Avg Sharpe", "sh"),
        ("Avg MaxDD", "mdd"),
    ]

    for label, key in metrics:
        tr_val = avg_stat(tr_trades, key)
        fh_val = avg_stat(fh_trades, key)
        if key in ("ret", "wr", "sh"):
            winner = "FH ✅" if fh_val > tr_val else "TR ✅"
        else:
            winner = "FH ✅" if fh_val < tr_val else "TR ✅"
        print(f"  {label:20s}  {tr_val:>14.2f}  {fh_val:>14.2f}  {winner:>10s}")

    tr_pos = positive_pct(tr_trades)
    fh_pos = positive_pct(fh_trades)
    winner = "FH ✅" if fh_pos > tr_pos else "TR ✅"
    print(f"  {'% Profitable':20s}  {tr_pos:>13.1f}%  {fh_pos:>13.1f}%  {winner:>10s}")
    print(
        f"  {'Total tests':20s}  {len(tr_trades):>15d}  {len(fh_trades):>15d}  {'—':>10s}"
    )


def display_final_recommendations(deep, all_trades, best_per_coin):
    """Display final actionable recommendations."""
    print()
    print(f"  {BOLD(SEP)}")
    print(f"  {BOLD('🎯 KẾT LUẬN: GIAO DICH TỐT NHẤT HIỆN TẠI')}")
    print(f"  {BOLD(SEP)}")
    print()

    # Find coins with STRONG_BUY or BUY confluence AND positive backtest
    actionable = []
    for sym, d in deep:
        conf = d.get("confluence", "MIXED")
        td = d.get("tf_data", {})
        price = 0
        for tf in ("15m", "1h", "4h", "1d"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break

        bt_best = best_per_coin.get(sym)
        bt_ret = bt_best["ret"] if bt_best else 0
        bt_strat = bt_best["strategy"] if bt_best else "?"
        bt_iv = bt_best["interval"] if bt_best else "?"

        # Only include if confluence matches backtest direction
        is_good = False
        if "BUY" in conf and bt_ret > 0:
            is_good = True
        elif "SELL" in conf and bt_ret > 0:
            is_good = True

        actionable.append({
            "sym": sym, "conf": conf, "score": d["score"],
            "price": price, "bt_ret": bt_ret, "bt_strat": bt_strat,
            "bt_iv": bt_iv, "bt_wr": bt_best.get("wr", 0) if bt_best else 0,
            "bt_sh": bt_best.get("sh", 0) if bt_best else 0,
            "bt_mdd": bt_best.get("mdd", 0) if bt_best else 0,
            "is_good": is_good,
        })

    # Sort: good trades first, then by score
    actionable.sort(key=lambda x: (not x["is_good"], -x["score"]))

    # ── TOP LONGS ──
    longs = [a for a in actionable if "BUY" in a["conf"] and a["is_good"]]
    if longs:
        print(f"  {BGR('🟢 TOP LONG CANDIDATES')}")
        print(f"  {BOLD(LINE)}")
        for i, a in enumerate(longs[:8]):
            tag = FH_TAG.get(a["bt_strat"], "TR")
            print(
                f"    {i+1}. {BOLD(a['sym']):15s}  Score: {fmt_score(a['score'])}  "
                f"{fmt_dir(a['conf'])}  ${a['price']:,.4f}"
            )
            print(
                f"       Best: [{tag}] {a['bt_strat']} ({a['bt_iv']})  "
                f"Ret: {fmt_ret(a['bt_ret'])}  WR: {fmt_wr(a['bt_wr'])}  "
                f"Sharpe: {a['bt_sh']:.2f}"
            )
            if a["price"] > 0 and "BUY" in a["conf"]:
                sl = a["price"] * 0.97
                tp1 = a["price"] * 1.03
                tp2 = a["price"] * 1.06
                print(
                    f"       💰 Entry: ${a['price']:,.4f} | SL: ${sl:,.4f} | "
                    f"TP1: ${tp1:,.4f} | TP2: ${tp2:,.4f}"
                )
            print()

    # ── TOP SHORTS ──
    shorts = [a for a in actionable if "SELL" in a["conf"] and a["is_good"]]
    if shorts:
        print(f"  {BRD('🔴 TOP SHORT CANDIDATES')}")
        print(f"  {BOLD(LINE)}")
        for i, a in enumerate(shorts[:5]):
            tag = FH_TAG.get(a["bt_strat"], "TR")
            print(
                f"    {i+1}. {BOLD(a['sym']):15s}  Score: {fmt_score(a['score'])}  "
                f"{fmt_dir(a['conf'])}  ${a['price']:,.4f}"
            )
            print(
                f"       Best: [{tag}] {a['bt_strat']} ({a['bt_iv']})  "
                f"Ret: {fmt_ret(a['bt_ret'])}  WR: {fmt_wr(a['bt_wr'])}  "
                f"Sharpe: {a['bt_sh']:.2f}"
            )
            if a["price"] > 0 and "SELL" in a["conf"]:
                sl = a["price"] * 1.03
                tp1 = a["price"] * 0.97
                tp2 = a["price"] * 0.94
                print(
                    f"       💰 Entry: ${a['price']:,.4f} | SL: ${sl:,.4f} | "
                    f"TP1: ${tp1:,.4f} | TP2: ${tp2:,.4f}"
                )
            print()

    # ── REGIME SUMMARY ──
    print(f"  {BOLD(LINE)}")
    print(f"  {BOLD('📋 MARKET REGIME SUMMARY')}")
    print(f"  {BOLD(LINE)}")
    print()

    regimes = {}
    for sym, d in deep:
        td = d.get("tf_data", {})
        r = td.get("4h", (0, {}))[1].get("regime", "UNKNOWN")
        regimes[r] = regimes.get(r, 0) + 1

    for r, cnt in sorted(regimes.items(), key=lambda x: -x[1]):
        print(f"    {r:>12s}: {cnt:3d} coins  → Best strategies: {', '.join(REGIME_BEST_STRATEGY.get(r, ['fh_composite']))}")

    n_buys = sum(1 for a in actionable if "BUY" in a["conf"])
    n_sells = sum(1 for a in actionable if "SELL" in a["conf"])
    n_mixed = sum(1 for a in actionable if a["conf"] == "MIXED")
    n_good = sum(1 for a in actionable if a["is_good"])

    print()
    print(
        f"    Total: {len(actionable)} coins | "
        f"{GR('BUY')}: {n_buys} | {RD('SELL')}: {n_sells} | {YL('MIXED')}: {n_mixed} | "
        f"{BGR('Actionable')}: {n_good}"
    )

    # ── TOP 5 ALL-ROUND BEST ──
    print()
    print(f"  {BOLD(LINE)}")
    print(f"  {BOLD('⭐ TOP 5 ALL-ROUND BEST (Score + Backtest)')}")
    print(f"  {BOLD(LINE)}")
    print()
    for i, a in enumerate(actionable[:5]):
        emoji = "🟢" if "BUY" in a["conf"] else "🔴" if "SELL" in a["conf"] else "⚪"
        print(
            f"    {i+1}. {emoji} {BOLD(a['sym']):15s}  "
            f"Composite: {fmt_score(a['score'])}  {fmt_dir(a['conf'])}  | "
            f"Backtest: {fmt_ret(a['bt_ret'])} (WR:{a['bt_wr']:.0f}% Sharpe:{a['bt_sh']:.2f})"
        )


# ════════════════════════════════════════════════════════════════════
# SAVE RESULTS
# ════════════════════════════════════════════════════════════════════

def save_results(deep, all_trades, best_per_coin, best_per_strategy, sentiment_text=""):
    """Save comprehensive results to JSON."""
    rdir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(rdir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    rpt_path = os.path.join(rdir, f"full_scan_{ts}.json")

    # Build output
    output = {
        "timestamp": datetime.now().isoformat(),
        "version": "4.0",
        "sentiment": sentiment_text[:200] if sentiment_text else "",
        "coins_analyzed": len(deep),
        "strategies_tested": ALL_STRATEGIES,
        "backtest_intervals": BACKTEST_INTERVALS,
        "results": [],
        "best_trades": [],
        "best_per_strategy": {},
    }

    for sym, d in deep:
        td = d.get("tf_data", {})
        price = 0
        for tf in ("15m", "1h", "4h", "1d"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break

        bt_best = best_per_coin.get(sym, {})
        entry = {
            "rank": len(output["results"]) + 1,
            "symbol": sym,
            "price": price,
            "composite_score": round(d.get("score", 0), 2),
            "tf_scores": {
                tf: round(td[tf][0], 2)
                for tf in ("15m", "1h", "4h", "1d") if tf in td
            },
            "tf_regimes": {
                tf: td[tf][1].get("regime", "?")
                for tf in ("15m", "1h", "4h", "1d") if tf in td
            },
            "tf_hurst": {
                tf: td[tf][1].get("hurst", 0)
                for tf in ("15m", "1h", "4h", "1d") if tf in td
            },
            "confluence": d.get("confluence", ""),
            "directions": d.get("directions", {}),
            "best_backtest": {
                "strategy": bt_best.get("strategy", ""),
                "interval": bt_best.get("interval", ""),
                "return_pct": bt_best.get("ret", 0),
                "win_rate": bt_best.get("wr", 0),
                "sharpe": bt_best.get("sh", 0),
                "max_dd": bt_best.get("mdd", 0),
                "trades": bt_best.get("tr", 0),
                "score": bt_best.get("score", 0),
            } if bt_best else None,
        }
        output["results"].append(entry)

    # Top trades
    for t in all_trades[:30]:
        output["best_trades"].append({
            "symbol": t["symbol"],
            "strategy": t["strategy"],
            "tag": t["tag"],
            "interval": t["interval"],
            "return_pct": t["ret"],
            "win_rate": t["wr"],
            "sharpe": t["sh"],
            "max_dd": t["mdd"],
            "profit_factor": t.get("pf", 0),
            "trades": t["tr"],
            "score": t["score"],
        })

    # Best per strategy
    for strat, b in best_per_strategy.items():
        output["best_per_strategy"][strat] = {
            "symbol": b["sym"],
            "interval": b["iv"],
            "return_pct": b["ret"],
            "win_rate": b["wr"],
            "sharpe": b["sh"],
        }

    with open(rpt_path, "w") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    return rpt_path


# ════════════════════════════════════════════════════════════════════
# GET TOP 100 COINS
# ════════════════════════════════════════════════════════════════════

def fetch_top_coins(limit=100):
    """Fetch top coins from Binance via MCP."""
    cli = get_cli()
    top_text = cli.call("get_top_crypto", {"limit": limit, "sort_by": "volume"})
    put_cli(cli)

    symbols = []
    for m in re.finditer(r"(?:\d+\.\s+)([A-Z]+USDT)", top_text):
        sym = m.group(1)
        if sym not in SKIP_SYMBOLS and sym not in symbols:
            symbols.append(sym)

    if len(symbols) < 20:
        for m in re.finditer(r"([A-Z]{2,}USDT)", top_text):
            sym = m.group(1)
            if sym not in SKIP_SYMBOLS and sym not in symbols:
                symbols.append(sym)

    # Fallback
    for s in FALLBACK_COINS:
        if s not in symbols:
            symbols.append(s)

    return symbols[:limit]


# ════════════════════════════════════════════════════════════════════
# MAIN
# ════════════════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(description="BonBo Top 100 Full Scan + Backtest")
    parser.add_argument("--quick", action="store_true", help="Quick mode: 20 coins, skip some strategies")
    parser.add_argument("--top-n", type=int, default=100, help="Number of coins to analyze")
    parser.add_argument("--deep-n", type=int, default=30, help="Deep analysis top N from Phase 1")
    parser.add_argument("--bt-n", type=int, default=20, help="Backtest top N from Phase 2")
    parser.add_argument("--no-backtest", action="store_true", help="Skip Phase 3 backtest")
    parser.add_argument("--coins", nargs="+", help="Specific coins to analyze")
    parser.add_argument("--workers", type=int, default=NUM_WORKERS, help="Parallel workers")
    args = parser.parse_args()

    # Quick mode overrides
    if args.quick:
        args.top_n = min(args.top_n, 20)
        args.deep_n = min(args.deep_n, 15)
        args.bt_n = min(args.bt_n, 10)

    num_workers = args.workers

    print()
    print(BOLD(SEP))
    print(BOLD("  🔥 BONBO TOP 100 FULL SCAN + MULTI-STRATEGY BACKTEST v4.0"))
    print(
        f"  📅 {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} | "
        f"Workers: {num_workers} | "
        f"Coins: {args.top_n} → Deep: {args.deep_n} → Backtest: {args.bt_n}"
    )
    print(BOLD(SEP))
    print()

    # Init MCP pool
    print(f"  {CY('⚙️')} Initializing MCP client pool ({num_workers} workers)...")
    init_pool(num_workers)

    # ── Sentiment ──
    print(f"  {CY('📊')} Fetching market sentiment...")
    cli = get_cli()
    fg_text = cli.call("get_fear_greed_index", {"history": 1})
    put_cli(cli)
    fg_m = re.search(r"(\d+)/100", fg_text)
    fg_val = fg_m.group(1) if fg_m else "?"
    fg_label = (
        "Extreme Fear" if "Extreme Fear" in fg_text
        else "Fear" if "Fear" in fg_text and "Greed" not in fg_text
        else "Greed" if "Greed" in fg_text and "Extreme" not in fg_text
        else "Extreme Greed" if "Extreme Greed" in fg_text
        else "Neutral"
    )
    print(f"  Sentiment: Fear & Greed = {BOLD(fg_val)}/100 ({fg_label})")
    print()

    # ── Get coins ──
    if args.coins:
        symbols = [s.upper() for s in args.coins if s.upper() not in SKIP_SYMBOLS]
    else:
        symbols = fetch_top_coins(args.top_n)

    print(f"  {CY('📋')} Phase 0: {len(symbols)} coins (filtered stablecoins)")
    print()

    # ═══════════════════════════════════════════════════════════════
    # PHASE 1: QUICK SCAN
    # ═══════════════════════════════════════════════════════════════
    print(BOLD(f"  {LINE}"))
    print(BOLD(f"  PHASE 1: QUICK SCAN (1h + 4h) — {len(symbols)} coins"))
    print()
    ranked = phase1_quick_scan(symbols)
    display_phase1_summary(ranked)

    # ═══════════════════════════════════════════════════════════════
    # PHASE 2: DEEP ANALYSIS
    # ═══════════════════════════════════════════════════════════════
    print(BOLD(f"  {LINE}"))
    print(BOLD(f"  PHASE 2: DEEP ANALYSIS (15m+1h+4h+1d) — top {args.deep_n}"))
    print()
    deep = phase2_deep_analysis(ranked, top_n=args.deep_n)
    display_deep_table(deep)

    # ═══════════════════════════════════════════════════════════════
    # PHASE 3: BACKTEST
    # ═══════════════════════════════════════════════════════════════
    bt_data = {}
    best_per_coin = {}
    best_per_strategy = {}
    all_trades = []

    if not args.no_backtest:
        n_strats = len(ALL_STRATEGIES)
        n_tfs = len(BACKTEST_INTERVALS)
        total_bt = args.bt_n * n_strats * n_tfs
        print()
        print(BOLD(f"  {LINE}"))
        print(
            BOLD(
                f"  PHASE 3: MULTI-STRATEGY BACKTEST — "
                f"top {args.bt_n} × {n_strats} strategies × {n_tfs} TFs = {total_bt} tests"
            )
        )
        print()
        bt_data, best_per_coin, best_per_strategy, all_trades = phase3_backtest(
            deep, top_n=args.bt_n
        )
        display_backtest_matrix(bt_data, best_per_coin, best_per_strategy, all_trades)

    # ═══════════════════════════════════════════════════════════════
    # TOP 10 DETAIL + RECOMMENDATIONS
    # ═══════════════════════════════════════════════════════════════
    display_top10_detail(deep, bt_data, best_per_coin)
    display_final_recommendations(deep, all_trades, best_per_coin)

    # ═══════════════════════════════════════════════════════════════
    # SAVE
    # ═══════════════════════════════════════════════════════════════
    rpt_path = save_results(
        deep, all_trades, best_per_coin, best_per_strategy,
        sentiment_text=f"{fg_val}/100 ({fg_label})",
    )
    print()
    print(f"  📁 Report saved: {BOLD(rpt_path)}")
    print()
    print(BOLD(SEP))
    print()


if __name__ == "__main__":
    main()
