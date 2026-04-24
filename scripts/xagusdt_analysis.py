#!/usr/bin/env python3
"""
XAGUSDT (Silver) Deep Analysis + 30-Day Forecast v2.0

Phân tích toàn diện bằng dữ liệu Binance Futures:
  - Multi-timeframe technical analysis (1h, 4h, 1d, 1w)
  - Financial-Hacker indicators (Hurst, ALMA, SuperSmoother, LaguerreRSI, CMO)
  - Market regime detection
  - Support/Resistance (Pivot Points + Fibonacci)
  - Elliott Wave count (simplified)
  - Seasonality analysis
  - 30-day price forecast (multi-scenario)
  - Risk/Reward trade plan
"""

import json
import math
import os
import sys
import urllib.request
from datetime import datetime, timedelta
from typing import Dict, List, Optional, Tuple

import numpy as np
import pandas as pd
import talib

# ═══════════════════════════════════════════════════════════════
# CONFIG
# ═══════════════════════════════════════════════════════════════

SYMBOL = "XAGUSDT"
FUTURES_BASE = "https://fapi.binance.com"
TIMEFRAMES = {
    "15m": {"limit": 500, "weight": 0.05},
    "1h":  {"limit": 500, "weight": 0.15},
    "4h":  {"limit": 500, "weight": 0.30},
    "1d":  {"limit": 500, "weight": 0.35},
    "1w":  {"limit": 200, "weight": 0.15},
}

# Colors
def C(c, t): return f"\033[{c}m{t}\033[0m"
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

# ═══════════════════════════════════════════════════════════════
# DATA FETCHING
# ═══════════════════════════════════════════════════════════════

def fetch_klines(symbol: str, interval: str, limit: int = 500) -> pd.DataFrame:
    """Fetch candlestick data from Binance Futures."""
    url = f"{FUTURES_BASE}/fapi/v1/klines?symbol={symbol}&interval={interval}&limit={limit}"
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, timeout=30) as resp:
        raw = json.loads(resp.read().decode())

    df = pd.DataFrame(raw, columns=[
        "timestamp", "open", "high", "low", "close", "volume",
        "close_time", "quote_volume", "trades", "taker_buy_vol",
        "taker_buy_quote_vol", "ignore",
    ])
    for col in ["open", "high", "low", "close", "volume", "quote_volume"]:
        df[col] = pd.to_numeric(df[col], errors="coerce")
    df["timestamp"] = pd.to_datetime(df["timestamp"], unit="ms")
    df = df.set_index("timestamp")
    return df[["open", "high", "low", "close", "volume", "quote_volume"]]


def fetch_all_timeframes() -> Dict[str, pd.DataFrame]:
    """Fetch data for all timeframes."""
    data = {}
    for tf, cfg in TIMEFRAMES.items():
        try:
            data[tf] = fetch_klines(SYMBOL, tf, cfg["limit"])
            print(f"  {GR('✓')} {tf:>3s}: {len(data[tf])} candles  "
                  f"({data[tf].index[0].strftime('%Y-%m-%d')} → "
                  f"{data[tf].index[-1].strftime('%Y-%m-%d %H:%M')})")
        except Exception as e:
            print(f"  {RD('✗')} {tf:>3s}: {e}")
    return data


# ═══════════════════════════════════════════════════════════════
# TECHNICAL ANALYSIS
# ═══════════════════════════════════════════════════════════════

def compute_indicators(df: pd.DataFrame) -> Dict:
    """Compute comprehensive technical indicators."""
    o, h, l, c, v = df["open"], df["high"], df["low"], df["close"], df["volume"]
    result = {}

    # ── Trend Indicators ──
    result["sma_20"] = talib.SMA(c, 20)
    result["sma_50"] = talib.SMA(c, 50)
    result["sma_100"] = talib.SMA(c, 100)
    result["sma_200"] = talib.SMA(c, 200)
    result["ema_12"] = talib.EMA(c, 12)
    result["ema_26"] = talib.EMA(c, 26)
    result["ema_50"] = talib.EMA(c, 50)
    result["ema_200"] = talib.EMA(c, 200)

    # ── MACD ──
    macd, macd_signal, macd_hist = talib.MACD(c, 12, 26, 9)
    result["macd"] = macd
    result["macd_signal"] = macd_signal
    result["macd_hist"] = macd_hist

    # ── RSI ──
    result["rsi_14"] = talib.RSI(c, 14)
    result["rsi_7"] = talib.RSI(c, 7)

    # ── Bollinger Bands ──
    bb_upper, bb_mid, bb_lower = talib.BBANDS(c, 20, 2.0)
    result["bb_upper"] = bb_upper
    result["bb_mid"] = bb_mid
    result["bb_lower"] = bb_lower
    result["bb_pct"] = (c - bb_lower) / (bb_upper - bb_lower)

    # ── Stochastic ──
    slowk, slowd = talib.STOCH(h, l, c, fastk_period=14, slowk_period=3,
                                slowk_matype=0, slowd_period=3, slowd_matype=0)
    result["stoch_k"] = slowk
    result["stoch_d"] = slowd

    # ── ADX (Trend Strength) ──
    result["adx"] = talib.ADX(h, l, c, 14)
    result["plus_di"] = talib.PLUS_DI(h, l, c, 14)
    result["minus_di"] = talib.MINUS_DI(h, l, c, 14)

    # ── ATR (Volatility) ──
    result["atr_14"] = talib.ATR(h, l, c, 14)
    result["atr_7"] = talib.ATR(h, l, c, 7)

    # ── OBV ──
    result["obv"] = talib.OBV(c, v)

    # ── CCI ──
    result["cci"] = talib.CCI(h, l, c, 20)

    # ── Williams %R ──
    result["willr"] = talib.WILLR(h, l, c, 14)

    # ── Parabolic SAR ──
    result["sar"] = talib.SAR(h, l, acceleration=0.02, maximum=0.2)

    # ── Ichimoku ──
    high_9 = h.rolling(9).max()
    low_9 = l.rolling(9).min()
    high_26 = h.rolling(26).max()
    low_26 = l.rolling(26).min()
    high_52 = h.rolling(52).max()
    low_52 = l.rolling(52).min()
    result["tenkan"] = (high_9 + low_9) / 2
    result["kijun"] = (high_26 + low_26) / 2
    result["senkou_a"] = (result["tenkan"] + result["kijun"]) / 2
    result["senkou_b"] = (high_52 + low_52) / 2

    # ── VWAP ──
    typical = (h + l + c) / 3
    result["vwap"] = (typical * v).cumsum() / v.cumsum()

    # ── Financial Hacker: ALMA ──
    alma = compute_alma(c, period=50, offset=0.85, sigma=6)
    result["alma"] = alma

    # ── SuperSmoother (Ehlers) ──
    result["ss"] = compute_supersmoother(c, period=10)

    # ── Laguerre RSI ──
    result["lag_rsi"] = compute_laguerre_rsi(c, gamma=0.8)

    # ── CMO ──
    result["cmo"] = talib.CMO(c, 14)

    # ── Hurst Exponent ──
    result["hurst"] = compute_rolling_hurst(c, window=100)

    return result


# ═══════════════════════════════════════════════════════════════
# FINANCIAL-HACKER INDICATORS
# ═══════════════════════════════════════════════════════════════

def compute_alma(series: pd.Series, period: int = 50, offset: float = 0.85,
                 sigma: float = 6) -> pd.Series:
    """Arnaud Legoux Moving Average."""
    m = offset * (period - 1)
    s = period / sigma
    weights = np.array([np.exp(-((i - m) ** 2) / (2 * s * s)) for i in range(period)])
    weights = weights / weights.sum()

    alma = series.rolling(period).apply(lambda x: np.dot(x, weights), raw=True)
    return alma


def compute_supersmoother(series: pd.Series, period: int = 10) -> pd.Series:
    """Ehlers SuperSmoother filter."""
    a = math.exp(-1.414 * math.pi / period)
    b = 2 * a * math.cos(1.414 * math.pi / period)
    c2 = b
    c3 = -a * a
    c1 = 1 - c2 - c3

    ss = series.copy()
    for i in range(2, len(ss)):
        ss.iloc[i] = c1 * (series.iloc[i] + series.iloc[i - 1]) / 2 + c2 * ss.iloc[i - 1] + c3 * ss.iloc[i - 2]
    return ss


def compute_laguerre_rsi(series: pd.Series, gamma: float = 0.8) -> pd.Series:
    """Laguerre RSI (Ehlers)."""
    g = gamma
    L0 = np.zeros(len(series))
    L1 = np.zeros(len(series))
    L2 = np.zeros(len(series))
    L3 = np.zeros(len(series))

    for i in range(1, len(series)):
        L0[i] = (1 - g) * series.iloc[i] + g * L0[i - 1]
        L1[i] = -g * L0[i] + L0[i - 1] + g * L1[i - 1]
        L2[i] = -g * L1[i] + L1[i - 1] + g * L2[i - 1]
        L3[i] = -g * L2[i] + L2[i - 1] + g * L3[i - 1]

    def safe_num(val):
        return 0.0 if (np.isnan(val) or np.isinf(val)) else float(val)

    lag_rsi = np.full(len(series), 0.5)
    for i in range(1, len(series)):
        cu = 0.0
        cd = 0.0
        if safe_num(L0[i]) > safe_num(L1[i]):
            cu += safe_num(L0[i]) - safe_num(L1[i])
        else:
            cd += safe_num(L1[i]) - safe_num(L0[i])
        if safe_num(L1[i]) > safe_num(L2[i]):
            cu += safe_num(L1[i]) - safe_num(L2[i])
        else:
            cd += safe_num(L2[i]) - safe_num(L1[i])
        if safe_num(L2[i]) > safe_num(L3[i]):
            cu += safe_num(L2[i]) - safe_num(L3[i])
        else:
            cd += safe_num(L3[i]) - safe_num(L2[i])
        if cu + cd != 0:
            lag_rsi[i] = cu / (cu + cd)

    return pd.Series(lag_rsi, index=series.index)


def compute_rolling_hurst(series: pd.Series, window: int = 100) -> pd.Series:
    """Compute rolling Hurst exponent using R/S analysis."""
    hurst = pd.Series(np.nan, index=series.index)

    for i in range(window, len(series)):
        chunk = series.iloc[i - window:i].dropna().values
        if len(chunk) < window:
            continue
        try:
            h = _hurst_rs(chunk)
            hurst.iloc[i] = h
        except Exception:
            pass
    return hurst


def _hurst_rs(ts: np.ndarray) -> float:
    """Hurst exponent via R/S method."""
    n = len(ts)
    returns = np.diff(np.log(ts))
    max_k = int(np.floor(n / 2))

    rs_list = []
    ns = []
    for k in [10, 20, 40, 80, max_k]:
        if k < 4 or k > max_k:
            continue
        n_sub = n // k
        if n_sub < 1:
            continue
        rs_vals = []
        for i in range(k):
            chunk = returns[i * n_sub:(i + 1) * n_sub]
            if len(chunk) < 2:
                continue
            mean_c = chunk.mean()
            cumdev = np.cumsum(chunk - mean_c)
            R = cumdev.max() - cumdev.min()
            S = chunk.std()
            if S > 0:
                rs_vals.append(R / S)
        if rs_vals:
            rs_list.append(np.log(np.mean(rs_vals)))
            ns.append(np.log(n_sub))

    if len(ns) >= 2:
        slope = np.polyfit(ns, rs_list, 1)[0]
        return max(0.0, min(1.0, slope))
    return 0.5


# ═══════════════════════════════════════════════════════════════
# SUPPORT / RESISTANCE
# ═══════════════════════════════════════════════════════════════

def compute_pivot_points(df: pd.DataFrame) -> Dict:
    """Classic + Fibonacci Pivot Points."""
    h = df["high"].iloc[-1]
    l = df["low"].iloc[-1]
    c = df["close"].iloc[-1]

    pivot = (h + l + c) / 3
    r1 = 2 * pivot - l
    s1 = 2 * pivot - h
    r2 = pivot + (h - l)
    s2 = pivot - (h - l)
    r3 = h + 2 * (pivot - l)
    s3 = l - 2 * (h - pivot)

    # Fibonacci pivots
    fr1 = pivot + 0.382 * (h - l)
    fr2 = pivot + 0.618 * (h - l)
    fr3 = pivot + (h - l)
    fs1 = pivot - 0.382 * (h - l)
    fs2 = pivot - 0.618 * (h - l)
    fs3 = pivot - (h - l)

    return {
        "pivot": pivot,
        "classic": {"r3": r3, "r2": r2, "r1": r1, "s1": s1, "s2": s2, "s3": s3},
        "fibonacci": {"fr3": fr3, "fr2": fr2, "fr1": fr1, "fs1": fs1, "fs2": fs2, "fs3": fs3},
    }


def find_key_levels(df: pd.DataFrame, lookback: int = 100) -> Dict:
    """Find key S/R from swing highs/lows."""
    recent = df.tail(lookback)
    highs = recent["high"].values
    lows = recent["low"].values

    # Find local peaks and troughs
    from scipy.signal import argrelextrema

    peak_idx = argrelextrema(highs, np.greater, order=5)[0]
    trough_idx = argrelextrema(lows, np.less, order=5)[0]

    resistances = sorted(highs[peak_idx], reverse=True)[:5] if len(peak_idx) > 0 else []
    supports = sorted(lows[trough_idx], reverse=True)[:5] if len(trough_idx) > 0 else []

    return {"resistances": resistances, "supports": supports}


# ═══════════════════════════════════════════════════════════════
# MARKET REGIME
# ═══════════════════════════════════════════════════════════════

def detect_regime(hurst_val: float, adx_val: float, atr_val: float,
                  atr_avg: float) -> Dict:
    """Detect market regime from indicators."""
    if hurst_val > 0.55:
        if adx_val > 25:
            regime = "TRENDING"
            strategy = "Trend-following: ALMA, EhlersTrend, FH Composite"
        else:
            regime = "WEAK_TREND"
            strategy = "Cautious trend: MACD, EMA crossover"
    elif hurst_val < 0.45:
        regime = "MEAN_REVERTING"
        strategy = "Mean-reversion: RSI, Bollinger, LaguerreRSI"
    else:
        if atr_val > atr_avg * 1.5:
            regime = "VOLATILE"
            strategy = "Breakout: Momentum, Breakout, CMO"
        else:
            regime = "RANDOM_WALK"
            strategy = "Range-bound: Bollinger, Grid"

    return {"regime": regime, "strategy": strategy, "hurst": hurst_val,
            "adx": adx_val, "atr_ratio": atr_val / atr_avg if atr_avg > 0 else 1.0}


# ═══════════════════════════════════════════════════════════════
# ELLIOTT WAVE (Simplified)
# ═══════════════════════════════════════════════════════════════

def elliott_wave_analysis(df: pd.DataFrame) -> Dict:
    """Simplified Elliott Wave counting."""
    close = df["close"].values
    n = len(close)

    # Find recent swing points
    from scipy.signal import argrelextrema
    peak_idx = argrelextrema(close, np.greater, order=10)[0]
    trough_idx = argrelextrema(close, np.less, order=10)[0]

    # Combine and sort all extrema
    all_extrema = []
    for i in peak_idx:
        all_extrema.append((i, close[i], "peak"))
    for i in trough_idx:
        all_extrema.append((i, close[i], "trough"))
    all_extrema.sort(key=lambda x: x[0])

    # Take last 10 extrema
    recent = all_extrema[-10:] if len(all_extrema) >= 10 else all_extrema

    # Determine current wave position
    current_price = close[-1]
    if len(recent) >= 2:
        last = recent[-1]
        prev = recent[-2]
        if last[2] == "peak" and current_price < last[1]:
            wave_pos = "After Wave 3/5 peak → ABC correction likely"
            bias = "BEARISH short-term"
        elif last[2] == "trough" and current_price > last[1]:
            wave_pos = "After Wave 4 correction → Wave 5 up likely"
            bias = "BULLISH"
        else:
            wave_pos = "In transition"
            bias = "NEUTRAL"
    else:
        wave_pos = "Insufficient data"
        bias = "NEUTRAL"

    return {
        "extrema_count": len(all_extrema),
        "recent_extrema": recent[-5:],
        "wave_position": wave_pos,
        "bias": bias,
    }


# ═══════════════════════════════════════════════════════════════
# SEASONALITY
# ═══════════════════════════════════════════════════════════════

def seasonality_analysis(df: pd.DataFrame) -> Dict:
    """Monthly and day-of-week seasonality."""
    df_copy = df.copy()
    df_copy["month"] = df_copy.index.month
    df_copy["dow"] = df_copy.index.dayofweek
    df_copy["return"] = df_copy["close"].pct_change()

    monthly = df_copy.groupby("month")["return"].agg(["mean", "median", "count"])
    dow = df_copy.groupby("dow")["return"].agg(["mean", "median", "count"])

    current_month = datetime.now().month
    current_dow = datetime.now().weekday()

    return {
        "monthly": monthly.to_dict(),
        "dow": dow.to_dict(),
        "current_month_bias": "BULLISH" if monthly.loc[current_month, "mean"] > 0 else "BEARISH",
        "current_month_avg_return": monthly.loc[current_month, "mean"] * 100 if current_month in monthly.index else 0,
    }


# ═══════════════════════════════════════════════════════════════
# 30-DAY FORECAST
# ═══════════════════════════════════════════════════════════════

def forecast_30days(df: pd.DataFrame, indicators: Dict) -> Dict:
    """Multi-scenario 30-day price forecast."""
    close = df["close"].iloc[-1]
    daily = df

    # ── Method 1: Linear regression on momentum ──
    returns_20d = daily["close"].pct_change(20).iloc[-1]
    returns_10d = daily["close"].pct_change(10).iloc[-1]
    returns_5d = daily["close"].pct_change(5).iloc[-1]
    returns_5d = daily["close"].pct_change(5).iloc[-1]
    momentum_30 = returns_20d * 1.5  # extrapolate

    # ── Method 2: ATR-based projection ──
    atr = indicators["atr_14"].iloc[-1]
    atr_annual = atr / close  # daily ATR as % of price
    vol_30d = atr_annual * math.sqrt(30) * close

    # ── Method 3: Trend-based (MA direction) ──
    sma50 = indicators["sma_50"].iloc[-1]
    sma200_val = indicators["sma_200"].iloc[-1] if not pd.isna(indicators["sma_200"].iloc[-1]) else sma50
    trend_slope = (sma50 - sma200_val) / sma200_val
    trend_30d = close * (1 + trend_slope * 30 / 50)

    # ── Method 4: Fibonacci extensions ──
    # Use recent swing high/low
    recent_high = daily["high"].tail(60).max()
    recent_low = daily["low"].tail(60).min()
    diff = recent_high - recent_low
    fib_extensions = {
        "0.618": recent_high - 0.382 * diff,
        "0.786": recent_high - 0.214 * diff,
        "1.000": recent_high,
        "1.272": recent_high + 0.272 * diff,
        "1.618": recent_high + 0.618 * diff,
        "2.000": recent_high + 1.000 * diff,
    }

    # ── Composite scenarios ──
    # Bullish: uptrend continues
    bull_target = close + vol_30d * 0.8  # 80th percentile move
    # Base: moderate
    base_target = close + vol_30d * 0.1  # slight drift
    # Bearish: reversal
    bear_target = close - vol_30d * 0.6  # downside

    # Apply trend direction
    if trend_slope > 0:
        bull_target = max(bull_target, trend_30d * 1.05)
        base_target = max(base_target, trend_30d)
    else:
        bear_target = min(bear_target, trend_30d * 0.95)
        base_target = min(base_target, trend_30d)

    # RSI extreme adjustment
    rsi = indicators["rsi_14"].iloc[-1]
    if rsi > 70:
        bear_target = min(bear_target, close * 0.95)
    elif rsi < 30:
        bull_target = max(bull_target, close * 1.08)

    return {
        "current_price": close,
        "recent_high": recent_high,
        "recent_low": recent_low,
        "atr_14": atr,
        "vol_30d": vol_30d,
        "momentum_5d": returns_5d * 100,
        "momentum_10d": returns_10d * 100,
        "momentum_20d": returns_20d * 100,
        "trend_slope": trend_slope,
        "scenarios": {
            "bullish": {"target": bull_target, "change": (bull_target / close - 1) * 100},
            "base": {"target": base_target, "change": (base_target / close - 1) * 100},
            "bearish": {"target": bear_target, "change": (bear_target / close - 1) * 100},
        },
        "fib_extensions": fib_extensions,
        "probabilities": {
            "bullish": 0.35 if trend_slope > 0 else 0.25,
            "base": 0.40,
            "bearish": 0.25 if trend_slope > 0 else 0.35,
        },
    }


# ═══════════════════════════════════════════════════════════════
# TRADE PLAN
# ═══════════════════════════════════════════════════════════════

def generate_trade_plan(price: float, forecast: Dict, pivot: Dict,
                        regime: Dict, rsi: float, adx: float) -> Dict:
    """Generate actionable trade plan."""
    atr = forecast["atr_14"]

    if regime["regime"] in ("TRENDING", "WEAK_TREND"):
        # Trend-following plan
        direction = "LONG" if forecast["trend_slope"] > 0 else "SHORT"
    elif regime["regime"] == "MEAN_REVERTING":
        direction = "LONG" if rsi < 35 else "SHORT" if rsi > 65 else "WAIT"
    else:
        direction = "WAIT"

    if direction == "LONG":
        entry = price
        sl = price - 2.5 * atr
        tp1 = price + 2.0 * atr
        tp2 = price + 4.0 * atr
        tp3 = forecast["scenarios"]["bullish"]["target"]
        rr1 = (tp1 - entry) / (entry - sl) if entry != sl else 0
        rr2 = (tp2 - entry) / (entry - sl) if entry != sl else 0
    elif direction == "SHORT":
        entry = price
        sl = price + 2.5 * atr
        tp1 = price - 2.0 * atr
        tp2 = price - 4.0 * atr
        tp3 = forecast["scenarios"]["bearish"]["target"]
        rr1 = (entry - tp1) / (sl - entry) if sl != entry else 0
        rr2 = (entry - tp2) / (sl - entry) if sl != entry else 0
    else:
        return {"direction": "WAIT", "reason": "Market regime unclear, wait for confirmation"}

    return {
        "direction": direction,
        "entry": entry,
        "stop_loss": sl,
        "tp1": tp1, "tp2": tp2, "tp3": tp3,
        "rr1": rr1, "rr2": rr2,
        "risk_pct": abs(sl - entry) / entry * 100,
        "confidence": min(adx / 50.0, 1.0) * 80 + 10,
    }


# ═══════════════════════════════════════════════════════════════
# DISPLAY
# ═══════════════════════════════════════════════════════════════

def fmt_price(p):
    return f"${p:.3f}" if p < 1000 else f"${p:,.2f}"


def fmt_pct(p):
    if p > 0:
        return BGR(f"+{p:.2f}%")
    elif p < 0:
        return BRD(f"{p:.2f}%")
    return "0.00%"


def fmt_rsi(v):
    if v > 70: return BRD(f"{v:.1f} (Overbought)")
    if v > 55: return YL(f"{v:.1f}")
    if v > 45: return GR(f"{v:.1f}")
    if v < 30: return BGR(f"{v:.1f} (Oversold)")
    return f"{v:.1f}"


def display_section(title, content_lines):
    print(f"\n  {BOLD(LINE)}")
    print(f"  {BOLD(title)}")
    print(f"  {BOLD(LINE)}")
    for line in content_lines:
        print(f"  {line}")


def display_multi_tf_analysis(data: Dict, all_indicators: Dict):
    """Display multi-timeframe technical analysis."""
    display_section(
        f"📊 SECTION 1: MULTI-TIMEFRAME TECHNICAL ANALYSIS — {SYMBOL}",
        [],
    )

    for tf in ["15m", "1h", "4h", "1d", "1w"]:
        if tf not in all_indicators:
            continue
        ind = all_indicators[tf]
        df = data[tf]
        price = df["close"].iloc[-1]
        prev_price = df["close"].iloc[-2] if len(df) > 1 else price

        rsi = ind["rsi_14"].iloc[-1]
        macd_val = ind["macd"].iloc[-1]
        macd_sig = ind["macd_signal"].iloc[-1]
        macd_h = ind["macd_hist"].iloc[-1]
        bb_pct = ind["bb_pct"].iloc[-1]
        adx = ind["adx"].iloc[-1]
        atr = ind["atr_14"].iloc[-1]
        stoch_k = ind["stoch_k"].iloc[-1]
        stoch_d = ind["stoch_d"].iloc[-1]
        hurst = ind["hurst"].iloc[-1] if not pd.isna(ind["hurst"].iloc[-1]) else None
        lag = ind["lag_rsi"].iloc[-1]
        cmo = ind["cmo"].iloc[-1]
        alma = ind["alma"].iloc[-1] if not pd.isna(ind["alma"].iloc[-1]) else None
        sma20 = ind["sma_20"].iloc[-1]
        sma50 = ind["sma_50"].iloc[-1]
        ema50 = ind["ema_50"].iloc[-1]
        ema200 = ind["ema_200"].iloc[-1] if not pd.isna(ind["ema_200"].iloc[-1]) else None
        sar = ind["sar"].iloc[-1]
        tenkan = ind["tenkan"].iloc[-1]
        kijun = ind["kijun"].iloc[-1]

        # Trend determination
        trend = "BULLISH" if price > sma20 > sma50 else "BEARISH" if price < sma20 < sma50 else "NEUTRAL"
        trend_emoji = "🟢" if trend == "BULLISH" else "🔴" if trend == "BEARISH" else "⚪"

        print(f"\n  ┌{'─'*116}┐")
        print(f"  │ {trend_emoji} [{tf:>3s}]  Price: {BOLD(fmt_price(price))}  "
              f"({fmt_pct((price/prev_price - 1)*100)})  "
              f"Trend: {trend}  ATR: {atr:.3f}")
        print(f"  └{'─'*116}┘")

        # Moving averages
        ma_line = f"    SMA20:{fmt_price(sma20)}  SMA50:{fmt_price(sma50)}"
        if ema200 and not pd.isna(ema200):
            ma_line += f"  EMA200:{fmt_price(ema200)}"
            ema_cross = "🟢 ABOVE" if price > ema200 else "🔴 BELOW"
            ma_line += f"  → {ema_cross}"
        print(ma_line)

        # MACD
        macd_dir = "🟢 Bullish" if macd_val > macd_sig else "🔴 Bearish"
        print(f"    MACD: {macd_val:.3f}  Signal: {macd_sig:.3f}  "
              f"Hist: {macd_h:.4f}  → {macd_dir}")

        # RSI + Stoch
        print(f"    RSI: {fmt_rsi(rsi)}  Stoch K:{stoch_k:.1f} D:{stoch_d:.1f}")

        # Bollinger + CMO
        print(f"    BB %B: {bb_pct:.2f}  CMO: {cmo:.1f}  LagRSI: {lag:.3f}")

        # Hurst
        if hurst:
            h_str = GR(f"{hurst:.3f} Trending") if hurst > 0.55 else \
                    RD(f"{hurst:.3f} MeanRev") if hurst < 0.45 else \
                    YL(f"{hurst:.3f} Random")
            print(f"    Hurst: {h_str}  ADX: {adx:.1f}")

        # ALMA
        if alma and not pd.isna(alma):
            alma_dir = "🟢 Bullish" if price > alma else "🔴 Bearish"
            print(f"    ALMA: {fmt_price(alma)} → {alma_dir}")

        # SAR
        sar_dir = "🟢 Bullish" if sar < price else "🔴 Bearish"
        print(f"    SAR: {fmt_price(sar)} → {sar_dir}")

        # Ichimoku
        ich_dir = "🟢" if tenkan > kijun else "🔴"
        print(f"    Ichimoku: Tenkan:{fmt_price(tenkan)} Kijun:{fmt_price(kijun)} → {ich_dir}")


def display_signals(all_indicators: Dict):
    """Generate and display trading signals per timeframe."""
    display_section("🚦 SECTION 2: TRADING SIGNALS (Composite)", [])

    for tf in ["1h", "4h", "1d", "1w"]:
        if tf not in all_indicators:
            continue
        ind = all_indicators[tf]

        signals = []
        price = None

        # RSI signal
        rsi = ind["rsi_14"].iloc[-1]
        if rsi < 30:
            signals.append(("BUY", "RSI Oversold", 80))
        elif rsi < 40:
            signals.append(("BUY", "RSI Low", 50))
        elif rsi > 70:
            signals.append(("SELL", "RSI Overbought", 80))
        elif rsi > 60:
            signals.append(("SELL", "RSI High", 50))

        # MACD signal
        macd_h = ind["macd_hist"].iloc[-1]
        prev_h = ind["macd_hist"].iloc[-2] if len(ind["macd_hist"]) > 1 else 0
        if macd_h > 0 and prev_h <= 0:
            signals.append(("BUY", "MACD Cross Up", 75))
        elif macd_h < 0 and prev_h >= 0:
            signals.append(("SELL", "MACD Cross Down", 75))
        elif macd_h > 0:
            signals.append(("BUY", "MACD Positive", 55))
        else:
            signals.append(("SELL", "MACD Negative", 55))

        # Bollinger Band signal
        bb_pct = ind["bb_pct"].iloc[-1]
        if bb_pct < 0.05:
            signals.append(("BUY", "BB Lower Touch", 70))
        elif bb_pct > 0.95:
            signals.append(("SELL", "BB Upper Touch", 70))

        # Stochastic signal
        stoch_k = ind["stoch_k"].iloc[-1]
        stoch_d = ind["stoch_d"].iloc[-1]
        if stoch_k < 20 and stoch_d < 20:
            signals.append(("BUY", "Stoch Oversold", 65))
        elif stoch_k > 80 and stoch_d > 80:
            signals.append(("SELL", "Stoch Overbought", 65))

        # ADX + DI signal
        adx = ind["adx"].iloc[-1]
        plus_di = ind["plus_di"].iloc[-1]
        minus_di = ind["minus_di"].iloc[-1]
        if adx > 25:
            if plus_di > minus_di:
                signals.append(("BUY", f"ADX Trend Up ({adx:.0f})", 70))
            else:
                signals.append(("SELL", f"ADX Trend Down ({adx:.0f})", 70))

        # Laguerre RSI
        lag = ind["lag_rsi"].iloc[-1]
        if lag < 0.2:
            signals.append(("BUY", "LagRSI Oversold", 75))
        elif lag > 0.8:
            signals.append(("SELL", "LagRSI Overbought", 75))

        # CMO
        cmo = ind["cmo"].iloc[-1]
        if cmo > 40:
            signals.append(("BUY", "CMO Strong Momentum", 60))
        elif cmo < -40:
            signals.append(("SELL", "CMO Strong Bearish", 60))

        # Print signals
        buys = [s for s in signals if s[0] == "BUY"]
        sells = [s for s in signals if s[0] == "SELL"]

        net = len(buys) - len(sells)
        if net >= 3:
            consensus = BGR("▲▲ STRONG BUY")
        elif net >= 1:
            consensus = GR("▲ BUY")
        elif net <= -3:
            consensus = BRD("▼▼ STRONG SELL")
        elif net <= -1:
            consensus = RD("▼ SELL")
        else:
            consensus = YL("● NEUTRAL")

        print(f"\n    [{tf:>3s}] → {consensus}  (Buy:{len(buys)} Sell:{len(sells)})")
        for d, name, conf in signals:
            marker = GR("🟢") if d == "BUY" else RD("🔴")
            print(f"      {marker} {d:4s} | {name:<30s} | Confidence: {conf}%")


def display_sr(pivots: Dict, key_levels: Dict, forecast: Dict):
    """Display S/R levels."""
    display_section("🎯 SECTION 3: SUPPORT & RESISTANCE LEVELS", [])

    price = forecast["current_price"]
    piv = pivots["pivot"]

    print(f"\n    Current Price: {BOLD(fmt_price(price))}  |  Pivot: {fmt_price(piv)}")
    print()
    print(f"    {'Level':20s}  {'Classic':>12s}  {'Fibonacci':>12s}  {'Distance':>10s}")
    print(f"    {'─'*20}  {'─'*12}  {'─'*12}  {'─'*10}")

    levels = [
        ("Resistance 3", "r3", "fr3"),
        ("Resistance 2", "r2", "fr2"),
        ("Resistance 1", "r1", "fr1"),
        ("Support 1", "s1", "fs1"),
        ("Support 2", "s2", "fs2"),
        ("Support 3", "s3", "fs3"),
    ]

    for name, classic_key, fib_key in levels:
        cv = pivots["classic"][classic_key]
        fv = pivots["fibonacci"][fib_key]
        dist = (cv / price - 1) * 100
        marker = BRD("🔴") if "R" in name[0] else BGR("🟢") if "S" in name[0] else "  "
        print(f"    {marker} {name:18s}  {fmt_price(cv):>12s}  {fmt_price(fv):>12s}  {dist:+.2f}%")

    # Key levels from swing analysis
    if key_levels["resistances"]:
        print(f"\n    {BOLD('Key Resistances (Swing Highs)')}:")
        for r in key_levels["resistances"][:3]:
            dist = (r / price - 1) * 100
            print(f"      🔴 {fmt_price(r)}  ({dist:+.2f}%)")

    if key_levels["supports"]:
        print(f"\n    {BOLD('Key Supports (Swing Lows)')}:")
        for s in key_levels["supports"][:3]:
            dist = (s / price - 1) * 100
            print(f"      🟢 {fmt_price(s)}  ({dist:+.2f}%)")


def display_regime(regimes: Dict):
    """Display market regime per timeframe."""
    display_section("🌊 SECTION 4: MARKET REGIME DETECTION", [])

    for tf in ["1h", "4h", "1d", "1w"]:
        if tf not in regimes:
            continue
        r = regimes[tf]
        regime = r["regime"]
        hurst = r["hurst"]
        adx = r["adx"]

        if "TREND" in regime:
            emoji, color = "📈", GR
        elif "MEAN" in regime:
            emoji, color = "🔄", YL
        elif "VOLATILE" in regime:
            emoji, color = "⚡", BRD
        else:
            emoji, color = "❓", DM

        print(f"    [{tf:>3s}] {emoji} {BOLD(regime):20s}  "
              f"Hurst: {hurst:.3f}  ADX: {adx:.1f}  ATR ratio: {r['atr_ratio']:.2f}")
        print(f"          → {DM(r['strategy'])}")


def display_elliott(ew: Dict):
    """Display Elliott Wave analysis."""
    display_section("🌊 SECTION 5: ELLIOTT WAVE ANALYSIS (Simplified)", [])

    print(f"    Total extrema found: {ew['extrema_count']}")
    print(f"    Current position: {ew['wave_position']}")
    print(f"    Bias: {ew['bias']}")

    if ew["recent_extrema"]:
        print(f"\n    Recent swing points:")
        for idx, val, typ in ew["recent_extrema"]:
            emoji = "🔺" if typ == "peak" else "🔻"
            print(f"      {emoji} {fmt_price(val)} ({typ})")


def display_seasonality(season: Dict, df_daily: pd.DataFrame):
    """Display seasonality analysis."""
    display_section("📅 SECTION 6: SEASONALITY ANALYSIS", [])

    monthly = season["monthly"]
    print(f"\n    Monthly average returns (%):")
    print(f"    {'Month':8s}  {'Avg Return':>12s}  {'Median':>10s}  {'Count':>6s}  {'Signal':>8s}")
    print(f"    {'─'*8}  {'─'*12}  {'─'*10}  {'─'*6}  {'─'*8}")

    month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                   "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    for m in range(1, 13):
        if m in monthly["mean"]:
            avg = monthly["mean"][m] * 100
            med = monthly["median"][m] * 100
            cnt = int(monthly["count"][m])
            sig = BGR("🟢 BULL") if avg > 0.2 else BRD("🔴 BEAR") if avg < -0.2 else "⚪ Flat"
            current = " ← CURRENT" if m == datetime.now().month else ""
            print(f"    {month_names[m-1]:8s}  {avg:+10.3f}%  {med:+8.3f}%  {cnt:6d}  {sig}{current}")

    cm = datetime.now().month
    bias = season["current_month_bias"]
    avg_ret = season["current_month_avg_return"]
    print(f"\n    Current month ({month_names[cm-1]}) bias: {bias} "
          f"(avg: {fmt_pct(avg_ret)})")


def display_forecast(forecast: Dict):
    """Display 30-day forecast."""
    display_section("🔮 SECTION 7: 30-DAY PRICE FORECAST", [])

    price = forecast["current_price"]
    rh = forecast["recent_high"]
    rl = forecast["recent_low"]

    print(f"\n    Current: {BOLD(fmt_price(price))}  "
          f"60D High: {fmt_price(rh)}  Low: {fmt_price(rl)}")
    print(f"    ATR(14): {forecast['atr_14']:.3f}  "
          f"30D Volatility Band: ±{fmt_price(forecast['vol_30d'])}")
    print(f"    Momentum: 5D={fmt_pct(forecast['momentum_5d'])}  "
          f"10D={fmt_pct(forecast['momentum_10d'])}  "
          f"20D={fmt_pct(forecast['momentum_20d'])}")

    # Scenarios
    scenarios = forecast["scenarios"]
    probs = forecast["probabilities"]

    print(f"\n    {'Scenario':15s}  {'Target':>12s}  {'Change':>10s}  {'Probability':>12s}")
    print(f"    {'─'*15}  {'─'*12}  {'─'*10}  {'─'*12}")

    for name, label, emoji in [
        ("bullish", "Bullish", "🟢"),
        ("base", "Base", "⚪"),
        ("bearish", "Bearish", "🔴"),
    ]:
        s = scenarios[name]
        p = probs[name]
        print(f"    {emoji} {label:13s}  {fmt_price(s['target']):>12s}  "
              f"{fmt_pct(s['change']):>18s}  {p*100:.0f}%")

    # Fibonacci targets
    print(f"\n    {BOLD('Fibonacci Extension Targets:')}")
    for level, val in forecast["fib_extensions"].items():
        dist = (val / price - 1) * 100
        print(f"      Fib {level}: {fmt_price(val)}  ({dist:+.2f}%)")

    # Expected value
    ev = (probs["bullish"] * scenarios["bullish"]["change"] +
          probs["base"] * scenarios["base"]["change"] +
          probs["bearish"] * scenarios["bearish"]["change"])
    print(f"\n    Expected 30D return: {fmt_pct(ev)}")
    print(f"    Expected 30D price: {fmt_price(price * (1 + ev/100))}")

    # Weekly breakdown
    print(f"\n    {BOLD('Weekly Price Projections:')}")
    weekly_targets = []
    for wk in range(1, 5):
        frac = wk / 4.3  # fraction of month
        bull_w = price + (scenarios["bullish"]["target"] - price) * frac
        base_w = price + (scenarios["base"]["target"] - price) * frac
        bear_w = price + (scenarios["bearish"]["target"] - price) * frac
        date = datetime.now() + timedelta(weeks=wk)
        print(f"      Week {wk} ({date.strftime('%b %d')}):  "
              f"Bull: {fmt_price(bull_w)}  "
              f"Base: {fmt_price(base_w)}  "
              f"Bear: {fmt_price(bear_w)}")
        weekly_targets.append((date, bull_w, base_w, bear_w))

    return {"expected_return": ev, "weekly": weekly_targets}


def display_trade_plan(plan: Dict):
    """Display trade plan."""
    display_section("💰 SECTION 8: ACTIONABLE TRADE PLAN", [])

    if plan["direction"] == "WAIT":
        print(f"    {YL('⏳ WAIT')} — {plan['reason']}")
        return

    d = plan["direction"]
    emoji = "🟢 LONG" if d == "LONG" else "🔴 SHORT"
    print(f"\n    Direction: {BOLD(emoji)}")
    print(f"    Confidence: {plan['confidence']:.0f}%")
    print()
    print(f"    Entry:       {BOLD(fmt_price(plan['entry']))}")
    print(f"    Stop Loss:   {BRD(fmt_price(plan['stop_loss']))}  (risk: {plan['risk_pct']:.2f}%)")
    print(f"    Take Profit 1: {GR(fmt_price(plan['tp1']))}  (R:R = 1:{plan['rr1']:.1f})")
    print(f"    Take Profit 2: {GR(fmt_price(plan['tp2']))}  (R:R = 1:{plan['rr2']:.1f})")
    print(f"    Take Profit 3: {BGR(fmt_price(plan['tp3']))}  (full target)")

    # Position sizing
    equity = 10000  # example
    risk_per_trade = 0.02  # 2% risk
    risk_amount = equity * risk_per_trade
    sl_distance = abs(plan["entry"] - plan["stop_loss"])
    if sl_distance > 0:
        position_size = risk_amount / sl_distance
        notional = position_size * plan["entry"]
        leverage = notional / equity
        print(f"\n    Position Sizing (example $10,000 equity, 2% risk):")
        print(f"      Risk amount: ${risk_amount:.2f}")
        print(f"      Position: {position_size:.2f} units ({fmt_price(notional)} notional)")
        print(f"      Suggested leverage: {leverage:.1f}x")


# ═══════════════════════════════════════════════════════════════
# MAIN
# ═══════════════════════════════════════════════════════════════

def main():
    print()
    print(BOLD(SEP))
    print(BOLD(f"  🥈 {SYMBOL} (SILVER) — DEEP ANALYSIS + 30-DAY FORECAST"))
    print(f"  📅 {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} | "
          f"Data: Binance Futures | Timeframes: 15m, 1h, 4h, 1d, 1w")
    print(BOLD(SEP))

    # ── Fetch data ──
    print(f"\n  {CY('📡')} Fetching candlestick data from Binance Futures...")
    data = fetch_all_timeframes()

    if "1d" not in data:
        print(f"\n  {BRD('ERROR')}: Could not fetch daily data. Exiting.")
        sys.exit(1)

    # ── Compute indicators for all timeframes ──
    print(f"\n  {CY('⚙️')} Computing technical indicators...")
    all_indicators = {}
    for tf, df in data.items():
        try:
            all_indicators[tf] = compute_indicators(df)
            print(f"  {GR('✓')} {tf:>3s}: indicators computed")
        except Exception as e:
            print(f"  {RD('✗')} {tf:>3s}: {e}")

    # ── Display multi-TF analysis ──
    display_multi_tf_analysis(data, all_indicators)

    # ── Trading signals ──
    display_signals(all_indicators)

    # ── S/R from daily ──
    pivots = compute_pivot_points(data["1d"])
    key_levels = find_key_levels(data["1d"], lookback=100)

    # ── Forecast ──
    ind_1d = all_indicators["1d"]
    forecast = forecast_30days(data["1d"], ind_1d)

    display_sr(pivots, key_levels, forecast)

    # ── Regime detection ──
    regimes = {}
    for tf in ["1h", "4h", "1d", "1w"]:
        if tf in all_indicators:
            ind = all_indicators[tf]
            hurst = ind["hurst"].iloc[-1] if not pd.isna(ind["hurst"].iloc[-1]) else 0.5
            adx = ind["adx"].iloc[-1]
            atr = ind["atr_14"].iloc[-1]
            atr_avg = ind["atr_14"].mean()
            regimes[tf] = detect_regime(hurst, adx, atr, atr_avg)

    display_regime(regimes)

    # ── Elliott Wave ──
    try:
        ew = elliott_wave_analysis(data["1d"])
        display_elliott(ew)
    except Exception as e:
        print(f"\n    Elliott Wave: {RD(f'Skipped ({e})')}")

    # ── Seasonality ──
    try:
        season = seasonality_analysis(data["1d"])
        display_seasonality(season, data["1d"])
    except Exception as e:
        print(f"\n    Seasonality: {RD(f'Skipped ({e})')}")

    # ── 30-Day Forecast ──
    display_forecast(forecast)

    # ── Trade Plan ──
    rsi_1d = ind_1d["rsi_14"].iloc[-1]
    adx_1d = ind_1d["adx"].iloc[-1]
    regime_1d = regimes.get("1d", {"regime": "RANDOM_WALK"})

    plan = generate_trade_plan(
        data["1d"]["close"].iloc[-1], forecast, pivots,
        regime_1d, rsi_1d, adx_1d,
    )
    display_trade_plan(plan)

    # ── Summary ──
    display_section("📋 FINAL SUMMARY", [])

    price = data["1d"]["close"].iloc[-1]
    trend_1d = "BULLISH" if price > ind_1d["sma_50"].iloc[-1] else "BEARISH"
    regime_str = regimes.get("1d", {}).get("regime", "UNKNOWN")
    forecast_ev = forecast["scenarios"]["base"]["change"]

    print(f"""
    Symbol:         {BOLD(SYMBOL)} (Silver/USDT)
    Current Price:  {BOLD(fmt_price(price))}
    Daily Trend:    {BOLD(trend_1d)}
    Market Regime:  {BOLD(regime_str)}
    RSI(14):        {fmt_rsi(rsi_1d)}
    MACD:           {'Bullish' if ind_1d['macd_hist'].iloc[-1] > 0 else 'Bearish'}

    30-Day Forecast:
      Bullish:  {fmt_price(forecast['scenarios']['bullish']['target'])}  ({fmt_pct(forecast['scenarios']['bullish']['change'])})
      Base:     {fmt_price(forecast['scenarios']['base']['target'])}  ({fmt_pct(forecast['scenarios']['base']['change'])})
      Bearish:  {fmt_price(forecast['scenarios']['bearish']['target'])}  ({fmt_pct(forecast['scenarios']['bearish']['change'])})

    Trade Direction: {BOLD(plan['direction'])}
    Best Strategies: {regimes.get('1d', {}).get('strategy', 'N/A')}
    """)

    # Save report
    rdir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(rdir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    rpt_path = os.path.join(rdir, f"xagusdt_{ts}.json")

    report = {
        "timestamp": datetime.now().isoformat(),
        "symbol": SYMBOL,
        "current_price": price,
        "forecast": forecast["scenarios"],
        "regime": {tf: r["regime"] for tf, r in regimes.items()},
        "trade_plan": plan,
        "indicators_daily": {
            "rsi": rsi_1d,
            "adx": adx_1d,
            "macd_hist": ind_1d["macd_hist"].iloc[-1],
            "atr": ind_1d["atr_14"].iloc[-1],
            "bb_pct": ind_1d["bb_pct"].iloc[-1],
        },
    }

    with open(rpt_path, "w") as f:
        json.dump(report, f, indent=2, default=str)

    print(f"  📁 Report: {BOLD(rpt_path)}")
    print()
    print(BOLD(SEP))


if __name__ == "__main__":
    main()
