import json, urllib.request, math

def fj(url):
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=15) as r: return json.loads(r.read())
    except: return None

def fk(sym, tf="1h", n=500):
    d = fj(f"https://fapi.binance.com/fapi/v1/klines?symbol={sym}&interval={tf}&limit={n}")
    if not d: return None
    return [{'t':k[0],'o':float(k[1]),'h':float(k[2]),'l':float(k[3]),'c':float(k[4]),'v':float(k[5])} for k in d]

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

# ── Analyze TOP candidates with multiple timeframes ──
coins = ["SWARMSUSDT", "1000LUNCUSDT", "ORCAUSDT", "PENGUUSDT"]

print("="*95)
print("  DEEP MULTI-TIMEFRAME ANALYSIS — TOP PICKS")
print("="*95)

for sym in coins:
    print(f"\n{'─'*95}")
    print(f"  🔍 {sym}")
    print(f"{'─'*95}")

    # Current price
    tk = fj(f"https://fapi.binance.com/fapi/v1/ticker/price?symbol={sym}")
    if not tk: continue
    cur_price = float(tk['price'])

    for tf in ["15m", "1h", "4h", "1d"]:
        cd = fk(sym, tf, 200)
        if not cd or len(cd) < 50: continue
        cs = [c['c'] for c in cd]; pr = cs[-1]
        r = rsi(cs); e12 = ema(cs, 12); e26 = ema(cs, 26); e50 = ema(cs, 50)
        a = calc_atr(cd); h = hurst(cs[-100:])
        vols = [c['v'] for c in cd[-20:]]
        avg_v = sum(vols)/len(vols)
        rec_v = sum(vols[-3:])/3
        vr = rec_v/avg_v if avg_v > 0 else 1

        ema_st = "BULL" if e12 and e26 and e12 > e26 else "BEAR"
        h_int = "TREND" if h and h > 0.55 else "MREV" if h and h < 0.45 else "RAND"
        r_st = "OVERBOUGHT" if r and r > 70 else "OVERSOLD" if r and r < 30 else "Neutral"

        bb_sma = sum(cs[-20:])/20
        bb_std = math.sqrt(sum((c-bb_sma)**2 for c in cs[-20:])/20)
        bb_up = bb_sma + 2*bb_std; bb_lo = bb_sma - 2*bb_std
        bb_pos = (pr-bb_lo)/(bb_up-bb_lo)*100 if bb_up != bb_lo else 50

        above_e50 = "ABOVE" if e50 and pr > e50 else "BELOW"

        r_str = f"{r:.1f}" if r else "  N/A"
        h_str = f"{h:.3f}" if h else "  N/A"
        print(f"  {tf:>3s} | RSI={r_str:>6s} {r_st:12s} | EMA: {ema_st} | {above_e50} E50 | H={h_str:>6s} {h_int} | BB={bb_pos:.0f}% | Vol x{vr:.1f}")

    # ── 10x Leverage Trade Plan ──
    cd_1h = fk(sym, "1h", 200)
    if not cd_1h: continue
    cs_1h = [c['c'] for c in cd_1h]
    r_1h = rsi(cs_1h)
    h_1h = hurst(cs_1h[-100:])
    a_1h = calc_atr(cd_1h)
    if not all([r_1h, h_1h, a_1h]): continue

    e = cur_price; lv = 10; eq = 1000; mg = eq*0.5; ps = mg*lv; qy = ps/e; mt = 0.004

    # Direction based on EMA
    e12_1h = ema(cs_1h, 12); e26_1h = ema(cs_1h, 26)
    if e12_1h and e26_1h and e12_1h > e26_1h:
        d = "LONG"; lq = e*(1-1/lv+mt)
        sm = 2.0 if h_1h > 0.55 else 2.5
        sl = max(e - a_1h*sm, lq*1.03)
        sd = e - sl
        t1 = e+sd*1.5; t2 = e+sd*2.0; t3 = e+sd*3.0
    else:
        d = "SHORT"; lq = e*(1+1/lv-mt)
        sm = 2.0 if h_1h > 0.55 else 2.5
        sl = min(e + a_1h*sm, lq*0.97)
        sd = sl - e
        t1 = e-sd*1.5; t2 = e-sd*2.0; t3 = e-sd*3.0

    ld = ps*sd/e
    p1 = ps*abs(t1-e)/e*0.3; p2 = ps*abs(t2-e)/e*0.4; p3 = ps*abs(t3-e)/e*0.3
    tp = p1+p2+p3; rr = tp/ld if ld > 0 else 0
    lb = abs(sl-lq)/lq*100
    sl_pct = abs(sl-e)/e*100
    liq_pct = abs(lq-e)/e*100

    print(f"""
  ┌─────────────── FINAL TRADE PLAN ────────────────────────────────────────┐
  │  {sym:15s}  ${e:,.4f}  Direction: {d:6s}  Leverage: {lv}x              │
  │                                                                        │
  │  📥 ENTRY:   MARKET {'BUY' if d=='LONG' else 'SELL'} ${e:.4f}  — VAO NGAY            │
  │     Qty: {qy:,.4f}  Exposure: ${ps:,.0f}  Margin: ${mg:,.0f}               │
  │                                                                        │
  │  🛑 SL:      ${sl:.4f}  (-{sl_pct:.2f}%)  Loss: -${ld:,.2f} ({ld/eq*100:.1f}% equity)    │
  │  💀 LIQUIDATION: ${lq:.4f}  (-{liq_pct:.2f}%)  Buffer: {lb:.1f}%                │
  │                                                                        │
  │  🎯 TP1 (30%): ${t1:.4f}  -> +${p1:,.2f}                            │
  │  🎯 TP2 (40%): ${t2:.4f}  -> +${p2:,.2f}                            │
  │  🎯 TP3 (30%): ${t3:.4f}  -> +${p3:,.2f}                            │
  │                                                                        │
  │  💰 Target: +${tp:,.2f} ({tp/eq*100:.0f}%)     Risk: -${ld:,.2f} ({ld/eq*100:.0f}%)          │
  │  📊 R:R: {rr:.1f}:1                                                   │
  └────────────────────────────────────────────────────────────────────────┘""")

# ── FINAL RECOMMENDATION ──
print(f"\n{'='*95}")
print("  RANKING FINAL — COIN TỐT NHẤT ĐỂ VÀO LỆNH NGAY")
print("="*95)
print("""
  ⚠️  PHÂN TÍCH RỦI RO:

  1. SWARMSUSDT (Score 97.7) — RSI=89 QUÁ MUA!
     → Đã pump +42% trong 24h → Rủi ro cao bị correction
     → KHÔNG khuyến nghị FOMO chase ở đây!

  2. DAMUSDT (Score 94.6) — RSI=84, +149%!!
     → Coin mới pump cực mạnh → CỰC KỲ NGUY HIỂM
     → TRÁNH TUYỆT ĐỐI!

  3. 1000LUNCUSDT (Score 92.8) — RSI=68, +22%
     → Momentum tốt, chưa quá overbought
     → Hurst 0.74 = strong trending
     → ✅ CÓ THỂ XEM XÉT

  4. ORCAUSDT (Score 72.6) — RSI=72
     → Uptrend ổn định hơn, volume tăng
     → Hurst 0.55 = trending nhẹ
     → ✅ AN TOÀN HƠN nhưng cần đợi pullback

  🏆 KHUYẾN NGHỊ: 1000LUNCUSDT LONG
     → Balance tốt nhất giữa momentum và risk
     → Trending strong (H=0.74) + chưa quá overbought (RSI=68)
""")
