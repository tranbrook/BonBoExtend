#!/usr/bin/env python3
"""DOTUSDT Position Analysis — Full analysis with technical indicators, position info, and forecast."""

import json, subprocess, time, os, sys
from datetime import datetime, timedelta

# ── Load .env ─────────────────────────────────────────────────────────────────
API_KEY = ""
API_SECRET = ""

env_path = os.path.expanduser("~/BonBoExtend/.env")
with open(env_path) as f:
    for line in f:
        line = line.strip()
        if '=' in line and not line.startswith('#'):
            key, val = line.split('=', 1)
            if key == 'BINANCE_API_KEY':
                API_KEY = val
            elif key == 'BINANCE_API_SECRET':
                API_SECRET = val

# ── API helpers ───────────────────────────────────────────────────────────────
def signed_api(endpoint, params=""):
    ts = int(time.time() * 1000)
    p = f"{params}&timestamp={ts}&recvWindow=60000" if params else f"timestamp={ts}&recvWindow=60000"
    sig = subprocess.run(['openssl','dgst','-sha256','-hmac', API_SECRET],
        input=p, capture_output=True, text=True).stdout.strip().split()[-1]
    r = subprocess.run(['curl','-s','--max-time','10','-H',f'X-MBX-APIKEY: {API_KEY}',
        f"https://fapi.binance.com{endpoint}?{p}&signature={sig}"],
        capture_output=True, text=True)
    return json.loads(r.stdout)

def public_api(endpoint, params=""):
    url = f"https://fapi.binance.com{endpoint}?{params}" if params else f"https://fapi.binance.com{endpoint}"
    r = subprocess.run(['curl','-s','--max-time','10', url],
        capture_output=True, text=True)
    return json.loads(r.stdout)

# ── Technical indicator calculations ─────────────────────────────────────────
def calc_sma(closes, period):
    if len(closes) < period:
        return None
    return sum(closes[-period:]) / period

def calc_ema(closes, period):
    if len(closes) < period:
        return None
    k = 2 / (period + 1)
    ema = sum(closes[:period]) / period
    for c in closes[period:]:
        ema = c * k + ema * (1 - k)
    return ema

def calc_rsi(closes, period=14):
    if len(closes) < period + 1:
        return None
    gains, losses = [], []
    for i in range(1, len(closes)):
        diff = closes[i] - closes[i-1]
        gains.append(max(diff, 0))
        losses.append(max(-diff, 0))
    avg_gain = sum(gains[:period]) / period
    avg_loss = sum(losses[:period]) / period
    for i in range(period, len(gains)):
        avg_gain = (avg_gain * (period - 1) + gains[i]) / period
        avg_loss = (avg_loss * (period - 1) + losses[i]) / period
    if avg_loss == 0:
        return 100
    rs = avg_gain / avg_loss
    return 100 - (100 / (1 + rs))

def calc_macd(closes):
    if len(closes) < 26:
        return None, None, None
    ema12 = calc_ema(closes, 12)
    ema26 = calc_ema(closes, 26)
    macd_line = ema12 - ema26
    # Calculate signal line (9-period EMA of MACD)
    macd_vals = []
    for i in range(26, len(closes)):
        e12 = calc_ema(closes[:i+1], 12)
        e26 = calc_ema(closes[:i+1], 26)
        macd_vals.append(e12 - e26)
    if len(macd_vals) >= 9:
        signal = calc_ema(macd_vals, 9)
        hist = macd_line - signal
        return macd_line, signal, hist
    return macd_line, None, None

def calc_atr(highs, lows, closes, period=14):
    if len(highs) < period + 1:
        return None
    trs = []
    for i in range(1, len(highs)):
        tr = max(highs[i] - lows[i], abs(highs[i] - closes[i-1]), abs(lows[i] - closes[i-1]))
        trs.append(tr)
    return sum(trs[-period:]) / period

def calc_bollinger(closes, period=20, std_mult=2.0):
    if len(closes) < period:
        return None, None, None
    sma = sum(closes[-period:]) / period
    variance = sum((c - sma) ** 2 for c in closes[-period:]) / period
    std = variance ** 0.5
    upper = sma + std_mult * std
    lower = sma - std_mult * std
    pct_b = (closes[-1] - lower) / (upper - lower) if upper != lower else 0.5
    return upper, lower, pct_b

def calc_stoch(highs, lows, closes, k_period=14, d_period=3):
    if len(closes) < k_period:
        return None, None
    period_highs = highs[-k_period:]
    period_lows = lows[-k_period:]
    hh = max(period_highs)
    ll = min(period_lows)
    k = ((closes[-1] - ll) / (hh - ll)) * 100 if hh != ll else 50
    return k, None  # Simplified

def calc_adx(highs, lows, closes, period=14):
    if len(closes) < period * 2:
        return None
    trs, plus_dms, minus_dms = [], [], []
    for i in range(1, len(highs)):
        tr = max(highs[i] - lows[i], abs(highs[i] - closes[i-1]), abs(lows[i] - closes[i-1]))
        up_move = highs[i] - highs[i-1]
        down_move = lows[i-1] - lows[i]
        plus_dm = up_move if up_move > down_move and up_move > 0 else 0
        minus_dm = down_move if down_move > up_move and down_move > 0 else 0
        trs.append(tr)
        plus_dms.append(plus_dm)
        minus_dms.append(minus_dm)
    
    if len(trs) < period:
        return None
    atr = sum(trs[-period:]) / period
    if atr == 0:
        return 0
    plus_di = (sum(plus_dms[-period:]) / period) / atr * 100
    minus_di = (sum(minus_dms[-period:]) / period) / atr * 100
    dx = abs(plus_di - minus_di) / (plus_di + minus_di) * 100 if (plus_di + minus_di) != 0 else 0
    return dx

def calc_vwap(highs, lows, closes, volumes):
    if not volumes or sum(volumes) == 0:
        return None
    cum_tp_vol = sum((h + l + c) / 3 * v for h, l, c, v in zip(highs, lows, closes, volumes))
    cum_vol = sum(volumes)
    return cum_tp_vol / cum_vol if cum_vol > 0 else None

def calc_pivot_points(h, l, c):
    pivot = (h + l + c) / 3
    r1 = 2 * pivot - l
    s1 = 2 * pivot - h
    r2 = pivot + (h - l)
    s2 = pivot - (h - l)
    r3 = h + 2 * (pivot - l)
    s3 = l - 2 * (h - pivot)
    return {"pivot": pivot, "r1": r1, "r2": r2, "r3": r3, "s1": s1, "s2": s2, "s3": s3}

def detect_regime(rsi, adx, atr, avg_atr, hurst=0.5):
    if adx is not None and adx > 30:
        regime = "TRENDING"
        strategy = "Trend-following (MACD, MA crossover)"
    elif adx is not None and adx < 20:
        regime = "RANGING"
        strategy = "Mean-reversion (RSI, Bollinger Bands)"
    else:
        regime = "TRANSITIONAL"
        strategy = "Breakout / Mixed"
    
    volatility = "HIGH" if atr and avg_atr and atr > avg_atr * 1.3 else "NORMAL" if atr and avg_atr and atr > avg_atr * 0.7 else "LOW"
    
    return {"regime": regime, "volatility": volatility, "strategy": strategy}

def forecast_scenarios(price, sma50, sma200, rsi, macd_hist, atr, adx, pivots):
    """Generate 30-day forecast scenarios."""
    # Base scenario
    base_change = 0
    if sma50 and sma200:
        if sma50 > sma200:
            base_change = 3.0  # Golden cross, bullish bias
        else:
            base_change = -2.0  # Death cross, bearish bias
    
    if rsi is not None:
        if rsi > 70:
            base_change -= 2.0
        elif rsi < 30:
            base_change += 2.0
    
    if macd_hist is not None:
        base_change += macd_hist * 5  # Scale MACD hist impact
    
    # Bullish scenario
    bull_change = base_change + max(atr * 3 / price * 100 if atr else 5, 8)
    # Bearish scenario
    bear_change = base_change - max(atr * 3 / price * 100 if atr else 5, 8)
    
    # Probabilities based on indicators
    bull_prob = 0.35
    bear_prob = 0.30
    if rsi is not None and rsi > 60:
        bull_prob += 0.1
        bear_prob -= 0.05
    elif rsi is not None and rsi < 40:
        bull_prob -= 0.05
        bear_prob += 0.1
    
    if sma50 and price > sma50:
        bull_prob += 0.05
        bear_prob -= 0.05
    
    base_prob = 1.0 - bull_prob - bear_prob
    
    return {
        "scenarios": {
            "bullish": {"target": round(price * (1 + bull_change/100), 4), "change": round(bull_change, 2)},
            "base": {"target": round(price * (1 + base_change/100), 4), "change": round(base_change, 2)},
            "bearish": {"target": round(price * (1 + bear_change/100), 4), "change": round(bear_change, 2)},
        },
        "probabilities": {"bullish": round(bull_prob, 2), "base": round(base_prob, 2), "bearish": round(bear_prob, 2)},
    }

# ── MAIN ─────────────────────────────────────────────────────────────────────
def main():
    SYMBOL = "DOTUSDT"
    
    print()
    print("=" * 100)
    print(f"  🔮 DOTUSDT — PHÂN TÍCH VỊ THẾ TOÀN DIỆN")
    print(f"  Thời gian: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("=" * 100)
    
    # ══════════════════════════════════════════════════════════════════════════
    # 1. THÔNG TIN VỊ THẾ (POSITION)
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("━" * 100)
    print("  📊 PHẦN 1: THÔNG TIN VỊ THẾ DOTUSDT TRÊN BINANCE FUTURES")
    print("━" * 100)
    
    # Get position info
    positions = signed_api("/fapi/v3/positionRisk", f"symbol={SYMBOL}")
    dot_pos = None
    if isinstance(positions, list):
        for p in positions:
            if p.get("symbol") == SYMBOL:
                dot_pos = p
                break
    
    # Get account balance
    account = signed_api("/fapi/v3/account")
    wallet_balance = 0
    unrealized_pnl = 0
    available_balance = 0
    if isinstance(account, dict):
        wallet_balance = float(account.get("totalWalletBalance", 0))
        unrealized_pnl = float(account.get("totalUnrealizedProfit", 0))
        available_balance = float(account.get("availableBalance", 0))
    
    # Get open orders
    open_orders = signed_api("/fapi/v1/openOrders", f"symbol={SYMBOL}")
    
    # Get recent trades
    recent_trades = signed_api("/fapi/v1/userTrades", f"symbol={SYMBOL}&limit=30")
    
    # Current price
    ticker = public_api("/fapi/v1/ticker/price", f"symbol={SYMBOL}")
    current_price = float(ticker.get("price", 0)) if isinstance(ticker, dict) else 0
    
    # Mark price
    mark_price_data = public_api("/fapi/v1/premiumIndex", f"symbol={SYMBOL}")
    mark_price = float(mark_price_data.get("markPrice", 0)) if isinstance(mark_price_data, dict) else 0
    funding_rate = float(mark_price_data.get("lastFundingRate", 0)) if isinstance(mark_price_data, dict) else 0
    next_funding_time = int(mark_price_data.get("nextFundingTime", 0)) if isinstance(mark_price_data, dict) else 0
    
    # Liquidation price from position
    liq_price = float(dot_pos.get("liquidationPrice", 0)) if dot_pos else 0
    
    if dot_pos:
        pos_amt = float(dot_pos.get("positionAmt", 0))
        entry_price = float(dot_pos.get("entryPrice", 0))
        unrealized = float(dot_pos.get("unRealizedProfit", 0))
        leverage = dot_pos.get("leverage", "?")
        margin_type = "ISOLATED" if abs(pos_amt) > 0 else "-"
        
        print(f"\n  📌 Vị thế: {'LONG 🟢' if pos_amt > 0 else 'SHORT 🔴' if pos_amt < 0 else 'FLAT ⚪'}")
        print(f"  ┌─────────────────────────────────────────────────────────────────────")
        print(f"  │ Symbol:          {SYMBOL}")
        print(f"  │ Số lượng:        {pos_amt} DOT")
        print(f"  │ Giá entry:       ${entry_price:.4f}")
        print(f"  │ Giá hiện tại:    ${current_price:.4f} (Mark: ${mark_price:.4f})")
        print(f"  │ Leverage:        {leverage}x")
        print(f"  │ PnL chưa thực:   ${unrealized:+.4f}")
        
        if entry_price > 0 and pos_amt != 0:
            pnl_pct = ((current_price - entry_price) / entry_price) * 100
            if pos_amt < 0:
                pnl_pct = -pnl_pct
            notional = abs(pos_amt) * current_price
            margin_used = notional / int(leverage) if leverage != "?" else 0
            actual_pnl_pct = (unrealized / margin_used * 100) if margin_used > 0 else 0
            
            print(f"  │ PnL % (vào lệnh): {pnl_pct:+.2f}%")
            print(f"  │ PnL % (margin):   {actual_pnl_pct:+.2f}%")
            print(f"  │ Notional Value:  ${notional:.2f}")
            print(f"  │ Margin Used:     ${margin_used:.2f}")
        
        if liq_price > 0:
            liq_dist = ((current_price - liq_price) / current_price) * 100
            print(f"  │ Giá thanh lý:    ${liq_price:.4f} (cách {liq_dist:.1f}%)")
        
        print(f"  └─────────────────────────────────────────────────────────────────────")
    else:
        print(f"\n  ⚪ KHÔNG CÓ VỊ THẾ DOTUSDT đang mở")
    
    # Open orders
    print(f"\n  📋 Lệnh mở ({len(open_orders) if isinstance(open_orders, list) else 0}):")
    if isinstance(open_orders, list) and open_orders:
        for o in open_orders:
            o_side = o.get("side", "?")
            o_type = o.get("type", "?")
            o_price = float(o.get("price", 0))
            o_qty = float(o.get("origQty", 0))
            o_status = o.get("status", "?")
            o_id = o.get("orderId", "?")
            stop_price = float(o.get("stopPrice", 0))
            emoji = "🟢" if o_side == "BUY" else "🔴"
            price_str = f"${o_price:.4f}" if o_price > 0 else f"Stop@${stop_price:.4f}"
            print(f"    {emoji} #{o_id} | {o_side} {o_type} | {o_qty} DOT @ {price_str} | {o_status}")
    else:
        print(f"    (Không có lệnh mở)")
    
    # Recent trades
    print(f"\n  📜 Lệnh gần đây ({len(recent_trades) if isinstance(recent_trades, list) else 0}):")
    if isinstance(recent_trades, list) and recent_trades:
        total_realized = 0
        total_commission = 0
        for t in reversed(recent_trades[-10:]):
            t_side = t.get("side", "?")
            t_price = float(t.get("price", 0))
            t_qty = float(t.get("qty", 0))
            t_pnl = float(t.get("realizedPnl", 0))
            t_comm = float(t.get("commission", 0))
            t_time = datetime.fromtimestamp(t.get("time", 0) / 1000).strftime("%m/%d %H:%M")
            total_realized += t_pnl
            total_commission += t_comm
            emoji = "🟢" if t_side == "BUY" else "🔴"
            print(f"    {emoji} {t_time} | {t_side:4s} {t_qty:8.1f} DOT @ ${t_price:.4f} | PnL: ${t_pnl:+.4f} | Fee: ${t_comm:.4f}")
        print(f"    ────────────────────────────────────────────────────")
        print(f"    💰 Tổng Realized PnL: ${total_realized:+.4f} | Total Fee: ${total_commission:.4f} | Net: ${total_realized - total_commission:+.4f}")
    
    # Account summary
    print(f"\n  💼 Tổng quan tài khoản:")
    print(f"    Wallet Balance:    ${wallet_balance:.2f} USDT")
    print(f"    Available:         ${available_balance:.2f} USDT")
    print(f"    Unrealized PnL:    ${unrealized_pnl:+.4f} USDT (tất cả vị thế)")
    
    # Funding rate
    if funding_rate != 0:
        fr_annual = funding_rate * 3 * 365 * 100
        fr_next = datetime.fromtimestamp(next_funding_time / 1000).strftime("%H:%M") if next_funding_time else "?"
        print(f"\n  💵 Funding Rate:     {funding_rate*100:.4f}% (annual: {fr_annual:.1f}%)")
        print(f"    Next Funding:      {fr_next}")
    
    # ══════════════════════════════════════════════════════════════════════════
    # 2. PHÂN TÍCH KỸ THUẬT ĐA TIMEFRAME
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("━" * 100)
    print("  📈 PHẦN 2: PHÂN TÍCH KỸ THUẬT ĐA TIMEFRAME")
    print("━" * 100)
    
    timeframes = {
        "4h": {"interval": "4h", "limit": 100, "label": "4H"},
        "1d": {"interval": "1d", "limit": 200, "label": "1D"},
        "1w": {"interval": "1w", "limit": 52, "label": "1W"},
    }
    
    tf_data = {}
    
    for key, tf in timeframes.items():
        raw = public_api("/fapi/v1/klines", f"symbol={SYMBOL}&interval={tf['interval']}&limit={tf['limit']}")
        if not isinstance(raw, list):
            continue
        
        closes = [float(c[4]) for c in raw]
        highs = [float(c[2]) for c in raw]
        lows = [float(c[3]) for c in raw]
        opens = [float(c[1]) for c in raw]
        volumes = [float(c[5]) for c in raw]
        
        rsi = calc_rsi(closes)
        macd_line, signal, hist = calc_macd(closes)
        sma20 = calc_sma(closes, 20)
        sma50 = calc_sma(closes, 50)
        sma200 = calc_sma(closes, 200)
        ema12 = calc_ema(closes, 12)
        ema26 = calc_ema(closes, 26)
        atr = calc_atr(highs, lows, closes)
        bb_upper, bb_lower, bb_pct = calc_bollinger(closes)
        stoch_k, _ = calc_stoch(highs, lows, closes)
        adx = calc_adx(highs, lows, closes)
        vwap = calc_vwap(highs[-20:], lows[-20:], closes[-20:], volumes[-20:]) if len(volumes) >= 20 else None
        
        avg_atr = None
        if len(closes) > 14:
            avg_atr_val = sum(closes) / len(closes) * 0.03  # rough estimate
            avg_atr = avg_atr_val
        
        tf_data[key] = {
            "closes": closes, "highs": highs, "lows": lows, "volumes": volumes,
            "rsi": rsi, "macd_line": macd_line, "signal": signal, "macd_hist": hist,
            "sma20": sma20, "sma50": sma50, "sma200": sma200,
            "ema12": ema12, "ema26": ema26,
            "atr": atr, "bb_upper": bb_upper, "bb_lower": bb_lower, "bb_pct": bb_pct,
            "stoch_k": stoch_k, "adx": adx, "vwap": vwap, "avg_atr": avg_atr,
            "last_high": highs[-1], "last_low": lows[-1],
            "24h_high": max(highs[-6:]), "24h_low": min(lows[-6:]),
            "vol_avg": sum(volumes[-20:]) / 20 if len(volumes) >= 20 else None,
        }
    
    # Print analysis for each timeframe
    for key, tf in timeframes.items():
        d = tf_data.get(key)
        if not d:
            continue
        
        print(f"\n  ── {tf['label']} Timeframe ──")
        print(f"  Giá đóng gần nhất: ${d['closes'][-1]:.4f}")
        
        if d['sma20']:
            pos_vs_sma20 = "ABOVE ✅" if d['closes'][-1] > d['sma20'] else "BELOW ⚠️"
            print(f"  SMA(20):    ${d['sma20']:.4f}  [{pos_vs_sma20}]")
        if d['sma50']:
            pos_vs_sma50 = "ABOVE ✅" if d['closes'][-1] > d['sma50'] else "BELOW ⚠️"
            print(f"  SMA(50):    ${d['sma50']:.4f}  [{pos_vs_sma50}]")
        if d['sma200']:
            pos_vs_sma200 = "ABOVE ✅" if d['closes'][-1] > d['sma200'] else "BELOW ⚠️"
            print(f"  SMA(200):   ${d['sma200']:.4f}  [{pos_vs_sma200}]")
        
        if d['rsi'] is not None:
            rsi_status = "OVERBOUGHT 🔴" if d['rsi'] > 70 else "OVERSOLD 🟢" if d['rsi'] < 30 else "NEUTRAL ⚪"
            print(f"  RSI(14):    {d['rsi']:.1f}  [{rsi_status}]")
        
        if d['macd_hist'] is not None:
            macd_status = "BULLISH 🟢" if d['macd_hist'] > 0 else "BEARISH 🔴"
            print(f"  MACD Hist:  {d['macd_hist']:.5f}  [{macd_status}]")
            if d['macd_line'] is not None and d['signal'] is not None:
                cross = "GOLDEN CROSS ✨" if d['macd_line'] > d['signal'] else "DEATH CROSS 💀"
                print(f"  MACD/Signal: {cross}")
        
        if d['adx'] is not None:
            trend_str = "TRENDING 📊" if d['adx'] > 25 else "RANGING ↔️"
            print(f"  ADX:        {d['adx']:.1f}  [{trend_str}]")
        
        if d['atr']:
            atr_pct = d['atr'] / d['closes'][-1] * 100
            print(f"  ATR(14):    {d['atr']:.4f} ({atr_pct:.2f}%)")
        
        if d['bb_upper'] and d['bb_lower']:
            print(f"  Bollinger:  Upper ${d['bb_upper']:.4f} | Lower ${d['bb_lower']:.4f} | %B: {d['bb_pct']:.3f}")
        
        if d['stoch_k'] is not None:
            stoch_status = "OVERBOUGHT" if d['stoch_k'] > 80 else "OVERSOLD" if d['stoch_k'] < 20 else "NEUTRAL"
            print(f"  Stoch %K:   {d['stoch_k']:.1f}  [{stoch_status}]")
        
        if d['vwap']:
            vwap_pos = "ABOVE" if d['closes'][-1] > d['vwap'] else "BELOW"
            print(f"  VWAP:       ${d['vwap']:.4f}  [{vwap_pos}]")
        
        vol_now = d['volumes'][-1] if d['volumes'] else 0
        vol_avg = d['vol_avg']
        if vol_avg and vol_avg > 0:
            vol_ratio = vol_now / vol_avg
            vol_status = "HIGH 🔊" if vol_ratio > 1.5 else "NORMAL 🔉" if vol_ratio > 0.5 else "LOW 🔈"
            print(f"  Volume:     {vol_ratio:.2f}x avg  [{vol_status}]")
    
    # ══════════════════════════════════════════════════════════════════════════
    # 3. SUPPORT / RESISTANCE / PIVOT POINTS
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("━" * 100)
    print("  🎯 PHẦN 3: SUPPORT / RESISTANCE / PIVOT POINTS")
    print("━" * 100)
    
    d1 = tf_data.get("1d", {})
    if d1:
        pivots = calc_pivot_points(d1['last_high'], d1['last_low'], d1['closes'][-1])
        print(f"\n  Pivot Points (dựa trên 1D):")
        print(f"  ┌───────────────────────────────────────────────────")
        print(f"  │ Pivot:   ${pivots['pivot']:.4f}")
        print(f"  │ R1: ${pivots['r1']:.4f}  |  S1: ${pivots['s1']:.4f}")
        print(f"  │ R2: ${pivots['r2']:.4f}  |  S2: ${pivots['s2']:.4f}")
        print(f"  │ R3: ${pivots['r3']:.4f}  |  S3: ${pivots['s3']:.4f}")
        print(f"  └───────────────────────────────────────────────────")
        
        # Key levels from recent price action
        if d1['closes']:
            recent_highs = sorted(d1['highs'][-30:], reverse=True)[:5]
            recent_lows = sorted(d1['lows'][-30:])[:5]
            print(f"\n  Key Resistance (30 ngày): {', '.join(f'${h:.4f}' for h in recent_highs[:3])}")
            print(f"  Key Support (30 ngày):    {', '.join(f'${l:.4f}' for l in recent_lows[:3])}")
    
    # ══════════════════════════════════════════════════════════════════════════
    # 4. REGIME DETECTION + TÍN HIỆU TỔNG HỢP
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("━" * 100)
    print("  🧠 PHẦN 4: REGIME + TÍN HIỆU TỔNG HỢP")
    print("━" * 100)
    
    d1_rsi = tf_data.get("1d", {}).get("rsi")
    d1_adx = tf_data.get("1d", {}).get("adx")
    d1_atr = tf_data.get("1d", {}).get("atr")
    d1_avg_atr = tf_data.get("1d", {}).get("avg_atr")
    
    regime = detect_regime(d1_rsi, d1_adx, d1_atr, d1_avg_atr or 0)
    print(f"\n  🔄 Market Regime:  {regime['regime']}")
    print(f"  📊 Volatility:     {regime['volatility']}")
    print(f"  🎯 Best Strategy:  {regime['strategy']}")
    
    # Signal scoring
    signals = []
    signal_score = 0  # -100 to +100
    
    d_4h = tf_data.get("4h", {})
    d_1d = tf_data.get("1d", {})
    d_1w = tf_data.get("1w", {})
    
    # RSI signals
    if d_1d.get("rsi") is not None:
        if d_1d["rsi"] < 30:
            signals.append(("RSI(1D) Oversold", "BULLISH", 15))
            signal_score += 15
        elif d_1d["rsi"] < 40:
            signals.append(("RSI(1D) Low zone", "BULLISH", 8))
            signal_score += 8
        elif d_1d["rsi"] > 70:
            signals.append(("RSI(1D) Overbought", "BEARISH", -15))
            signal_score -= 15
        elif d_1d["rsi"] > 60:
            signals.append(("RSI(1D) High zone", "BEARISH", -8))
            signal_score -= 8
    
    # MACD signals
    if d_1d.get("macd_hist") is not None:
        if d_1d["macd_hist"] > 0:
            signals.append(("MACD(1D) Bullish", "BULLISH", 12))
            signal_score += 12
        else:
            signals.append(("MACD(1D) Bearish", "BEARISH", -12))
            signal_score -= 12
    
    # MA alignment
    if d_1d.get("sma50") and d_1d.get("sma200"):
        if d_1d["sma50"] > d_1d["sma200"]:
            signals.append(("Golden Cross (SMA50>SMA200)", "BULLISH", 10))
            signal_score += 10
        else:
            signals.append(("Death Cross (SMA50<SMA200)", "BEARISH", -10))
            signal_score -= 10
    
    # Price vs SMA50
    if d_1d.get("sma50"):
        if current_price > d_1d["sma50"]:
            signals.append(("Price > SMA50(1D)", "BULLISH", 8))
            signal_score += 8
        else:
            signals.append(("Price < SMA50(1D)", "BEARISH", -8))
            signal_score -= 8
    
    # Bollinger position
    if d_1d.get("bb_pct") is not None:
        if d_1d["bb_pct"] < 0.2:
            signals.append(("BB Lower band touch", "BULLISH", 8))
            signal_score += 8
        elif d_1d["bb_pct"] > 0.8:
            signals.append(("BB Upper band touch", "BEARISH", -8))
            signal_score -= 8
    
    # 4H timeframe alignment
    if d_4h.get("rsi") is not None:
        if d_4h["rsi"] < 35:
            signal_score += 5
            signals.append(("RSI(4H) Low", "BULLISH", 5))
        elif d_4h["rsi"] > 65:
            signal_score -= 5
            signals.append(("RSI(4H) High", "BEARISH", -5))
    
    if d_4h.get("macd_hist") is not None:
        if d_4h["macd_hist"] > 0:
            signal_score += 5
            signals.append(("MACD(4H) Bullish", "BULLISH", 5))
        else:
            signal_score -= 5
            signals.append(("MACD(4H) Bearish", "BEARISH", -5))
    
    # ADX trend strength
    if d_1d.get("adx") is not None:
        if d_1d["adx"] > 30:
            # Strong trend - go with direction
            if signal_score > 0:
                signal_score += 5
                signals.append(("ADX(1D) Strong Trend + direction", "BULLISH", 5))
            else:
                signal_score -= 5
                signals.append(("ADX(1D) Strong Trend - direction", "BEARISH", -5))
    
    print(f"\n  📶 Tín hiệu chi tiết:")
    for name, direction, score in signals:
        emoji = "🟢" if direction == "BULLISH" else "🔴"
        print(f"    {emoji} {name}: {direction} ({score:+d})")
    
    signal_score = max(-100, min(100, signal_score))
    if signal_score > 20:
        overall = "BULLISH 🟢"
    elif signal_score > 5:
        overall = "SLIGHTLY BULLISH 🟡"
    elif signal_score > -5:
        overall = "NEUTRAL ⚪"
    elif signal_score > -20:
        overall = "SLIGHTLY BEARISH 🟠"
    else:
        overall = "BEARISH 🔴"
    
    print(f"\n  ╔════════════════════════════════════════════════════════╗")
    print(f"  ║ TỔNG HỢP TÍN HIỆU: {overall}  (Score: {signal_score:+d}/100)  ║")
    print(f"  ╚════════════════════════════════════════════════════════╝")
    
    # ══════════════════════════════════════════════════════════════════════════
    # 5. DỰ BÁO 30 NGÀY
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("━" * 100)
    print("  🔮 PHẦN 5: DỰ BÁO 30 NGÀY")
    print("━" * 100)
    
    if d_1d:
        fc = forecast_scenarios(
            current_price,
            d_1d.get("sma50"), d_1d.get("sma200"),
            d_1d.get("rsi"), d_1d.get("macd_hist"),
            d_1d.get("atr"), d_1d.get("adx"),
            pivots if 'pivots' in dir() else {"pivot": current_price}
        )
        
        sc = fc["scenarios"]
        pr = fc["probabilities"]
        ev = pr["bullish"] * sc["bullish"]["change"] + pr["base"] * sc["base"]["change"] + pr["bearish"] * sc["bearish"]["change"]
        
        print(f"\n  Bullish ({pr['bullish']*100:.0f}%):  ${sc['bullish']['target']:.4f}  ({sc['bullish']['change']:+.2f}%)")
        print(f"  Base    ({pr['base']*100:.0f}%):  ${sc['base']['target']:.4f}  ({sc['base']['change']:+.2f}%)")
        print(f"  Bearish ({pr['bearish']*100:.0f}%):  ${sc['bearish']['target']:.4f}  ({sc['bearish']['change']:+.2f}%)")
        print(f"\n  Expected return: {ev:+.2f}%")
        print(f"  Expected price:  ${current_price * (1 + ev/100):.4f}")
        
        print(f"\n  📅 Weekly Projections:")
        for wk in range(1, 5):
            frac = wk / 4.3
            bull_w = current_price + (sc["bullish"]["target"] - current_price) * frac
            base_w = current_price + (sc["base"]["target"] - current_price) * frac
            bear_w = current_price + (sc["bearish"]["target"] - current_price) * frac
            dt = datetime.now() + timedelta(weeks=wk)
            print(f"    Week {wk} ({dt.strftime('%b %d')}):  Bull ${bull_w:.4f}  Base ${base_w:.4f}  Bear ${bear_w:.4f}")
    
    # ══════════════════════════════════════════════════════════════════════════
    # 6. TRADE PLAN
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("━" * 100)
    print("  📋 PHẦN 6: TRADE PLAN ĐỀ XUẤT")
    print("━" * 100)
    
    d1_atr = tf_data.get("1d", {}).get("atr") or (current_price * 0.03)
    
    if dot_pos and float(dot_pos.get("positionAmt", 0)) != 0:
        # Have existing position - manage it
        pos_amt = float(dot_pos["positionAmt"])
        entry_price = float(dot_pos["entryPrice"])
        leverage_val = int(dot_pos.get("leverage", "10"))
        
        print(f"\n  📍 Quản lý vị thế hiện tại:")
        
        if pos_amt > 0:
            direction = "LONG"
            sl_price = entry_price - d1_atr * 2
            tp1 = entry_price + d1_atr * 1.5
            tp2 = entry_price + d1_atr * 2.5
            tp3 = entry_price + d1_atr * 4
        else:
            direction = "SHORT"
            sl_price = entry_price + d1_atr * 2
            tp1 = entry_price - d1_atr * 1.5
            tp2 = entry_price - d1_atr * 2.5
            tp3 = entry_price - d1_atr * 4
        
        risk = abs(current_price - sl_price) / current_price * 100
        rr1 = abs(tp1 - entry_price) / abs(sl_price - entry_price)
        rr2 = abs(tp2 - entry_price) / abs(sl_price - entry_price)
        rr3 = abs(tp3 - entry_price) / abs(sl_price - entry_price)
        
        print(f"  Direction:  {direction} ({abs(pos_amt)} DOT)")
        print(f"  Entry:      ${entry_price:.4f}")
        print(f"  Current:    ${current_price:.4f}")
        print(f"  Stop Loss:  ${sl_price:.4f} (risk: {risk:.2f}%)")
        print(f"  TP1:        ${tp1:.4f} (R:R = 1:{rr1:.1f})")
        print(f"  TP2:        ${tp2:.4f} (R:R = 1:{rr2:.1f})")
        print(f"  TP3:        ${tp3:.4f} (R:R = 1:{rr3:.1f})")
        
        # Advice
        if signal_score > 15:
            print(f"\n  ✅ KHUYẾN NGHỊ: Giữ vị thế, xu hướng thuận lợi. Có thể thêm tại pullback.")
        elif signal_score > 0:
            print(f"\n  ⚠️ KHUYẾN NGHỊ: Giữ vị thế nhưng thận trọng. Chú ý SL.")
        elif signal_score > -15:
            print(f"\n  ⚠️ KHUYẾN NGHỊ: Cân nhắc cắt lỗ hoặc giảm kích thước vị thế.")
        else:
            print(f"\n  🔴 KHUYẾN NGHỊ: Xu hướng ngược vị thế. Cân nhắc đóng sớm hoặc SL chặt.")
    else:
        # No position - suggest entry
        print(f"\n  📍 Gợi ý entry mới:")
        
        if signal_score > 15:
            direction = "LONG"
            entry = current_price
            sl = current_price - d1_atr * 2
            tp1 = current_price + d1_atr * 1.5
            tp2 = current_price + d1_atr * 2.5
            tp3 = current_price + d1_atr * 4
        elif signal_score < -15:
            direction = "SHORT"
            entry = current_price
            sl = current_price + d1_atr * 2
            tp1 = current_price - d1_atr * 1.5
            tp2 = current_price - d1_atr * 2.5
            tp3 = current_price - d1_atr * 4
        else:
            direction = "WAIT"
            entry = sl = tp1 = tp2 = tp3 = 0
        
        if direction != "WAIT":
            risk = abs(entry - sl) / entry * 100
            rr1 = abs(tp1 - entry) / abs(sl - entry)
            rr2 = abs(tp2 - entry) / abs(sl - entry)
            rr3 = abs(tp3 - entry) / abs(sl - entry)
            
            print(f"  Direction:  {direction}")
            print(f"  Entry:      ${entry:.4f}")
            print(f"  Stop Loss:  ${sl:.4f} (risk: {risk:.2f}%)")
            print(f"  TP1:        ${tp1:.4f} (R:R = 1:{rr1:.1f})")
            print(f"  TP2:        ${tp2:.4f} (R:R = 1:{rr2:.1f})")
            print(f"  TP3:        ${tp3:.4f} (R:R = 1:{rr3:.1f})")
            confidence = min(abs(signal_score), 95)
            print(f"  Confidence: {confidence:.0f}%")
        else:
            print(f"  ⏸️ WAIT — Tín hiệu chưa rõ ràng (score: {signal_score:+d})")
            print(f"  Chờ xác nhận rõ hơn từ RSI, MACD, hoặc breakout khỏi range.")
    
    # ══════════════════════════════════════════════════════════════════════════
    # 7. SUMMARY
    # ══════════════════════════════════════════════════════════════════════════
    print()
    print("=" * 100)
    print(f"  📊 TÓM TẮT NHANH")
    print("=" * 100)
    
    if dot_pos and float(dot_pos.get("positionAmt", 0)) != 0:
        pnl = float(dot_pos.get("unRealizedProfit", 0))
        pnl_emoji = "🟢" if pnl >= 0 else "🔴"
        print(f"  {pnl_emoji} PnL: ${pnl:+.4f} | Giá: ${current_price:.4f} | Regime: {regime['regime']} | Signal: {overall}")
    else:
        print(f"  ⚪ Không vị thế | Giá: ${current_price:.4f} | Regime: {regime['regime']} | Signal: {overall}")
    
    print()
    print("=" * 100)
    
    # Save report
    rdir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(rdir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    rpt = os.path.join(rdir, f"dotusdt_{ts}.json")
    with open(rpt, "w") as f:
        json.dump({
            "timestamp": datetime.now().isoformat(),
            "symbol": SYMBOL,
            "price": current_price,
            "mark_price": mark_price,
            "position": dot_pos,
            "signal_score": signal_score,
            "regime": regime,
            "indicators_1d": {k: v for k, v in tf_data.get("1d", {}).items() if k not in ("closes","highs","lows","volumes")},
            "indicators_4h": {k: v for k, v in tf_data.get("4h", {}).items() if k not in ("closes","highs","lows","volumes")},
            "wallet_balance": wallet_balance,
            "funding_rate": funding_rate,
        }, f, indent=2, default=str)
    print(f"  💾 Report saved: {rpt}")

if __name__ == "__main__":
    main()
