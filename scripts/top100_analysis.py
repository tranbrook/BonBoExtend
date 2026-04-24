#!/usr/bin/env python3
"""
BonBo Top 100 Multi-Timeframe Analysis v3 — 2-Phase Architecture

Phase 1: Quick scan 100 coins (1h+4h, parallel 4 workers) → rank → top 30
Phase 2: Deep analysis top 30 (15m+1h+4h+1d) → detailed report
"""

import json, os, re, subprocess, sys, time, threading, queue
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor, as_completed

MCP_BIN = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                        "target/release/bonbo-extend-mcp")

NUM_WORKERS = 4

SKIP_SYMBOLS = {
    "USDCUSDT", "USDTUSDT", "BUSDUSDT", "DAIUSDT", "TUSDUSDT",
    "FDUSDUSDT", "RLUSDUSDT", "EURUSDT", "PAXGUSDT", "USDPUSDT",
    "IDRTUSDT", "BIDRUSDT", "USDSBUSDT", "USDSUSDT",
    "PYUSDUSDT", "FIRSTUSDT", "LDBNBUSDT", "LDBTCUSDT",
    "BTCDOMUSDT", "DEFIUSDT", "NFTUSDT",
}

# ── MCP Client Pool ───────────────────────────────────────────────

class MCPClient:
    def __init__(self, bin_path):
        self.bin_path = bin_path
        self._lock = threading.Lock()
        self._seq = 0

    def call(self, tool, args=None, timeout=30):
        with self._lock:
            self._seq += 1
            if args is None:
                args = {}
            init_req = json.dumps({
                "jsonrpc": "2.0", "method": "initialize",
                "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                           "clientInfo": {"name": "b100", "version": "3.0"}},
                "id": "0"
            })
            call_req = json.dumps({
                "jsonrpc": "2.0", "method": "tools/call",
                "params": {"name": tool, "arguments": args},
                "id": str(self._seq)
            })
            stdin_data = init_req + "\n" + call_req + "\n"
            try:
                p = subprocess.run([self.bin_path], input=stdin_data,
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
            except Exception:
                pass
            return ""


clients = queue.Queue()
for _ in range(NUM_WORKERS):
    clients.put(MCPClient(MCP_BIN))


def get_cli():
    return clients.get()

def put_cli(c):
    clients.put(c)


# ── Parsers ────────────────────────────────────────────────────────

def p_price(t):
    m = re.search(r'\$([0-9,.]+)', t)
    return float(m.group(1).replace(",", "")) if m else 0.0

def p_hurst(t):
    m = re.search(r'Hurst[^:]*:\s*([0-9.]+)', t)
    return float(m.group(1)) if m else 0.0

def p_lag(t):
    m = re.search(r'LaguerreRSI[^:]*:\s*([0-9.]+)', t)
    return float(m.group(1)) if m else 0.5

def p_cmo(t):
    m = re.search(r'CMO[^:]*:\s*([-0-9.]+)', t)
    return float(m.group(1)) if m else 0.0

def p_rsi(t):
    m = re.search(r'RSI[^:]*:\s*([0-9.]+)', t)
    return float(m.group(1)) if m else 50.0

def p_bb(t):
    m = re.search(r'%B=([0-9.]+)', t)
    return float(m.group(1)) if m else 0.5

def p_signals(t):
    sigs = []
    for m in re.finditer(r'(🟢|🔴)\s+\*\*(Buy|Sell)\*\*\s+\[([^\]]+)\]\s+\(([0-9]+)%\)', t):
        sigs.append({"direction": m.group(2).upper(), "name": m.group(3), "confidence": int(m.group(4))})
    return sigs

def p_regime(t):
    tl = t.lower()
    h = 0.0
    m = re.search(r'Hurst[^:]*:\s*([0-9.]+)', t)
    if m:
        h = float(m.group(1))
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
    for m in re.finditer(r'(S\d?):\s*\$([0-9,.]+)', t):
        sup.append(float(m.group(2).replace(",", "")))
    for m in re.finditer(r'(R\d?):\s*\$([0-9,.]+)', t):
        res.append(float(m.group(2).replace(",", "")))
    return {"supports": sup, "resistances": res}

def p_alma(t):
    m = re.search(r'(ALMA.*?)(?:\n|$)', t)
    return m.group(0).strip() if m else ""

def p_ss(t):
    m = re.search(r'(SuperSmoother.*?)(?:\n|$)', t)
    return m.group(0).strip() if m else ""


# ── Scoring ────────────────────────────────────────────────────────

def score_tf(ind, sig, reg, sr=""):
    price = p_price(ind)
    hurst = p_hurst(ind) or p_hurst(reg)
    lag = p_lag(ind)
    cmo = p_cmo(ind)
    rsi = p_rsi(ind)
    bb = p_bb(ind)
    alma = p_alma(ind)
    ss = p_ss(ind)
    signals = p_signals(sig)
    regime = p_regime(reg)
    srlvls = p_sr(sr)

    d = {"price": price, "hurst": hurst, "regime": regime["regime"],
         "lag": lag, "cmo": cmo, "rsi": rsi, "bb": bb,
         "alma": alma, "ss": ss, "signals": signals, "sr": srlvls}

    tot, wt = 0.0, 0.0

    # Hurst 0-20
    h = 20.0 if hurst > 0.6 else 16.0 if hurst > 0.55 else 10.0 if hurst > 0.5 else 4.0 if hurst > 0 else 5.0
    tot += h; wt += 20

    # Signals 0-30
    buys = [s for s in signals if s["direction"] == "BUY"]
    sells = [s for s in signals if s["direction"] == "SELL"]
    ns = len(signals) if signals else 1
    if not signals:
        sv = 5.0
    elif len(buys) > len(sells):
        avg = sum(s["confidence"] for s in buys) / len(buys)
        sv = min(10 + (len(buys) / ns) * 15 + (avg / 100) * 5, 30.0)
    elif len(sells) > len(buys):
        avg = sum(s["confidence"] for s in sells) / len(sells)
        sv = min(5 + (len(sells) / ns) * 10 + (avg / 100) * 5, 30.0)
    else:
        sv = 8.0
    tot += sv; wt += 30

    # LagRSI 0-15
    rv = 15.0 if lag < 0.2 else 11.0 if lag < 0.35 else 7.0 if lag < 0.65 else 5.0 if lag < 0.8 else 3.0
    tot += rv; wt += 15

    # CMO 0-15
    cv = 13.0 if abs(cmo) > 40 else 9.0 if abs(cmo) > 20 else 6.0 if abs(cmo) > 10 else 3.0
    tot += cv; wt += 15

    # Trend 0-20
    ab = "Bullish" in alma or "🟢" in alma
    sb = "positive" in ss.lower() or "🟢" in ss
    tv = 20.0 if ab and sb else 13.0 if ab or sb else 6.0
    tot += tv; wt += 20

    return min((tot / wt * 100) if wt > 0 else 0, 100.0), d


# ── Phase 1: Quick Scan ───────────────────────────────────────────

def phase1(symbols):
    results = {}
    total = len(symbols)

    def work(sym):
        cli = get_cli()
        try:
            td = {}
            for tf in ("1h", "4h"):
                ind = cli.call("analyze_indicators", {"symbol": sym, "interval": tf, "limit": 200})
                sig = cli.call("get_trading_signals", {"symbol": sym, "interval": tf})
                reg = cli.call("detect_market_regime", {"symbol": sym, "interval": tf})
                sc, det = score_tf(ind, sig, reg)
                td[tf] = (sc, det)
            return sym, td
        finally:
            put_cli(cli)

    done = 0
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=NUM_WORKERS) as pool:
        futs = {pool.submit(work, s): s for s in symbols}
        for f in as_completed(futs):
            done += 1
            try:
                sym, td = f.result()
                w = {"1h": 0.45, "4h": 0.55}
                comp = sum(td[tf][0] * w.get(tf, 0.5) for tf in ("1h", "4h") if tf in td)
                results[sym] = {"tf_data": td, "score": comp}
            except Exception:
                results[futs[f]] = {"tf_data": {}, "score": 0}
            el = time.time() - t0
            eta = (el / done) * (total - done)
            print(f"\r  Phase1: [{done}/{total}] {el:.0f}s ETA:{eta:.0f}s   ", end="", flush=True)

    print(f"\n  ✅ Phase1: {total} coins in {time.time()-t0:.1f}s")
    return sorted(results.items(), key=lambda x: x[1]["score"], reverse=True)


# ── Phase 2: Deep ─────────────────────────────────────────────────

def phase2(ranked, top_n=30):
    targets = ranked[:top_n]
    all_tfs = ("15m", "1h", "4h", "1d")
    tf_w = {"15m": 0.10, "1h": 0.30, "4h": 0.35, "1d": 0.25}
    results = {}
    total = len(targets)

    def work(sym):
        cli = get_cli()
        try:
            td = {}
            for tf in all_tfs:
                ind = cli.call("analyze_indicators", {"symbol": sym, "interval": tf, "limit": 200})
                sig = cli.call("get_trading_signals", {"symbol": sym, "interval": tf})
                reg = cli.call("detect_market_regime", {"symbol": sym, "interval": tf})
                sr = cli.call("get_support_resistance", {"symbol": sym, "interval": tf})
                sc, det = score_tf(ind, sig, reg, sr)
                td[tf] = (sc, det)
            return sym, td
        finally:
            put_cli(cli)

    done = 0
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=NUM_WORKERS) as pool:
        futs = {pool.submit(work, s): s for s, _ in targets}
        for f in as_completed(futs):
            done += 1
            sym = futs[f]
            try:
                s, td = f.result()
                comp = sum(td[tf][0] * tf_w[tf] for tf in all_tfs if tf in td)
                dirs = {}
                for tf in all_tfs:
                    if tf in td:
                        buys = sum(1 for x in td[tf][1].get("signals", []) if x["direction"] == "BUY")
                        sells = sum(1 for x in td[tf][1].get("signals", []) if x["direction"] == "SELL")
                        dirs[tf] = "BUY" if buys > sells else "SELL" if sells > buys else "NEUTRAL"
                bc = sum(1 for v in dirs.values() if v == "BUY")
                sc = sum(1 for v in dirs.values() if v == "SELL")
                if bc >= 3:
                    conf = "STRONG_BUY"
                elif bc >= 2:
                    conf = "BUY"
                elif sc >= 3:
                    conf = "STRONG_SELL"
                elif sc >= 2:
                    conf = "SELL"
                else:
                    conf = "MIXED"
                results[s] = {"tf_data": td, "score": comp, "confluence": conf, "directions": dirs}
            except Exception:
                results[sym] = {"tf_data": {}, "score": 0, "confluence": "MIXED", "directions": {}}
            el = time.time() - t0
            eta = (el / done) * (total - done)
            print(f"\r  Phase2: [{done}/{total}] {el:.0f}s ETA:{eta:.0f}s   ", end="", flush=True)

    print(f"\n  ✅ Phase2: {total} coins in {time.time()-t0:.1f}s")
    return sorted(results.items(), key=lambda x: x[1]["score"], reverse=True)


# ── Display helpers ────────────────────────────────────────────────

def C(c, t): return f"\033[{c}m{t}\033[0m"
def BOLD(t): return C("1", t)
def GR(t): return C("32", t)
def BGR(t): return C("1;32", t)
def RD(t): return C("31", t)
def BRD(t): return C("1;31", t)
def YL(t): return C("33", t)
def DM(t): return C("2", t)

def fsc(s):
    if s >= 70: return BGR(f"{s:.1f}")
    if s >= 55: return GR(f"{s:.1f}")
    if s >= 40: return YL(f"{s:.1f}")
    return RD(f"{s:.1f}")

def fdir(d):
    if d == "STRONG_BUY": return BGR("▲▲ S.BUY")
    if d == "BUY": return GR("▲ BUY")
    if d == "STRONG_SELL": return BRD("▼▼ S.SELL")
    if d == "SELL": return RD("▼ SELL")
    return YL("● MIXED")

def fh(h):
    if h > 0.55: return GR(f"{h:.3f} Trend")
    if h > 0.45: return YL(f"{h:.3f} Random")
    if h > 0: return RD(f"{h:.3f} MeanRev")
    return DM("N/A")


# ── Main ───────────────────────────────────────────────────────────

def main():
    print()
    print(BOLD("=" * 110))
    print(BOLD("  🔥 BONBO TOP 100 MULTI-TIMEFRAME ANALYSIS v3.0"))
    print(f"  📅 {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} | Workers: {NUM_WORKERS} | 2-Phase")
    print(BOLD("=" * 110))
    print()

    # Sentiment
    cli = get_cli()
    fg_text = cli.call("get_fear_greed_index", {"history": 1})
    put_cli(cli)
    fg_m = re.search(r'(\d+)/100', fg_text)
    fg_val = fg_m.group(1) if fg_m else "?"
    fg_label = "Fear" if "Fear" in fg_text and "Greed" not in fg_text else \
               "Greed" if "Greed" in fg_text else "Neutral"
    print(DM("━" * 110))
    print(f"  Sentiment: Fear&Greed = {fg_val}/100 ({fg_label})")
    print()

    # Get coins
    cli = get_cli()
    top_text = cli.call("get_top_crypto", {"limit": 100, "sort_by": "volume"})
    put_cli(cli)

    symbols = []
    for m in re.finditer(r'(?:\d+\.\s+)([A-Z]+USDT)', top_text):
        sym = m.group(1)
        if sym not in SKIP_SYMBOLS and sym not in symbols:
            symbols.append(sym)
    if len(symbols) < 20:
        for m in re.finditer(r'([A-Z]{2,}USDT)', top_text):
            sym = m.group(1)
            if sym not in SKIP_SYMBOLS and sym not in symbols:
                symbols.append(sym)

    FALLBACK = [
        "BTCUSDT","ETHUSDT","SOLUSDT","BNBUSDT","XRPUSDT","DOGEUSDT",
        "ADAUSDT","AVAXUSDT","LINKUSDT","DOTUSDT","NEARUSDT","APTUSDT",
        "ARBUSDT","UNIUSDT","AAVEUSDT","MKRUSDT","LTCUSDT","ATOMUSDT",
        "FILUSDT","OPUSDT","INJUSDT","SUIUSDT","SEIUSDT","FETUSDT",
        "RENDERUSDT","PEPEUSDT","WIFUSDT","ORDIUSDT","STXUSDT","TIAUSDT",
        "PENDLEUSDT","ENAUSDT","TONUSDT","KASUSDT","JUPUSDT","WLDUSDT",
        "CKBUSDT","COREUSDT","IMXUSDT","RUNEUSDT","APEUSDT","DYDXUSDT",
        "COMPUSDT","CRVUSDT","LDOUSDT","SNXUSDT","1INCHUSDT","GRTUSDT",
    ]
    for s in FALLBACK:
        if s not in symbols:
            symbols.append(s)
    symbols = symbols[:100]

    print(DM("━" * 110))
    print(f"  PHASE 0: {len(symbols)} coins (sau khi lọc stablecoins)")
    print()

    # Phase 1
    print(DM("━" * 110))
    print(BOLD("  PHASE 1: QUICK SCAN (1h+4h) — tất cả coins"))
    print()
    ranked = phase1(symbols)

    print()
    print(f"  Quick Top 30:")
    for i, (sym, d) in enumerate(ranked[:30]):
        price = 0
        for tf in ("1h", "4h"):
            p = d["tf_data"].get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break
        print(f"    {i+1:>3}. {sym:15s}  {fsc(d['score']):>12s}  ${price:>12,.4f}")
    print()

    # Phase 2
    print(DM("━" * 110))
    print(BOLD("  PHASE 2: DEEP ANALYSIS (15m+1h+4h+1d) — top 30"))
    print()
    deep = phase2(ranked, top_n=30)

    # ── Results Table ──
    print()
    print()
    print(BOLD("=" * 110))
    print(BOLD("  KẾT QUẢ — TOP 30 (4 TIMEFRAMES)"))
    print(BOLD("=" * 110))
    print()

    print(f"  {'#':>3}  {'Symbol':15s}  {'Price':>12s}  {'Score':>8s}  {'15m':>6s}  {'1h':>6s}  {'4h':>6s}  {'1d':>6s}  {'Signal':20s}")
    print(f"  {'─'*3}  {'─'*15}  {'─'*12}  {'─'*8}  {'─'*6}  {'─'*6}  {'─'*6}  {'─'*6}  {'─'*20}")

    for i, (sym, d) in enumerate(deep):
        td = d.get("tf_data", {})
        price = 0
        for tf in ("15m", "1h", "4h", "1d"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break
        sc = {tf: td[tf][0] for tf in ("15m", "1h", "4h", "1d") if tf in td}
        s15 = fsc(sc["15m"]) if "15m" in sc else "  —"
        s1h = fsc(sc["1h"]) if "1h" in sc else "  —"
        s4h = fsc(sc["4h"]) if "4h" in sc else "  —"
        s1d = fsc(sc["1d"]) if "1d" in sc else "  —"
        print(f"  {i+1:>3}  {sym:15s}  ${price:>10,.4f}  {fsc(d['score']):>14s}  {s15:>14s}  {s1h:>14s}  {s4h:>14s}  {s1d:>14s}  {fdir(d.get('confluence',''))}")

    # ── Detail Top 10 ──
    print()
    print(BOLD("═" * 110))
    print(BOLD("  🔥 CHI TIẾT TOP 10"))
    print(BOLD("═" * 110))

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
        print(f"  ┌{'─'*108}┐")
        print(f"  │ #{rank+1}  {BOLD(sym):15s}  Score: {fsc(d['score'])}  Signal: {fdir(conf)}  Price: ${price:,.4f}")
        print(f"  └{'─'*108}┘")

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
            print(f"    ├── [{tf:>3s}] {fsc(sc)}  Hurst: {fh(hurst)}  {regime}")
            print(f"    │   RSI:{rsi:.0f}  LagRSI:{lag:.3f}  CMO:{cmo:.1f}  BB%B:{bb:.2f}")

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

        print(f"    │")
        if "BUY" in conf:
            print(f"    ├── {BGR('💡 LONG')} ↑")
            if price > 0:
                sl = price * 0.97
                tp1 = price * 1.03
                tp2 = price * 1.06
                print(f"    │   Entry:${price:,.4f} SL:${sl:,.4f} TP1:${tp1:,.4f} TP2:${tp2:,.4f}")
        elif "SELL" in conf:
            print(f"    ├── {BRD('💡 SHORT')} ↓")
            if price > 0:
                sl = price * 1.03
                tp1 = price * 0.97
                tp2 = price * 0.94
                print(f"    │   Entry:${price:,.4f} SL:${sl:,.4f} TP1:${tp1:,.4f} TP2:${tp2:,.4f}")
        else:
            print(f"    ├── {YL('💡 WAIT')}")
        print(f"    └{'─'*108}┘")

    # ── Summary ──
    print()
    print(BOLD("═" * 110))
    print(BOLD("  📋 TÓM TẮT"))
    print(BOLD("═" * 110))
    print()

    buys = [(s, d) for s, d in deep if "BUY" in d.get("confluence", "")]
    sells = [(s, d) for s, d in deep if "SELL" in d.get("confluence", "")]
    mixed = [(s, d) for s, d in deep if d.get("confluence", "") == "MIXED"]

    print(f"  Quick scan: {len(symbols)} | Deep: {len(deep)}")
    print(f"  {GR('BUY')}: {len(buys)} | {RD('SELL')}: {len(sells)} | {YL('MIXED')}: {len(mixed)}")
    print()

    if buys:
        print(f"  {BGR('🟢 TOP BUY:')}")
        for sym, d in buys[:10]:
            td = d.get("tf_data", {})
            parts = " | ".join(f"{tf}:{td[tf][0]:.0f}" for tf in ("15m","1h","4h","1d") if tf in td)
            print(f"    {sym:15s}  {fsc(d['score'])}  [{parts}]  {d.get('confluence','')}")
    print()

    if sells:
        print(f"  {BRD('🔴 TOP SHORT:')}")
        for sym, d in sells[:5]:
            td = d.get("tf_data", {})
            parts = " | ".join(f"{tf}:{td[tf][0]:.0f}" for tf in ("15m","1h","4h","1d") if tf in td)
            print(f"    {sym:15s}  {fsc(d['score'])}  [{parts}]  {d.get('confluence','')}")
    print()

    # Save
    rdir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(rdir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    rpt = os.path.join(rdir, f"top100_{ts}.json")

    out = []
    for sym, d in deep:
        td = d.get("tf_data", {})
        price = 0
        for tf in ("15m", "1h", "4h", "1d"):
            p = td.get(tf, (0, {}))[1].get("price", 0)
            if p > 0:
                price = p
                break
        out.append({
            "rank": len(out) + 1,
            "symbol": sym,
            "price": price,
            "composite_score": round(d.get("score", 0), 2),
            "tf_scores": {tf: round(td[tf][0], 2) for tf in ("15m","1h","4h","1d") if tf in td},
            "confluence": d.get("confluence", ""),
            "directions": d.get("directions", {}),
        })

    with open(rpt, "w") as f:
        json.dump({
            "timestamp": datetime.now().isoformat(),
            "sentiment": f"{fg_val}/100 ({fg_label})",
            "phase1_total": len(symbols),
            "phase2_deep": len(deep),
            "results": out,
        }, f, indent=2)

    print(f"  📁 {rpt}")
    print()
    print(BOLD("=" * 110))


if __name__ == "__main__":
    main()
