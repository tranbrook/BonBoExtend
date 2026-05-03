import json, urllib.request, math, time

def fj(url):
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=15) as r: return json.loads(r.read())
    except: return None

def fk(sym, tf="1h", n=200):
    d = fj(f"https://fapi.binance.com/fapi/v1/klines?symbol={sym}&interval={tf}&limit={n}")
    if not d: return None
    return [{'o':float(k[1]),'h':float(k[2]),'l':float(k[3]),'c':float(k[4]),'v':float(k[5])} for k in d]

def rsi(cs, p=14):
    if len(cs) < p+1: return None
    g, l = [], []
    for i in range(1, len(cs)):
        d = cs[i]-cs[i-1]; g.append(max(d,0)); l.append(max(-d,0))
    ag = sum(g[:p])/p; al = sum(l[:p])/p
    if al == 0: return 100.0
    rv = 100-(100/(1+ag/al))
    for i in range(p, len(g)):
        ag = (ag*(p-1)+g[i])/p; al = (al*(p-1)+l[i])/p
        rv = 100-(100/(1+ag/al)) if al > 0 else 100.0
    return rv

def ema(cs, p):
    if len(cs) < p: return None
    k = 2/(p+1); e = sum(cs[:p])/p
    for c in cs[p:]: e = c*k+e*(1-k)
    return e

def calc_atr(cd, p=14):
    if len(cd) < p+1: return None
    trs = []
    for i in range(1, len(cd)):
        c, pv = cd[i], cd[i-1]
        trs.append(max(c['h']-c['l'], abs(c['h']-pv['c']), abs(c['l']-pv['c'])))
    a = sum(trs[:p])/p
    for t in trs[p:]: a = (a*(p-1)+t)/p
    return a

def hurst(pr):
    if len(pr) < 40: return None
    ret = [math.log(pr[i]/pr[i-1]) for i in range(1, len(pr))]
    n = len(ret); rv = []
    for s in [8,16,32,64]:
        nw = n//s
        if nw < 2: continue
        rl = []
        for w in range(nw):
            ch = ret[w*s:(w+1)*s]; m = sum(ch)/len(ch); cl = []; cs = 0
            for r in ch: cs += r-m; cl.append(cs)
            R = max(cl)-min(cl); S = math.sqrt(sum((r-m)**2 for r in ch)/len(ch))
            if S > 0: rl.append(R/S)
        if rl: rv.append((math.log(s), math.log(sum(rl)/len(rl))))
    if len(rv) < 2: return None
    np_ = len(rv); sx = sum(v[0] for v in rv); sy = sum(v[1] for v in rv)
    sxy = sum(v[0]*v[1] for v in rv); sx2 = sum(v[0]**2 for v in rv)
    d = np_*sx2-sx**2
    return max(0.0, min(1.0, (np_*sxy-sx*sy)/d)) if abs(d) > 1e-10 else None

def zs(v, c, s):
    if s == 0: return 0
    return max(-1.0, min(1.0, (v-c)/s))

# ── Step 1: Get top tickers ──
print("Scanning market...")
tk = fj("https://fapi.binance.com/fapi/v1/ticker/24hr")
usdt = [t for t in tk if t['symbol'].endswith('USDT')]
fl = []
for t in usdt:
    v = float(t['quoteVolume']); p = float(t['lastPrice'])
    if v > 50e6 and p > 0.001:
        fl.append({'sym': t['symbol'], 'pr': p, 'pct': float(t['priceChangePercent']), 'vol': v})
fl.sort(key=lambda x: x['vol'], reverse=True)
cands = fl[:50]
print(f"Scanning {len(cands)} coins...")

# ── Step 2: Compute indicators ──
results = []
for coin in cands:
    sym = coin['sym']
    time.sleep(0.15)
    cd = fk(sym, "1h", 200)
    if not cd or len(cd) < 100: continue
    cs = [c['c'] for c in cd]; pr = cs[-1]
    r = rsi(cs); e12 = ema(cs, 12); e26 = ema(cs, 26); e50 = ema(cs, 50)
    a = calc_atr(cd); h = hurst(cs[-100:])
    if None in [r, e12, e26, e50, a, h]: continue

    sc = 0; w = 0; dirn = "LONG"; sigs = []

    # RSI
    if r < 30:
        sc += zs(r, 30, 15)*20; w += 20; sigs.append(f"RSI={r:.0f}OVRS")
    elif r > 70:
        sc += zs(r, 70, 15)*20; w += 20; sigs.append(f"RSI={r:.0f}OVRB")
    else:
        sc += zs(r, 50, 25)*20; w += 20

    # EMA cross
    if e12 > e26:
        ep = (e12-e26)/e26*100; sc += min(1.0, ep/3)*20; w += 20; sigs.append(f"EMA▲{ep:.1f}")
    else:
        ep = (e26-e12)/e12*100; sc -= min(1.0, ep/3)*20; w += 20; sigs.append(f"EMA▼{ep:.1f}"); dirn = "SHORT"

    # Price vs EMA50
    if pr > e50:
        sc += 1.0*15; w += 15; sigs.append(">E50")
    else:
        sc -= 1.0*15; w += 15; sigs.append("<E50"); dirn = "SHORT"

    # Hurst
    if h > 0.55:
        sc += zs(h, 0.55, 0.15)*15; w += 15; reg = "TREND"
    elif h < 0.45:
        sc -= zs(h, 0.45, 0.15)*15; w += 15; reg = "MREV"
    else:
        sc += 0*15; w += 15; reg = "RAND"
    sigs.append(f"H={h:.2f}{reg}")

    # Volume
    vls = [c['v'] for c in cd[-20:]]; av = sum(vls)/len(vls); rv = sum(vls[-3:])/3
    vr = rv/av if av > 0 else 1
    sc += min(1.0, max(-1.0, (vr-1)/2))*10; w += 10
    if vr > 1.5: sigs.append(f"Vol×{vr:.1f}")

    # Momentum 5 bars
    m5 = (cs[-1]-cs[-6])/cs[-6]*100 if len(cs) >= 6 else 0
    sc += zs(m5, 0, 5)*10; w += 10

    # ATR sweet spot
    ap = a/pr*100
    if 1.5 < ap < 5: sc += 1.0*10
    elif ap > 5: sc += 0.5*10
    else: sc += 0.3*10
    w += 10; sigs.append(f"ATR={ap:.1f}%")

    comp = max(0, min(100, (sc/w+1)/2*100))
    if comp >= 70: act = f"BUY {dirn}"
    elif comp >= 58: act = f"LEAN {dirn}"
    elif comp <= 30: act = "SELL"
    elif comp <= 42: act = "LEAN SHORT"
    else: act = "HOLD"

    sm = 2.0 if h > 0.55 else 2.5
    if dirn == "LONG":
        sl = pr-a*sm; t1 = pr+a*sm*1.5; t2 = pr+a*sm*2; t3 = pr+a*sm*3
        rr = (t2-pr)/(pr-sl) if pr > sl else 0
    else:
        sl = pr+a*sm; t1 = pr-a*sm*1.5; t2 = pr-a*sm*2; t3 = pr-a*sm*3
        rr = (pr-t2)/(sl-pr) if sl > pr else 0

    results.append({
        'sym': sym, 'pr': pr, 'pct': coin['pct'], 'rsi': r,
        'ema': 'BULL' if e12 > e26 else 'BEAR', 'h': h, 'a': a, 'ap': ap,
        'vr': vr, 'sm': sm, 'sl': sl, 't1': t1, 't2': t2, 't3': t3,
        'rr': rr, 'comp': comp, 'act': act, 'dir': dirn, 'sig': sigs, 'reg': reg
    })

results.sort(key=lambda x: x['comp'], reverse=True)
print(f"\nScanned {len(results)} coins OK\n")

# ── Step 3: Print ranking table ──
print("="*115)
print(f"  #  {'Symbol':14s} {'Price':>11s} {'24h':>7s} {'RSI':>5s} {'EMA':>5s} {'Hurst':>6s} {'ATR%':>5s} {'Score':>6s} {'Action':12s} {'R:R':>5s} {'SL':>10s} {'TP2':>10s}")
print("="*115)
for i, r in enumerate(results[:25]):
    m = "🏆" if i == 0 else "🥈" if i == 1 else "🥉" if i == 2 else "  "
    print(f"  {m}{i+1:<2} {r['sym']:14s} {r['pr']:>11,.4f} {r['pct']:>+6.1f}% {r['rsi']:>5.1f} {r['ema']:>5s} {r['h']:>6.3f} {r['ap']:>5.1f} {r['comp']:>6.1f} {r['act']:12s} {r['rr']:>5.2f} {r['sl']:>10,.4f} {r['t2']:>10,.4f}")

# ── Step 4: Detail TOP 3 with 10x leverage ──
for rank, r in enumerate(results[:3]):
    ms = ["🏆 BEST ENTRY", "🥈 RUNNER UP", "🥉 THIRD PICK"]
    print(f"\n{'='*90}")
    print(f"  {ms[rank]}: {r['sym']} — {r['act']} (Score {r['comp']:.1f})")
    print(f"{'='*90}")

    e = r['pr']; a = r['a']; lv = 10; eq = 1000; mg = eq*0.5; ps = mg*lv; qy = ps/e; mt = 0.004
    d = r['dir']

    if d == "LONG":
        lq = e*(1-1/lv+mt); sl = max(r['sl'], lq*1.03); sd = e-sl
        t1 = e+sd*1.5; t2 = e+sd*2; t3 = e+sd*3
    else:
        lq = e*(1+1/lv-mt); sl = min(r['sl'], lq*0.97); sd = sl-e
        t1 = e-sd*1.5; t2 = e-sd*2; t3 = e-sd*3

    ld = ps*abs(e-sl)/e
    p1 = ps*abs(t1-e)/e*0.3; p2 = ps*abs(t2-e)/e*0.4; p3 = ps*abs(t3-e)/e*0.3
    tp = p1+p2+p3; rr = tp/ld if ld > 0 else 0; lb = abs(sl-lq)/lq*100

    print(f"""
  ┌──────────────────────────────────────────────────────────────────────────┐
  │  {r['sym']:14s}  ${e:,.4f}   {d:6s}   Score: {r['comp']:.1f}                    │
  │  Signals: {' | '.join(r['sig'][:5])}                             │
  │                                                                          │
  │  📥 ENTRY:  MARKET {'BUY' if d=='LONG' else 'SELL'} ${e:.4f}  — VAO NGAY               │
  │     Qty: {qy:,.4f}  Exposure: ${ps:,.0f}  Margin: ${mg:,.0f}               │
  │                                                                          │
  │  🛑 SL:    ${sl:.4f}  (ATR x{r['sm']:.1f})  Loss: -${ld:,.2f} ({ld/eq*100:.1f}%)    │
  │  💀 Liq:   ${lq:.4f}  Buffer: {lb:.1f}%                               │
  │                                                                          │
  │  🎯 TP1(30%): ${t1:.4f}  -> +${p1:,.2f}                           │
  │  🎯 TP2(40%): ${t2:.4f}  -> +${p2:,.2f}                           │
  │  🎯 TP3(30%): ${t3:.4f}  -> +${p3:,.2f}                           │
  │                                                                          │
  │  💰 Target: +${tp:,.2f} ({tp/eq*100:.0f}% equity)   Risk: -${ld:,.2f} ({ld/eq*100:.0f}%)      │
  │  📊 R:R: {rr:.1f}:1                                                  │
  └──────────────────────────────────────────────────────────────────────────┘""")
