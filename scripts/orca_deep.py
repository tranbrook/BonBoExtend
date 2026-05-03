#!/usr/bin/env python3
"""Deep multi-timeframe analysis for ORCAUSDT."""
import json, subprocess, math, sys

SYM = sys.argv[1] if len(sys.argv) > 1 else 'ORCAUSDT'

def fj(url):
    r = subprocess.run(['curl', '-s', '--max-time', '15', url], capture_output=True, text=True, timeout=20)
    if r.returncode != 0 or not r.stdout.strip(): return None
    return json.loads(r.stdout)

def fk(sym, tf='1h', n=300):
    d = fj(f'https://fapi.binance.com/fapi/v1/klines?symbol={sym}&interval={tf}&limit={n}')
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

def bollinger(cs, p=20):
    if len(cs) < p: return None, None, None
    sm = sum(cs[-p:])/p
    std = math.sqrt(sum((c-sm)**2 for c in cs[-p:])/p)
    return sm+2*std, sm, sm-2*std

def stoch_k(cd, p=14):
    if len(cd) < p: return None
    highs = [c['h'] for c in cd[-p:]]
    lows = [c['l'] for c in cd[-p:]]
    hh = max(highs); ll = min(lows)
    cur = cd[-1]['c']
    if hh == ll: return 50.0
    return (cur - ll) / (hh - ll) * 100

def fmt(v, spec='.4f'):
    if v is None: return 'N/A'
    return format(v, spec)

print('=' * 95)
print('  🔍 PHÂN TÍCH CHUYÊN SÂU: ' + SYM)
print('=' * 95)

# Current price info
tk = fj(f'https://fapi.binance.com/fapi/v1/ticker/24hr?symbol={SYM}')
if not tk:
    print('  ❌ Không thể kết nối Binance API')
    sys.exit(1)

pr_now = float(tk['lastPrice'])
pct = float(tk['priceChangePercent'])
hi = float(tk['highPrice'])
lo = float(tk['lowPrice'])
vol = float(tk['volume'])
qv = float(tk['quoteVolume'])

print()
print('  Gia hien tai: $' + fmt(pr_now, ',.4f'))
print('  24h Change:   ' + format(pct, '+.2f') + '%')
print('  24h High:     $' + fmt(hi, ',.4f'))
print('  24h Low:      $' + fmt(lo, ',.4f'))
print('  Volume:       ' + fmt(vol, ',.0f') + ' ORCA')
print('  Quote Vol:    $' + fmt(qv, ',.0f'))

# Multi-timeframe analysis
for tf_name, tf in [('15 PHUT', '15m'), ('1 GIO', '1h'), ('4 GIO', '4h'), ('1 NGAY', '1d')]:
    cd = fk(SYM, tf, 300)
    if not cd or len(cd) < 100:
        print('  ⚠ Khong du data cho ' + tf_name)
        continue
    cs = [c['c'] for c in cd]
    pr = cs[-1]

    r = rsi(cs)
    e12 = ema(cs, 12)
    e26 = ema(cs, 26)
    e50 = ema(cs, 50)
    e200 = ema(cs, 200) if len(cs) >= 200 else None
    a = calc_atr(cd)
    h = hurst(cs[-100:])
    bb_up, bb_mid, bb_lo = bollinger(cs)
    stk = stoch_k(cd)

    vols = [c['v'] for c in cd[-20:]]
    avg_v = sum(vols) / len(vols)
    rec_v = sum(vols[-3:]) / 3
    vr = rec_v / avg_v if avg_v > 0 else 1

    ema_st = '🟢 BULL' if e12 and e26 and e12 > e26 else '🔴 BEAR'
    e50_pos = '🟢 TREN' if e50 and pr > e50 else '🔴 DUOI'
    e200_pos = '🟢 TREN' if e200 and pr > e200 else '🔴 DUOI' if e200 else '  N/A'

    r_st = '🔴 QUA MUA' if r and r > 70 else '🟢 QUA BAN' if r and r < 30 else '🟡 TRUNG TINH'
    h_st = '📈 TREND' if h and h > 0.55 else '🔄 MREV' if h and h < 0.45 else '➡ RAND'

    bb_pos = (pr - bb_lo) / (bb_up - bb_lo) * 100 if bb_up and bb_lo and bb_up != bb_lo else 50

    mom_5 = (cs[-1] - cs[-6]) / cs[-6] * 100 if len(cs) >= 6 else 0
    mom_10 = (cs[-1] - cs[-11]) / cs[-11] * 100 if len(cs) >= 11 else 0

    highs = sorted([c['h'] for c in cd[-50:]], reverse=True)[:3]
    lows = sorted([c['l'] for c in cd[-50:]])[:3]

    gap_pct = abs(e12 - e26) / e26 * 100 if e12 and e26 else 0

    sep = '─' * 95
    print()
    print('  ' + sep)
    print('  📊 ' + tf_name + ' (' + tf + ')')
    print('  ' + sep)
    print('  Gia:           $' + fmt(pr, ',.4f'))
    print('  RSI(14):       ' + fmt(r, '.1f') + ' ' + r_st)
    stk_str = fmt(stk, '.1f')
    stk_state = '🔴 Qua mua' if stk and stk > 80 else '🟢 Qua ban' if stk and stk < 20 else '🟡 Binh thuong'
    print('  Stochastic %K: ' + stk_str + ' ' + stk_state)
    print('  EMA12/26:      ' + ema_st + '  (Gap: ' + fmt(gap_pct, '.2f') + '%)')
    print('  EMA50:         ' + e50_pos + ' EMA50 ($' + fmt(e50) + ')')
    e200_str = '$' + fmt(e200) if e200 else 'N/A'
    print('  EMA200:        ' + e200_pos + ' EMA200 (' + e200_str + ')')
    print('  Bollinger:     Tren $' + fmt(bb_up) + ' | Giua $' + fmt(bb_mid) + ' | Duoi $' + fmt(bb_lo) + ' | Vi tri: ' + fmt(bb_pos, '.0f') + '%')
    atr_pct = a / pr * 100 if a and pr else 0
    print('  ATR(14):       $' + fmt(a) + ' (' + fmt(atr_pct, '.1f') + '%)')
    h_str = fmt(h, '.3f') if h else 'N/A'
    print('  Hurst:         ' + h_str + ' ' + h_st)
    print('  Volume:        Gap ' + fmt(vr, '.1f') + 'x TB 20 ky')
    print('  Momentum 5:    ' + fmt(mom_5, '+.2f') + '%  |  Momentum 10: ' + fmt(mom_10, '+.2f') + '%')
    print('  Resistance:    $' + fmt(highs[0], ',.4f') + ', $' + fmt(highs[1], ',.4f') + ', $' + fmt(highs[2], ',.4f'))
    print('  Support:       $' + fmt(lows[0], ',.4f') + ', $' + fmt(lows[1], ',.4f') + ', $' + fmt(lows[2], ',.4f'))

# ── Final Trade Plan ──
print()
print('=' * 95)
print('  📋 TONG HOP VA KE HOACH GIAO DICH')
print('=' * 95)

cd_4h = fk(SYM, '4h', 200)
cd_1h = fk(SYM, '1h', 200)
if not cd_4h or not cd_1h:
    print('  ❌ Khong the lay data')
    sys.exit(1)

cs_4h = [c['c'] for c in cd_4h]
cs_1h = [c['c'] for c in cd_1h]

r_4h = rsi(cs_4h); r_1h = rsi(cs_1h)
h_4h = hurst(cs_4h[-100:]); h_1h = hurst(cs_1h[-100:])
a_4h = calc_atr(cd_4h); a_1h = calc_atr(cd_1h)
e12_4h = ema(cs_4h, 12); e26_4h = ema(cs_4h, 26)
e12_1h = ema(cs_1h, 12); e26_1h = ema(cs_1h, 26)
e50_4h = ema(cs_4h, 50)

pr = cs_1h[-1]
dir_4h = 'LONG' if e12_4h and e26_4h and e12_4h > e26_4h else 'SHORT'
dir_1h = 'LONG' if e12_1h and e26_1h and e12_1h > e26_1h else 'SHORT'

print()
print('  ╔════════════════════════════════════════════════════════════════════════╗')
print('  ║                    DONG THUAN / XUNG DOT                             ║')
print('  ╠════════════════════════════════════════════════════════════════════════╣')
print('  ║  Huong 4H:  ' + dir_4h + '  (EMA12=' + fmt(e12_4h) + ' vs EMA26=' + fmt(e26_4h) + ')         ║')
print('  ║  Huong 1H:  ' + dir_1h + '  (EMA12=' + fmt(e12_1h) + ' vs EMA26=' + fmt(e26_1h) + ')         ║')
print('  ║  RSI 4H:    ' + fmt(r_4h, '.1f') + '   |  RSI 1H: ' + fmt(r_1h, '.1f') + '                           ║')
print('  ║  Hurst 4H:  ' + fmt(h_4h, '.3f') + '  |  Hurst 1H: ' + fmt(h_1h, '.3f') + '                        ║')
atr4_pct = a_4h / pr * 100 if a_4h else 0
atr1_pct = a_1h / pr * 100 if a_1h else 0
print('  ║  ATR 4H:    $' + fmt(a_4h) + ' (' + fmt(atr4_pct, '.1f') + '%)  |  ATR 1H: $' + fmt(a_1h) + ' (' + fmt(atr1_pct, '.1f') + '%)  ║')
e50_rel = 'TREN' if pr > e50_4h else 'DUOI'
print('  ║  EMA50 4H:  $' + fmt(e50_4h) + '  (Gia ' + e50_rel + ' EMA50)                  ║')
print('  ╚════════════════════════════════════════════════════════════════════════╝')

# Direction decision
if dir_4h == 'LONG' and dir_1h == 'LONG':
    d = 'LONG'
    verdict = '✅ DONG THUAN LONG - Tin hieu manh'
elif dir_4h == 'SHORT' and dir_1h == 'SHORT':
    d = 'SHORT'
    verdict = '✅ DONG THUAN SHORT - Tin hieu manh'
elif dir_4h == 'LONG' and dir_1h == 'SHORT':
    d = 'LONG'
    verdict = '⚠️ XUNG DOT - 4H LONG nhung 1H SHORT → Cho doi pullback'
else:
    d = 'SHORT'
    verdict = '⚠️ XUNG DOT - 4H SHORT nhung 1H LONG → Cho doi pullback'

print()
print('  Nhan dinh: ' + verdict)

# Generate trade plan with 10x leverage
lv = 10; eq = 1000; mg = eq * 0.5; ps = mg * lv; qy = ps / pr; mt = 0.004

if d == 'LONG':
    lq = pr * (1 - 1 / lv + mt)
    sm = 2.0 if (h_1h and h_1h > 0.55) else 2.5
    sl = max(pr - a_4h * sm, lq * 1.03)
    sd = pr - sl
    t1 = pr + sd * 1.5; t2 = pr + sd * 2.0; t3 = pr + sd * 3.0
else:
    lq = pr * (1 + 1 / lv - mt)
    sm = 2.0 if (h_1h and h_1h > 0.55) else 2.5
    sl = min(pr + a_4h * sm, lq * 0.97)
    sd = sl - pr
    t1 = pr - sd * 1.5; t2 = pr - sd * 2.0; t3 = pr - sd * 3.0

ld = ps * sd / pr
p1 = ps * abs(t1 - pr) / pr * 0.3
p2 = ps * abs(t2 - pr) / pr * 0.4
p3 = ps * abs(t3 - pr) / pr * 0.3
tp = p1 + p2 + p3
rr = tp / ld if ld > 0 else 0
lb = abs(sl - lq) / lq * 100
sl_pct = abs(sl - pr) / pr * 100
liq_pct = abs(lq - pr) / pr * 100

buy_sell = 'BUY' if d == 'LONG' else 'SELL'

print()
print('  ┌─────────────────── KE HOACH GIAO DICH ────────────────────────────────┐')
print('  │                                                                        │')
print('  │  🪙 ' + SYM + '    $' + fmt(pr, ',.4f') + '    Huong: ' + d + '    Don bay: ' + str(lv) + 'x')
print('  │                                                                        │')
print('  │  📥 ENTRY:     MARKET ' + buy_sell + ' $' + fmt(pr))
print('  │     So luong: ' + fmt(qy, ',.4f') + '  |  Khai thac: $' + fmt(ps, ',.0f') + '  |  Ky quyc: $' + fmt(mg, ',.0f'))
print('  │                                                                        │')
print('  │  🛑 STOP LOSS: $' + fmt(sl) + '  (-' + fmt(sl_pct, '.2f') + '%)  Lo: -$' + fmt(ld, ',.2f') + ' (' + fmt(ld/eq*100, '.1f') + '% equity)')
print('  │  💀 THANH LY:  $' + fmt(lq) + '  (-' + fmt(liq_pct, '.2f') + '%)  Buffer: ' + fmt(lb, '.1f') + '%')
print('  │                                                                        │')
print('  │  🎯 TP1 (30%): $' + fmt(t1) + '  -> +$' + fmt(p1, ',.2f'))
print('  │  🎯 TP2 (40%): $' + fmt(t2) + '  -> +$' + fmt(p2, ',.2f'))
print('  │  🎯 TP3 (30%): $' + fmt(t3) + '  -> +$' + fmt(p3, ',.2f'))
print('  │                                                                        │')
print('  │  💰 Loi nhuan: +$' + fmt(tp, ',.2f') + ' (' + fmt(tp/eq*100, '.0f') + '%)   Rui ro: -$' + fmt(ld, ',.2f') + ' (' + fmt(ld/eq*100, '.0f') + '%)')
print('  │  📊 R:R: ' + fmt(rr, '.1f') + ':1')
print('  └────────────────────────────────────────────────────────────────────────┘')

# Warnings
print()
warnings = []
if r_4h and r_4h > 70: warnings.append('  ⚠️ RSI 4H qua mua (' + fmt(r_4h, '.1f') + ') — Nguy co pullback')
if r_1h and r_1h > 75: warnings.append('  ⚠️ RSI 1H rat qua mua (' + fmt(r_1h, '.1f') + ') — Can than chasing')
if lb < 3: warnings.append('  ⚠️ Buffer thanh ly thap (' + fmt(lb, '.1f') + '%) — Nguy co bi liquidate cao')
if vr > 2: warnings.append('  ℹ️ Volume bat thuong (' + fmt(vr, '.1f') + 'x) — Co the la climax')
if dir_4h != dir_1h: warnings.append('  ⚠️ Xung dot huong 4H/1H — Khong nen voi vao')

if warnings:
    print('  ⚠️  CANH BAO:')
    for w in warnings:
        print(w)
else:
    print('  ✅ Khong co canh bao dac biet — Tin hieu tuong doi an toan')
