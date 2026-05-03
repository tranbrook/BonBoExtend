#!/usr/bin/env python3
"""Deep multi-timeframe analysis for a single symbol."""
import json, urllib.request, math, sys

def fj(url):
    import subprocess
    r = subprocess.run(['curl', '-s', '--max-time', '15', url], capture_output=True, text=True, timeout=20)
    if r.returncode != 0 or not r.stdout: return None
    return json.loads(r.stdout)

def fk(sym, tf='1h', n=500):
    d = fj(f'https://fapi.binance.com/fapi/v1/klines?symbol={sym}&interval={tf}&limit={n}')
    return [{'t':k[0],'o':float(k[1]),'h':float(k[2]),'l':float(k[3]),'c':float(k[4]),'v':float(k[5])} for k in d]

def rsi(cs, p=14):
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
    k = 2/(p+1); e = sum(cs[:p])/p
    for c in cs[p:]: e = c*k+e*(1-k)
    return e

def calc_atr(cd, p=14):
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

sym = sys.argv[1] if len(sys.argv) > 1 else 'ORCAUSDT'

print('=' * 95)
print(f'  🔍 PHÂN TÍCH CHUYÊN SÂU: {sym}')
print('=' * 95)

# Current price info
tk = fj(f'https://fapi.binance.com/fapi/v1/ticker/24hr?symbol={sym}')
pr_now = float(tk['lastPrice'])
print(f"""
  Giá hiện tại: ${pr_now:,.4f}
  24h Change:   {float(tk['priceChangePercent']):+.2f}%
  24h High:     ${float(tk['highPrice']):,.4f}
  24h Low:      ${float(tk['lowPrice']):,.4f}
  Volume:       {float(tk['volume']):,.0f} ORCA
  Quote Vol:    ${float(tk['quoteVolume']):,.0f}
""")

# Multi-timeframe analysis
for tf_name, tf in [('15 PHÚT', '15m'), ('1 GIỜ', '1h'), ('4 GIỜ', '4h'), ('1 NGÀY', '1d')]:
    cd = fk(sym, tf, 300)
    if not cd or len(cd) < 100:
        print(f"  ⚠️ Không đủ data cho {tf_name}")
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

    ema_st = '🟢 BULL CROSS' if e12 > e26 else '🔴 BEAR CROSS'
    above_e50 = '🟢 TRÊN' if pr > e50 else '🔴 DƯỚI'
    above_e200_str = '🟢 TRÊN' if e200 and pr > e200 else '🔴 DƯỚI' if e200 else '  N/A'

    r_st = '🔴 QUÁ MUA' if r > 70 else '🟢 QUÁ BÁN' if r < 30 else '🟡 TRUNG TÍNH'
    h_st = '📈 TREND' if h and h > 0.55 else '🔄 MRV' if h and h < 0.45 else '➡️ RAND'

    bb_pos = (pr - bb_lo) / (bb_up - bb_lo) * 100 if bb_up and bb_lo and bb_up != bb_lo else 50

    mom_5 = (cs[-1] - cs[-6]) / cs[-6] * 100 if len(cs) >= 6 else 0
    mom_10 = (cs[-1] - cs[-11]) / cs[-11] * 100 if len(cs) >= 11 else 0

    highs = sorted([c['h'] for c in cd[-50:]], reverse=True)[:3]
    lows = sorted([c['l'] for c in cd[-50:]])[:3]

    e200_str = f'${e200:.4f}' if e200 else 'N/A'

    print(f"""
  {'─' * 95}
  📊 {tf_name} ({tf})
  {'─' * 95}
  Giá:           ${pr:,.4f}
  RSI(14):       {r:.1f} {r_st}
  Stochastic %K: {stk:.1f} {'🔴 Quá mua' if stk and stk > 80 else '🟢 Quá bán' if stk and stk < 20 else '🟡 Bình thường'}
  EMA12/26:      {ema_st}  (Gap: {abs(e12 - e26) / e26 * 100:.2f}%)
  EMA50:         {above_e50} EMA50 (${e50:.4f})
  EMA200:        {above_e200_str} EMA200 ({e200_str})
  Bollinger:     Trên ${bb_up:.4f} | Giữa ${bb_mid:.4f} | Dưới ${bb_lo:.4f} | Vị trí: {bb_pos:.0f}%
  ATR(14):       ${a:.4f} ({a / pr * 100:.1f}%)
  Hurst:         {h:.3f if h else 'N/A'} {h_st}
  Volume:        Gấp {vr:.1f}x TB 20 kỳ
  Momentum 5:    {mom_5:+.2f}%  |  Momentum 10: {mom_10:+.2f}%
  Resistance:    ${highs[0]:,.4f}, ${highs[1]:,.4f}, ${highs[2]:,.4f}
  Support:       ${lows[0]:,.4f}, ${lows[1]:,.4f}, ${lows[2]:,.4f}
""")

# ── Final Trade Plan ──
print('=' * 95)
print('  📋 TỔNG HỢP VÀ KẾ HOẠCH GIAO DỊCH')
print('=' * 95)

cd_4h = fk(sym, '4h', 200)
cd_1h = fk(sym, '1h', 200)
cs_4h = [c['c'] for c in cd_4h]
cs_1h = [c['c'] for c in cd_1h]

r_4h = rsi(cs_4h)
r_1h = rsi(cs_1h)
h_4h = hurst(cs_4h[-100:])
h_1h = hurst(cs_1h[-100:])
a_4h = calc_atr(cd_4h)
a_1h = calc_atr(cd_1h)
e12_4h = ema(cs_4h, 12)
e26_4h = ema(cs_4h, 26)
e12_1h = ema(cs_1h, 12)
e26_1h = ema(cs_1h, 26)
e50_4h = ema(cs_4h, 50)

pr = cs_1h[-1]
dir_4h = 'LONG' if e12_4h > e26_4h else 'SHORT'
dir_1h = 'LONG' if e12_1h > e26_1h else 'SHORT'

print(f"""
  ╔════════════════════════════════════════════════════════════════════════╗
  ║                    ĐỒNG THUẬN / XUNG ĐỘT                            ║
  ╠════════════════════════════════════════════════════════════════════════╣
  ║  Hướng 4H:  {dir_4h:6s}  (EMA12={e12_4h:.4f} vs EMA26={e26_4h:.4f})             ║
  ║  Hướng 1H:  {dir_1h:6s}  (EMA12={e12_1h:.4f} vs EMA26={e26_1h:.4f})             ║
  ║  RSI 4H:    {r_4h:.1f}   |  RSI 1H: {r_1h:.1f}                              ║
  ║  Hurst 4H:  {h_4h:.3f}  |  Hurst 1H: {h_1h:.3f}                           ║
  ║  ATR 4H:    ${a_4h:.4f} ({a_4h / pr * 100:.1f}%)  |  ATR 1H: ${a_1h:.4f} ({a_1h / pr * 100:.1f}%)  ║
  ║  EMA50 4H:  ${e50_4h:.4f}  (Giá {'TRÊN' if pr > e50_4h else 'DƯỚI'} EMA50)                  ║
  ╚════════════════════════════════════════════════════════════════════════╝
""")

# Direction decision
if dir_4h == 'LONG' and dir_1h == 'LONG':
    d = 'LONG'
    verdict = '✅ ĐỒNG THUẬN LONG — Tín hiệu mạnh'
elif dir_4h == 'SHORT' and dir_1h == 'SHORT':
    d = 'SHORT'
    verdict = '✅ ĐỒNG THUẬN SHORT — Tín hiệu mạnh'
elif dir_4h == 'LONG' and dir_1h == 'SHORT':
    d = 'LONG'
    verdict = '⚠️ XUNG ĐỘT — 4H LONG nhưng 1H SHORT → Chờ đợi pullback'
else:
    d = 'SHORT'
    verdict = '⚠️ XUNG ĐỘT — 4H SHORT nhưng 1H LONG → Chờ đợi pullback'

print(f'  Nhận định: {verdict}')

# Generate trade plan with 10x leverage
lv = 10
eq = 1000
mg = eq * 0.5
ps = mg * lv
qy = ps / pr
mt = 0.004

if d == 'LONG':
    lq = pr * (1 - 1 / lv + mt)
    sm = 2.0 if (h_1h and h_1h > 0.55) else 2.5
    sl = max(pr - a_4h * sm, lq * 1.03)
    sd = pr - sl
    t1 = pr + sd * 1.5
    t2 = pr + sd * 2.0
    t3 = pr + sd * 3.0
else:
    lq = pr * (1 + 1 / lv - mt)
    sm = 2.0 if (h_1h and h_1h > 0.55) else 2.5
    sl = min(pr + a_4h * sm, lq * 0.97)
    sd = sl - pr
    t1 = pr - sd * 1.5
    t2 = pr - sd * 2.0
    t3 = pr - sd * 3.0

ld = ps * sd / pr
p1 = ps * abs(t1 - pr) / pr * 0.3
p2 = ps * abs(t2 - pr) / pr * 0.4
p3 = ps * abs(t3 - pr) / pr * 0.3
tp = p1 + p2 + p3
rr = tp / ld if ld > 0 else 0
lb = abs(sl - lq) / lq * 100
sl_pct = abs(sl - pr) / pr * 100
liq_pct = abs(lq - pr) / pr * 100

print(f"""
  ┌─────────────────── KẾ HOẠCH GIAO DỊCH ────────────────────────────────┐
  │                                                                        │
  │  🪙 {sym:15s}  ${pr:,.4f}    Hướng: {d:6s}    Đòn bẩy: {lv}x              │
  │                                                                        │
  │  📥 ENTRY:     MARKET {'BUY' if d == 'LONG' else 'SELL'} ${pr:.4f}                         │
  │     Số lượng: {qy:,.4f}  |  Khai thác: ${ps:,.0f}  |  Ký quỹ: ${mg:,.0f}          │
  │                                                                        │
  │  🛑 STOP LOSS: ${sl:.4f}  (-{sl_pct:.2f}%)  Lỗ: -${ld:,.2f} ({ld / eq * 100:.1f}% equity)   │
  │  💀 THANH LÝ:  ${lq:.4f}  (-{liq_pct:.2f}%)  Buffer: {lb:.1f}%                │
  │                                                                        │
  │  🎯 TP1 (30%): ${t1:.4f}  -> +${p1:,.2f}                            │
  │  🎯 TP2 (40%): ${t2:.4f}  -> +${p2:,.2f}                            │
  │  🎯 TP3 (30%): ${t3:.4f}  -> +${p3:,.2f}                            │
  │                                                                        │
  │  💰 Lợi nhuận: +${tp:,.2f} ({tp / eq * 100:.0f}%)   Rủi ro: -${ld:,.2f} ({ld / eq * 100:.0f}%)       │
  │  📊 R:R: {rr:.1f}:1                                                   │
  └────────────────────────────────────────────────────────────────────────┘
""")

# Warnings
warnings = []
if r_4h > 70: warnings.append(f'⚠️ RSI 4H quá mua ({r_4h:.1f}) — Nguy cơ pullback')
if r_1h > 75: warnings.append(f'⚠️ RSI 1H rất quá mua ({r_1h:.1f}) — Cẩn thận chasing')
if lb < 3: warnings.append(f'⚠️ Buffer thanh lý thấp ({lb:.1f}%) — Nguy cơ bị liquidate cao')
if vr > 2: warnings.append(f'ℹ️ Volume bất thường ({vr:.1f}x) — Có thể là climax')
if dir_4h != dir_1h: warnings.append('⚠️ Xung đột hướng 4H/1H — Không nên vội vào')

if warnings:
    print('  ⚠️  CẢNH BÁO:')
    for w in warnings:
        print(f'    {w}')
else:
    print('  ✅ Không có cảnh báo đặc biệt — Tín hiệu tương đối an toàn')
