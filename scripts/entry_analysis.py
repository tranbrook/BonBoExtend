#!/usr/bin/env python3
"""
BonBo Entry Point Analyzer — Phân tích Điểm vào lệnh TỐT NHẤT
Phân tích: Fibonacci retracement, Order Blocks, Volume Profile, S/R clusters
Multi-timeframe: 15m, 1h, 4h, 1d
"""
import json, urllib.request, math, sys

SYM = sys.argv[1] if len(sys.argv) > 1 else "TIAUSDT"

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

def bollinger(cs, p=20):
    if len(cs) < p: return None, None, None
    sm = sum(cs[-p:])/p
    std = math.sqrt(sum((c-sm)**2 for c in cs[-p:])/p)
    return sm+2*std, sm, sm-2*std

def alma(cs, period=50, offset=0.85, sigma=6):
    if len(cs) < period: return None
    m = offset * (period - 1)
    s = period / sigma
    weights = [math.exp(-((i - m)**2) / (2 * s * s)) for i in range(period)]
    wsum = sum(weights)
    weights = [w/wsum for w in weights]
    return sum(cs[-(period-i)] * weights[i] for i in range(period))

def super_smoother(cs, period=10):
    if len(cs) < period: return None
    a = math.exp(-1.414 * 3.14159 / period)
    b = 2 * a * math.cos(1.414 * 3.14159 / period)
    c2 = b; c3 = -a * a; c1 = 1 - c2 - c3
    filt = [cs[0]]
    for i in range(1, len(cs)):
        filt.append(c1 * (cs[i] + cs[i-1]) / 2 + c2 * filt[-1] + (c3 * filt[-2] if len(filt) >= 2 else 0))
    return filt[-1]

# ═══════════════════════════════════════════════════════════
#  MAIN ANALYSIS
# ═══════════════════════════════════════════════════════════

print("=" * 90)
print(f"  🎯 PHÂN TÍCH ĐIỂM VÀO LỆNH TỐT NHẤT: {SYM}")
print("=" * 90)

# ── Current Price ──
tk = fj(f"https://fapi.binance.com/fapi/v1/ticker/24hr?symbol={SYM}")
if not tk:
    print("  ❌ Không thể kết nối Binance API"); sys.exit(1)

cur_price = float(tk['lastPrice'])
pct_24h = float(tk['priceChangePercent'])
hi_24h = float(tk['highPrice'])
lo_24h = float(tk['lowPrice'])
vol_24h = float(tk['quoteVolume'])

print(f"\n  💰 Giá hiện tại:  ${cur_price:.4f}")
print(f"  📊 24h Change:    {pct_24h:+.2f}%")
print(f"  📈 24h High:      ${hi_24h:.4f}")
print(f"  📉 24h Low:       ${lo_24h:.4f}")
print(f"  💵 24h Volume:    ${vol_24h:,.0f}")

# ═══════════════════════════════════════════════════════════
#  ① FIBONACCI RETRACEMENT — Từ swing gần nhất
# ═══════════════════════════════════════════════════════════
print(f"\n{'─'*90}")
print(f"  ① FIBONACCI RETRACEMENT — Tìm vùng pullback tốt nhất")
print(f"{'─'*90}")

cd_4h = fk(SYM, "4h", 200)
cd_1h = fk(SYM, "1h", 500)
cd_15m = fk(SYM, "15m", 500)

if cd_4h and len(cd_4h) > 50:
    # Find recent swing high/low on 4H
    highs_4h = [(c['h'], c['t']) for c in cd_4h[-100:]]
    lows_4h = [(c['l'], c['t']) for c in cd_4h[-100:]]

    # Find the last significant swing
    swing_high = max(highs_4h, key=lambda x: x[0])
    swing_low = min(lows_4h, key=lambda x: x[0])

    sh_price, sh_time = swing_high
    sl_price, sl_time = swing_low

    # Determine trend direction
    if sh_time > sl_time:  # Low came first, then high → Uptrend
        trend = "UPTREND"
        fib_dir = "LONG"
        diff = sh_price - sl_price
        fib_levels = {
            "0.0% (Swing High)": sh_price,
            "23.6%": sh_price - diff * 0.236,
            "38.2% ⭐": sh_price - diff * 0.382,
            "50.0% ⭐⭐": sh_price - diff * 0.500,
            "61.8% ⭐⭐⭐ (Golden)": sh_price - diff * 0.618,
            "78.6%": sh_price - diff * 0.786,
            "100.0% (Swing Low)": sl_price,
        }
    else:  # High came first, then low → Downtrend
        trend = "DOWNTREND"
        fib_dir = "SHORT"
        diff = sl_price - sh_price
        fib_levels = {
            "0.0% (Swing Low)": sl_price,
            "23.6%": sl_price - diff * 0.236,
            "38.2% ⭐": sl_price - diff * 0.382,
            "50.0% ⭐⭐": sl_price - diff * 0.500,
            "61.8% ⭐⭐⭐ (Golden)": sl_price - diff * 0.618,
            "78.6%": sl_price - diff * 0.786,
            "100.0% (Swing High)": sh_price,
        }

    print(f"\n  📊 Xu hướng: {trend} ({fib_dir})")
    print(f"  📈 Swing High: ${sh_price:.4f}")
    print(f"  📉 Swing Low:  ${sl_price:.4f}")
    print(f"  📏 Range:      ${diff:.4f} ({diff/sl_price*100:.1f}%)")
    print()
    print(f"  {'Level':<30s} {'Giá':>12s} {'Khoảng cách':>14s} {'Zone':>8s}")
    print(f"  {'─'*30} {'─'*12} {'─'*14} {'─'*8}")

    for name, price in fib_levels.items():
        dist = (price - cur_price) / cur_price * 100
        near = " ◀️ BẠN ĐÂY" if abs(dist) < 1.5 else ""
        zone = "DEEP" if "61.8" in name or "78.6" in name else "SHALLOW" if "23.6" in name else "MID"
        marker = "🎯" if abs(dist) < 1.5 else "  "
        print(f"  {marker} {name:<28s} ${price:>10.4f}  {dist:>+7.2f}%     {zone}{near}")

# ═══════════════════════════════════════════════════════════
#  ② VOLUME PROFILE — Tìm vùng giá có volume cao nhất
# ═══════════════════════════════════════════════════════════
print(f"\n{'─'*90}")
print(f"  ② VOLUME PROFILE — Tìm vùng giá tích lũy / phân phối")
print(f"{'─'*90}")

if cd_1h and len(cd_1h) > 100:
    prices = [c['c'] for c in cd_1h[-200:]]
    volumes = [c['v'] for c in cd_1h[-200:]]
    price_min = min(c['l'] for c in cd_1h[-200:])
    price_max = max(c['h'] for c in cd_1h[-200:])

    # Create volume profile bins
    n_bins = 30
    bin_size = (price_max - price_min) / n_bins
    bins = [(price_min + i * bin_size, price_min + (i+1) * bin_size, 0) for i in range(n_bins)]
    bins = [[b[0], b[1], 0] for b in bins]

    for c in cd_1h[-200:]:
        mid = (c['h'] + c['l']) / 2
        for b in bins:
            if b[0] <= mid < b[1]:
                b[2] += c['v']
                break

    # Sort by volume to find POC, VA high, VA low
    total_vol = sum(b[2] for b in bins)
    sorted_bins = sorted(bins, key=lambda x: x[2], reverse=True)

    poc_bin = sorted_bins[0]
    poc = (poc_bin[0] + poc_bin[1]) / 2

    # Value area (70% volume)
    va_vol = 0; va_bins = []
    for b in sorted_bins:
        va_bins.append(b)
        va_vol += b[2]
        if va_vol >= total_vol * 0.7: break

    va_high = max((b[1] for b in va_bins))
    va_low = min((b[0] for b in va_bins))

    # Find high volume nodes (HVN) and low volume nodes (LVN)
    vol_threshold_high = total_vol / n_bins * 1.5
    vol_threshold_low = total_vol / n_bins * 0.5

    hvn_zones = [(b[0], b[1], b[2]) for b in bins if b[2] > vol_threshold_high]
    lvn_zones = [(b[0], b[1], b[2]) for b in bins if b[2] < vol_threshold_low]

    print(f"\n  📍 POC (Point of Control):  ${poc:.4f}")
    print(f"  📊 Value Area High:        ${va_high:.4f}")
    print(f"  📊 Value Area Low:         ${va_low:.4f}")
    print(f"  📏 Vị trí hiện tại vs VA:  ", end="")
    if cur_price > va_high:
        print(f"🟢 TRÊN VA (+{(cur_price-va_high)/cur_price*100:.1f}%) — Xuất sắc nếu hold LONG")
    elif cur_price < va_low:
        print(f"🔴 DƯỚI VA (-{(va_low-cur_price)/cur_price*100:.1f}%) — Cẩn thận, vùng rẻ nhưng cần confirm")
    else:
        print(f"🟡 TRONG VA — Neutral, đợi breakout")

    print(f"\n  🔴 High Volume Nodes (Support/Resistance mạnh):")
    for lo, hi, v in hvn_zones[:5]:
        mid = (lo + hi) / 2
        dist = (mid - cur_price) / cur_price * 100
        bar = '█' * int(v / total_vol * n_bins * 3)
        print(f"     ${lo:.4f} — ${hi:.4f}  (vol: {v:,.0f})  {dist:+.2f}%  {bar}")

    print(f"\n  🟢 Low Volume Nodes (Vùng giá di chuyển nhanh):")
    for lo, hi, v in lvn_zones[:3]:
        mid = (lo + hi) / 2
        dist = (mid - cur_price) / cur_price * 100
        print(f"     ${lo:.4f} — ${hi:.4f}  (vol: {v:,.0f})  {dist:+.2f}%")

# ═══════════════════════════════════════════════════════════
#  ③ ORDER BLOCKS — Tìm OB gần nhất
# ═══════════════════════════════════════════════════════════
print(f"\n{'─'*90}")
print(f"  ③ ORDER BLOCKS — Tìm vùng OB / Demand / Supply")
print(f"{'─'*90}")

if cd_4h and len(cd_4h) > 50:
    # Simple OB detection: last bullish/bearish candle before impulse move
    bullish_obs = []
    bearish_obs = []

    for i in range(2, len(cd_4h)):
        prev = cd_4h[i-1]
        curr = cd_4h[i]
        prev2 = cd_4h[i-2]

        # Bullish OB: bearish candle before strong bullish move
        if prev['c'] < prev['o'] and curr['c'] > curr['o']:
            move = (curr['c'] - curr['o']) / curr['o'] * 100
            if move > 2.0:  # Significant move
                bullish_obs.append({
                    'high': prev['h'], 'low': prev['l'],
                    'mid': (prev['h'] + prev['l']) / 2,
                    'move_pct': move,
                    'dist': (cur_price - prev['l']) / cur_price * 100
                })

        # Bearish OB: bullish candle before strong bearish move
        if prev['c'] > prev['o'] and curr['c'] < curr['o']:
            move = (curr['o'] - curr['c']) / curr['o'] * 100
            if move > 2.0:
                bearish_obs.append({
                    'high': prev['h'], 'low': prev['l'],
                    'mid': (prev['h'] + prev['l']) / 2,
                    'move_pct': move,
                    'dist': (prev['h'] - cur_price) / cur_price * 100
                })

    print(f"\n  🟢 Bullish Order Blocks (Demand zones — LONG entry):")
    if bullish_obs:
        for ob in sorted(bullish_obs, key=lambda x: abs(x['dist']))[:5]:
            if ob['dist'] > 0:  # Below current price
                status = "📍 GẦN ĐÂY" if abs(ob['dist']) < 3 else ""
                print(f"     ${ob['low']:.4f} — ${ob['high']:.4f}  (mid: ${ob['mid']:.4f})  "
                      f"Cách {-ob['dist']:.2f}%  Move: +{ob['move_pct']:.1f}%  {status}")
    else:
        print(f"     Không tìm thấy OB rõ ràng")

    print(f"\n  🔴 Bearish Order Blocks (Supply zones — Avoid/SL):")
    if bearish_obs:
        for ob in sorted(bearish_obs, key=lambda x: abs(x['dist']))[:5]:
            status = "⚠️ TRÊN ĐẦU" if ob['dist'] > 0 and ob['dist'] < 5 else ""
            print(f"     ${ob['low']:.4f} — ${ob['high']:.4f}  (mid: ${ob['mid']:.4f})  "
                  f"Cách +{ob['dist']:.2f}%  Move: -{ob['move_pct']:.1f}%  {status}")
    else:
        print(f"     Không tìm thấy OB rõ ràng")

# ═══════════════════════════════════════════════════════════
#  ④ MULTI-TIMEFRAME CONFLUENCE — Tìm điểm hội tụ
# ═══════════════════════════════════════════════════════════
print(f"\n{'─'*90}")
print(f"  ④ MULTI-TIMEFRAME CONFLUENCE — Điểm hội tụ tối ưu")
print(f"{'─'*90}")

confluence_zones = []

for tf_name, tf, cd_data in [("1D", "1d", fk(SYM, "1d", 200)), 
                               ("4H", "4h", cd_4h),
                               ("1H", "1h", cd_1h),
                               ("15M", "15m", cd_15m)]:
    if not cd_data or len(cd_data) < 50: continue
    cs = [c['c'] for c in cd_data]
    r = rsi(cs)
    e12 = ema(cs, 12); e26 = ema(cs, 26); e50 = ema(cs, 50); e200 = ema(cs, 200) if len(cs) >= 200 else None
    bb_up, bb_mid, bb_lo = bollinger(cs)
    a = calc_atr(cd_data)
    al = alma(cs)
    ss = super_smoother(cs)
    h = hurst(cs[-100:])

    # Collect support levels
    supports = []
    resistances = []

    if e50: supports.append(("EMA50", e50))
    if e200: supports.append(("EMA200", e200))
    if bb_lo: supports.append(("BB Lower", bb_lo))
    if al and al < cur_price: supports.append(("ALMA", al))

    if bb_up: resistances.append(("BB Upper", bb_up))
    if bb_mid: resistances.append(("BB Mid", bb_mid))

    # Recent swing lows/highs
    recent = cd_data[-20:]
    swing_low = min(c['l'] for c in recent)
    swing_high = max(c['h'] for c in recent)
    supports.append(("Swing Low", swing_low))
    resistances.append(("Swing High", swing_high))

    ema_st = "🟢 BULL" if e12 and e26 and e12 > e26 else "🔴 BEAR"
    h_st = "📈 TREND" if h and h > 0.55 else "🔄 MREV" if h and h < 0.45 else "➡ RAND"

    print(f"\n  ── {tf_name} ── {ema_st} | RSI={r:.1f} | Hurst={h:.3f} ({h_st})" if r and h else f"\n  ── {tf_name} ──")

    print(f"     Support:    ", end="")
    for name, price in sorted(supports, key=lambda x: x[1], reverse=True):
        dist = (price - cur_price) / cur_price * 100
        if dist < 0:  # Below current price
            print(f"${price:.4f}({name},{dist:+.1f}%) ", end="")
            confluence_zones.append((price, f"{tf_name} {name}"))
    print()

    print(f"     Resistance: ", end="")
    for name, price in sorted(resistances, key=lambda x: x[1]):
        dist = (price - cur_price) / cur_price * 100
        if dist > 0:  # Above current price
            print(f"${price:.4f}({name},{dist:+.1f}%) ", end="")
            confluence_zones.append((price, f"{tf_name} {name}"))
    print()

# ═══════════════════════════════════════════════════════════
#  ⑤ CONFLUENCE SCORING — Tìm điểm vào TỐT NHẤT
# ═══════════════════════════════════════════════════════════
print(f"\n{'─'*90}")
print(f"  ⑤ CONFLUENCE SCORING — Xếp hạng điểm vào lệnh")
print(f"{'─'*90}")

# Group confluence zones by proximity
zones_clustered = []
threshold = cur_price * 0.015  # 1.5% tolerance

sorted_zones = sorted(confluence_zones, key=lambda x: x[0])
i = 0
while i < len(sorted_zones):
    cluster = [sorted_zones[i]]
    j = i + 1
    while j < len(sorted_zones) and abs(sorted_zones[j][0] - sorted_zones[i][0]) < threshold:
        cluster.append(sorted_zones[j])
        j += 1
    zones_clustered.append(cluster)
    i = j

# Score each cluster
scored_zones = []
for cluster in zones_clustered:
    avg_price = sum(z[0] for z in cluster) / len(cluster)
    score = len(cluster)  # More confluences = higher score
    labels = [z[1] for z in cluster]
    dist = (avg_price - cur_price) / cur_price * 100
    scored_zones.append({
        'price': avg_price,
        'score': score,
        'labels': labels,
        'dist': dist,
        'type': 'SUPPORT' if dist < 0 else 'RESISTANCE'
    })

scored_zones.sort(key=lambda x: x['score'], reverse=True)

print(f"\n  {'Điểm vào':>12s} {'Score':>6s} {'Loại':>12s} {'Khoảng cách':>12s} Confluences")
print(f"  {'─'*12} {'─'*6} {'─'*12} {'─'*12} {'─'*40}")

for z in scored_zones[:10]:
    stars = "⭐" * min(z['score'], 5)
    labels_str = " + ".join(z['labels'][:4])
    if len(z['labels']) > 4:
        labels_str += f" +{len(z['labels'])-4} more"
    marker = "🎯" if z['score'] >= 3 else "  "
    print(f"  {marker} ${z['price']:>9.4f}  {z['score']:>3d}/5  {z['type']:>12s}  {z['dist']:>+8.2f}%  {stars}")
    print(f"     └─ {labels_str}")

# ═══════════════════════════════════════════════════════════
#  ⑥ FINAL ENTRY RECOMMENDATION
# ═══════════════════════════════════════════════════════════
print(f"\n{'═'*90}")
print(f"  🎯 FINAL — ĐIỂM VÀO LỆNH TỐT NHẤT CHO {SYM}")
print(f"{'═'*90}")

# Determine overall direction from 4H
if cd_4h and len(cd_4h) > 50:
    cs_4h = [c['c'] for c in cd_4h]
    e12_4h = ema(cs_4h, 12); e26_4h = ema(cs_4h, 26)
    direction = "LONG" if e12_4h and e26_4h and e12_4h > e26_4h else "SHORT"
else:
    direction = "LONG"

# Find best support zone for LONG entry or best resistance for SHORT
best_zone = None
best_buy_zone = None
best_sell_zone = None

for z in scored_zones:
    if direction == "LONG" and z['type'] == 'SUPPORT' and z['dist'] < 0:
        if best_buy_zone is None or z['score'] > best_buy_zone['score']:
            best_buy_zone = z
    elif direction == "SHORT" and z['type'] == 'RESISTANCE' and z['dist'] > 0:
        if best_sell_zone is None or z['score'] > best_sell_zone['score']:
            best_sell_zone = z

if direction == "LONG":
    # Find entry below current price (pullback)
    if best_buy_zone:
        entry = best_buy_zone['price']
        entry_type = "LIMIT BUY (Pullback)"
    else:
        entry = cur_price
        entry_type = "MARKET BUY"

    # SL below the zone
    a_4h = calc_atr(cd_4h) if cd_4h else cur_price * 0.03
    sl = entry - a_4h * 2.5

    # TP levels
    risk = entry - sl
    tp1 = entry + risk * 1.5
    tp2 = entry + risk * 2.0
    tp3 = entry + risk * 3.0

    liq = entry * (1 - 1/10 + 0.004)  # 10x leverage

    print(f"""
  ┌────────────────────────────────────────────────────────────────────┐
  │  🪙 {SYM}    ${cur_price:.4f}    📈 Hướng: {direction}    Đòn bẩy: 10x
  │
  │  ═══════════════ KỊCH BẢN 1: PULLBACK ENTRY (Khuyến nghị) ═══════
  │
  │  📥 LIMIT BUY:  ${entry:.4f}  ({(entry-cur_price)/cur_price*100:+.2f}% từ giá hiện tại)
  │     └─ Confluences: {' + '.join(best_buy_zone['labels'][:3]) if best_buy_zone else 'Market entry'}
  │     └─ Score: {best_buy_zone['score'] if best_buy_zone else 'N/A'}/5 {'⭐' * min(best_buy_zone['score'], 5) if best_buy_zone else ''}
  │
  │  ═══════════════ KỊCH BẢN 2: BREAKOUT ENTRY (Aggressive) ═════════
  │
  │  📥 STOP BUY:   ${cur_price * 1.005:.4f}  (+0.5% — breakout confirm)
  │     └─ Khi giá phá qua resistance gần nhất
  │
  │  ═══════════════ QUẢN LÝ RỦI RO ════════════════════════════════
  │
  │  🛑 STOP LOSS:  ${sl:.4f}  (-{(entry-sl)/entry*100:.2f}% từ entry)
  │  💀 THANH LÝ:   ${liq:.4f}  (-{(entry-liq)/entry*100:.2f}% từ entry)
  │
  │  🎯 TP1 (30%):  ${tp1:.4f}  (+{(tp1-entry)/entry*100:.2f}%)
  │  🎯 TP2 (40%):  ${tp2:.4f}  (+{(tp2-entry)/entry*100:.2f}%)
  │  🎯 TP3 (30%):  ${tp3:.4f}  (+{(tp3-entry)/entry*100:.2f}%)
  │
  │  📊 R:R = 1:{(tp1-entry)/(entry-sl):.1f} (TP1) | 1:{(tp2-entry)/(entry-sl):.1f} (TP2) | 1:{(tp3-entry)/(entry-sl):.1f} (TP3)
  └────────────────────────────────────────────────────────────────────┘""")

    # Scenario analysis
    print(f"\n  📊 PHÂN TÍCH KỊCH BẢN:")
    print(f"  {'─'*60}")

    if best_buy_zone:
        print(f"  ✅ Pullback về ${entry:.4f} và nảy lên → Vào LONG")
        print(f"     Entry tại vùng có {best_buy_zone['score']} confluences → Xác suất cao")
        print(f"     SL ngay dưới vùng support → Risk minimized")
    else:
        print(f"  ✅ Không có pullback zone rõ ràng → Market entry với SL rộng")

    # Key levels to watch
    print(f"\n  🔑 MỨC GIÁ CẦN THEO DÕI:")
    resistances_above = [z for z in scored_zones if z['type'] == 'RESISTANCE']
    supports_below = [z for z in scored_zones if z['type'] == 'SUPPORT']

    if resistances_above:
        r = resistances_above[0]
        print(f"  🔴 Resistance gần nhất: ${r['price']:.4f} ({r['dist']:+.2f}%)")
        print(f"     → Nếu phá qua → Target tiếp theo")
        print(f"     → Nếu bật xuống → Pullback entry opportunity")
    if supports_below:
        s = supports_below[0]
        print(f"  🟢 Support gần nhất: ${s['price']:.4f} ({s['dist']:+.2f}%)")
        print(f"     → Đây là vùng pullback lý tưởng")

else:  # SHORT
    if best_sell_zone:
        entry = best_sell_zone['price']
        entry_type = "LIMIT SELL (Pullback to resistance)"
    else:
        entry = cur_price
        entry_type = "MARKET SELL"

    a_4h = calc_atr(cd_4h) if cd_4h else cur_price * 0.03
    sl = entry + a_4h * 2.5
    risk = sl - entry
    tp1 = entry - risk * 1.5
    tp2 = entry - risk * 2.0
    tp3 = entry - risk * 3.0
    liq = entry * (1 + 1/10 - 0.004)

    print(f"""
  ┌────────────────────────────────────────────────────────────────────┐
  │  🪙 {SYM}    ${cur_price:.4f}    📉 Hướng: {direction}    Đòn bẩy: 10x
  │
  │  📥 LIMIT SELL: ${entry:.4f}  ({(entry-cur_price)/cur_price*100:+.2f}%)
  │     └─ {' + '.join(best_sell_zone['labels'][:3]) if best_sell_zone else 'Market entry'}
  │
  │  🛑 STOP LOSS:   ${sl:.4f}  (+{(sl-entry)/entry*100:.2f}%)
  │  💀 THANH LÝ:    ${liq:.4f}  (+{(liq-entry)/entry*100:.2f}%)
  │
  │  🎯 TP1 (30%):   ${tp1:.4f}  (-{(entry-tp1)/entry*100:.2f}%)
  │  🎯 TP2 (40%):   ${tp2:.4f}  (-{(entry-tp2)/entry*100:.2f}%)
  │  🎯 TP3 (30%):   ${tp3:.4f}  (-{(entry-tp3)/entry*100:.2f}%)
  │
  │  📊 R:R = 1:{(entry-tp1)/(sl-entry):.1f} (TP1) | 1:{(entry-tp2)/(sl-entry):.1f} (TP2)
  └────────────────────────────────────────────────────────────────────┘""")

# ═══════════════════════════════════════════════════════════
#  ⑦ TIMING — Khi nào vào lệnh?
# ═══════════════════════════════════════════════════════════
print(f"\n{'─'*90}")
print(f"  ⑦ TIMING — Khi nào vào lệnh?")
print(f"{'─'*90}")

if cd_15m and len(cd_15m) > 50:
    cs_15m = [c['c'] for c in cd_15m]
    r_15m = rsi(cs_15m)
    e12_15m = ema(cs_15m, 12); e26_15m = ema(cs_15m, 26)

    # Check 15m momentum
    last_5 = cs_15m[-5:]
    mom = (last_5[-1] - last_5[0]) / last_5[0] * 100

    # MACD histogram
    macd_line = (e12_15m - e26_15m) if e12_15m and e26_15m else 0

    if direction == "LONG":
        if r_15m and r_15m < 40:
            timing = "✅ TỐT — RSI 15m đang quá bán → Đang pullback, sắp nảy"
            action = "📥 ĐẶT LIMIT BUY ngay hoặc chờ nến 15m đóng trên EMA12"
        elif r_15m and r_15m < 50:
            timing = "🟡 KHÁ — RSI 15m neutral → Có thể vào với size nhỏ"
            action = "📥 VÀO 50% SIZE, giữ 50% đợi pullback sâu hơn"
        elif r_15m and r_15m > 60:
            timing = "⚠️ CHƯA TỐT — RSI 15m quá mua → Đang ở đỉnh ngắn hạn"
            action = "⏳ CHỜ — Đợi RSI 15m về <50 rồi mới vào"
        else:
            timing = "🟡 TRUNG TÍNH"
            action = "📥 Có thể vào với size nhỏ"
    else:
        if r_15m and r_15m > 60:
            timing = "✅ TỐT — RSI 15m đang quá mua → Đang pullback lên, sắp giảm"
            action = "📥 ĐẶT LIMIT SELL ngay hoặc chờ nến 15m đóng dưới EMA12"
        elif r_15m and r_15m > 50:
            timing = "🟡 KHÁ — RSI 15m neutral → Có thể vào với size nhỏ"
            action = "📥 VÀO 50% SIZE"
        else:
            timing = "⚠️ CHƯA TỐT — RSI 15m quá bán"
            action = "⏳ CHỜ đợi"

    print(f"\n  📊 RSI 15m:     {r_15m:.1f}" if r_15m else "  📊 RSI 15m:     N/A")
    print(f"  📊 Momentum 5:  {mom:+.3f}%")
    print(f"  📊 MACD 15m:    {'BULL' if macd_line > 0 else 'BEAR'} ({macd_line:+.6f})")
    print(f"\n  Nhận định: {timing}")
    print(f"  Hành động: {action}")

print(f"""
{'═'*90}
  ⚠️ DISCLAIMER: Phân tích mang tính tham khảo. Quản lý rủi ro cẩn thận!
  🔧 BonBo Entry Point Analyzer — {SYM}
{'═'*90}""")
