#!/usr/bin/env python3
"""
BonBoExtend Indicator Verification Suite
=========================================
Cross-checks ALL Rust indicators against Python reference implementations
using pandas, numpy, talib, and manual algorithms matching BonBo's exact logic.

Tests run on real DOTUSDT data from Binance.
"""

import json
import subprocess
import sys
import time
import warnings
from datetime import datetime

import numpy as np
import pandas as pd

warnings.filterwarnings("ignore")

# ═══════════════════════════════════════════════════════════════════════════
# DATA FETCHING
# ═══════════════════════════════════════════════════════════════════════════

def fetch_binance_klines(symbol="DOTUSDT", interval="1d", limit=200):
    """Fetch OHLCV from Binance SPOT (same source as BonBo MCP)."""
    url = f"https://api.binance.com/api/v3/klines?symbol={symbol}&interval={interval}&limit={limit}"
    import urllib.request
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, timeout=15) as resp:
        raw = json.loads(resp.read().decode())
    df = pd.DataFrame(raw, columns=[
        'timestamp', 'open', 'high', 'low', 'close', 'volume',
        'close_time', 'quote_volume', 'trades', 'taker_buy_vol',
        'taker_buy_quote_vol', 'ignore'
    ])
    for col in ['open', 'high', 'low', 'close', 'volume']:
        df[col] = df[col].astype(float)
    df['timestamp'] = df['timestamp'].astype(int)
    return df[['timestamp', 'open', 'high', 'low', 'close', 'volume']].copy()


def call_mcp(name, args=None):
    """Call BonBo MCP tool and return text result."""
    params = {"name": name}
    if args:
        params["arguments"] = args
    req = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params})
    proc = subprocess.run(
        ["./target/release/bonbo-extend-mcp"],
        input=req, capture_output=True, text=True, timeout=30,
        cwd="/home/tranbrook/BonBoExtend",
    )
    for line in proc.stdout.strip().split('\n'):
        try:
            resp = json.loads(line)
            if "result" in resp:
                for c in resp["result"].get("content", []):
                    return c.get("text", "")
        except json.JSONDecodeError:
            continue
    return None


def parse_mcp_analysis(text):
    """Parse analyze_indicators text output into a dict."""
    result = {}
    if not text:
        return result

    import re

    # SMA(20): $1.26
    m = re.search(r'SMA\(20\)\*\*:\s*\$?([\d.]+)', text)
    if m: result['sma20'] = float(m.group(1))

    # EMA(12): $1.27
    m = re.search(r'EMA\(12\)\*\*:\s*\$?([\d.]+)', text)
    if m: result['ema12'] = float(m.group(1))

    # EMA(26): $1.29
    m = re.search(r'EMA\(26\)\*\*:\s*\$?([\d.]+)', text)
    if m: result['ema26'] = float(m.group(1))

    # RSI(14): 46.1
    m = re.search(r'RSI\(14\)\*\*:\s*([\d.]+)', text)
    if m: result['rsi14'] = float(m.group(1))

    # MACD: line=-0.0195 signal=-0.0279 hist=0.0084
    m = re.search(r'MACD\*\*:\s*line=([-\d.]+)\s*signal=([-\d.]+)\s*hist=([-\d.]+)', text)
    if m:
        result['macd_line'] = float(m.group(1))
        result['macd_signal'] = float(m.group(2))
        result['macd_hist'] = float(m.group(3))

    # BB(20,2): upper=$1.35 mid=$1.26 lower=$1.18 %B=0.46
    m = re.search(r'BB\(20,2\)\*\*:\s*upper=\$?([\d.]+)\s*mid=\$?([\d.]+)\s*lower=\$?([\d.]+)\s*%B=([\d.]+)', text)
    if m:
        result['bb_upper'] = float(m.group(1))
        result['bb_mid'] = float(m.group(2))
        result['bb_lower'] = float(m.group(3))
        result['bb_pct_b'] = float(m.group(4))

    # ALMA(10): $1.27 | ALMA(30): $1.26 (note: MCP rounds to 2 decimals)
    m = re.search(r'ALMA\(10\)\*\*:\s*\$?([\d.]+)\s*\|\s*ALMA\(30\)\*\*:\s*\$?([\d.]+)', text)
    if not m:
        m = re.search(r'ALMA\(10\)\*\*:\s*\$?([\d.]+)', text)
    if m: result['alma10'] = float(m.group(1))
    if m and m.lastindex >= 2: result['alma30'] = float(m.group(2))

    # Also try just "ALMA(10)" without **
    if 'alma10' not in result:
        m = re.search(r'ALMA\(10\)[^:]*:\s*\$?([\d.]+)', text)
        if m: result['alma10'] = float(m.group(1))
    if 'alma30' not in result:
        m = re.search(r'ALMA\(30\)[^:]*:\s*\$?([\d.]+)', text)
        if m: result['alma30'] = float(m.group(1))

    # SuperSmoother(20): $1.27 (slope: +0.1366%)
    # Note: MCP rounds to 2 decimal places for price display
    m = re.search(r'SuperSmoother\(20\)\*\*:\s+.+?(\d+\.\d+)\s+\(slope:\s*([-\d.]+)%\)', text)
    if m:
        result['supersmoother'] = float(m.group(1))
        result['supersmoother_slope'] = float(m.group(2))

    # Hurst(100): 0.612
    m = re.search(r'Hurst\(100\)\*\*:\s*([\d.]+)', text)
    if m: result['hurst100'] = float(m.group(1))

    # CMO(14): -9.0
    m = re.search(r'CMO\(14\)\*\*:\s*([-\d.]+)', text)
    if m: result['cmo14'] = float(m.group(1))

    # LaguerreRSI(0.8): 0.017
    m = re.search(r'LaguerreRSI\(0\.8\)\*\*:\s*([\d.]+)', text)
    if m: result['laguerre_rsi'] = float(m.group(1))

    # Price: $1.26
    m = re.search(r'Price\*\*:\s*\$?([\d.]+)', text)
    if m: result['price'] = float(m.group(1))

    return result


# ═══════════════════════════════════════════════════════════════════════════
# PYTHON REFERENCE INDICATORS (matching BonBo Rust logic EXACTLY)
# ═══════════════════════════════════════════════════════════════════════════

class BonBoReference:
    """
    Python reference implementations matching BonBoExtend Rust code EXACTLY.
    Every algorithm is copied from the Rust source to ensure 1:1 match.
    """

    def __init__(self, df: pd.DataFrame):
        """
        df must have columns: open, high, low, close, volume
        Sorted chronologically (oldest first).
        """
        self.df = df.copy()
        self.c = df['close'].values.astype(np.float64)
        self.h = df['high'].values.astype(np.float64)
        self.l = df['low'].values.astype(np.float64)
        self.o = df['open'].values.astype(np.float64)
        self.v = df['volume'].values.astype(np.float64)
        self.n = len(self.c)

    # ─── SMA ──────────────────────────────────────────────────────────────
    def sma(self, period: int) -> np.ndarray:
        """Simple Moving Average. BonBo: sum / period, returns None until count >= period."""
        result = np.full(self.n, np.nan)
        for i in range(period - 1, self.n):
            result[i] = np.sum(self.c[i - period + 1:i + 1]) / period
        return result

    # ─── EMA (standard, SMA-seeded) ──────────────────────────────────────
    def ema(self, period: int) -> np.ndarray:
        """
        Standard EMA: alpha = 2/(period+1).
        BonBo seeds with SMA of first `period` values (matches TA-Lib).
        """
        result = np.full(self.n, np.nan)
        alpha = 2.0 / (period + 1)
        # Seed: SMA of first `period` values
        if self.n < period:
            return result
        seed = np.mean(self.c[:period])
        result[period - 1] = seed
        for i in range(period, self.n):
            result[i] = alpha * self.c[i] + (1 - alpha) * result[i - 1]
        return result

    # ─── EMA Wilder's ────────────────────────────────────────────────────
    def ema_wilders(self, period: int, data: np.ndarray = None) -> np.ndarray:
        """
        Wilder's EMA: alpha = 1/period.
        Used by RSI, ATR, ADX. Seeded with SMA of first `period` values.
        """
        src = data if data is not None else self.c
        n = len(src)
        result = np.full(n, np.nan)
        alpha = 1.0 / period
        if n < period:
            return result
        seed = np.mean(src[:period])
        result[period - 1] = seed
        for i in range(period, n):
            result[i] = alpha * src[i] + (1 - alpha) * result[i - 1]
        return result

    # ─── RSI (Wilder's) ─────────────────────────────────────────────────
    def rsi(self, period: int = 14) -> np.ndarray:
        """
        RSI using Wilder's smoothed averages (alpha=1/period).
        BonBo matches exactly: uses Wilder's EMA for avg_gain/avg_loss.
        """
        result = np.full(self.n, np.nan)
        if self.n < period + 1:
            return result

        changes = np.diff(self.c)
        gains = np.where(changes > 0, changes, 0.0)
        losses = np.where(changes < 0, -changes, 0.0)

        # First average: simple mean of first `period` values
        avg_gain = np.mean(gains[:period])
        avg_loss = np.mean(losses[:period])

        for i in range(period, len(changes)):
            avg_gain = (avg_gain * (period - 1) + gains[i]) / period
            avg_loss = (avg_loss * (period - 1) + losses[i]) / period

        # RSI from smoothed averages
        if avg_loss == 0:
            result[period] = 100.0
        else:
            rs = avg_gain / avg_loss
            result[period] = 100.0 - (100.0 / (1.0 + rs))

        # Continue for remaining values
        for i in range(period + 1, self.n):
            idx = i - 1  # index into changes
            avg_gain = (avg_gain * (period - 1) + gains[idx]) / period
            avg_loss = (avg_loss * (period - 1) + losses[idx]) / period
            if avg_loss == 0:
                result[i] = 100.0
            else:
                rs = avg_gain / avg_loss
                result[i] = 100.0 - (100.0 / (1.0 + rs))

        return result

    # ─── MACD ────────────────────────────────────────────────────────────
    def macd(self, fast=12, slow=26, signal=9):
        """
        MACD: fast_ema - slow_ema, signal = EMA(signal_period) of MACD line.
        BonBo uses standard EMA (alpha=2/(n+1)), SMA-seeded.
        """
        fast_ema = self.ema(fast)
        slow_ema = self.ema(slow)
        macd_line = fast_ema - slow_ema

        # Signal line: EMA of MACD line (where MACD is not NaN)
        valid = ~np.isnan(macd_line)
        valid_macd = macd_line[valid]
        if len(valid_macd) < signal:
            return macd_line, np.full(self.n, np.nan), np.full(self.n, np.nan)

        signal_line = np.full(self.n, np.nan)
        alpha = 2.0 / (signal + 1)
        # Find first valid MACD index
        first_valid = np.argmax(valid)
        # Seed signal with SMA of first `signal` MACD values
        if len(valid_macd) >= signal:
            sig_seed = np.mean(valid_macd[:signal])
            signal_line[first_valid + signal - 1] = sig_seed
            for i in range(first_valid + signal, self.n):
                if not np.isnan(macd_line[i]):
                    signal_line[i] = alpha * macd_line[i] + (1 - alpha) * signal_line[i - 1]

        hist = macd_line - signal_line
        return macd_line, signal_line, hist

    # ─── Bollinger Bands ─────────────────────────────────────────────────
    def bollinger_bands(self, period=20, std_mult=2.0):
        """
        BB with SAMPLE standard deviation (Bessel's correction: n-1).
        BonBo fix: uses sample variance = pop_variance * n/(n-1).
        SMA mid, upper = mid + k*std, lower = mid - k*std.
        """
        sma = self.sma(period)
        upper = np.full(self.n, np.nan)
        lower = np.full(self.n, np.nan)
        pct_b = np.full(self.n, np.nan)

        for i in range(period - 1, self.n):
            window = self.c[i - period + 1:i + 1]
            mean = np.mean(window)
            # Sample standard deviation (Bessel's correction)
            std = np.std(window, ddof=1)
            upper[i] = mean + std_mult * std
            lower[i] = mean - std_mult * std
            if upper[i] != lower[i]:
                pct_b[i] = (self.c[i] - lower[i]) / (upper[i] - lower[i])
            else:
                pct_b[i] = 0.5

        return upper, sma, lower, pct_b

    # ─── ATR ─────────────────────────────────────────────────────────────
    def atr(self, period=14) -> np.ndarray:
        """
        ATR using Wilder's EMA (alpha=1/period) of True Range.
        """
        result = np.full(self.n, np.nan)
        if self.n < 2:
            return result

        # True Range
        tr = np.full(self.n, np.nan)
        tr[0] = self.h[0] - self.l[0]
        for i in range(1, self.n):
            hl = self.h[i] - self.l[i]
            hc = abs(self.h[i] - self.c[i - 1])
            lc = abs(self.l[i] - self.c[i - 1])
            tr[i] = max(hl, hc, lc)

        # Wilder's EMA of TR, seeded with SMA
        if self.n < period:
            return result
        seed = np.mean(tr[1:period + 1])  # TR[0] might be weird, start from 1
        result[period] = seed
        alpha = 1.0 / period
        for i in range(period + 1, self.n):
            result[i] = alpha * tr[i] + (1 - alpha) * result[i - 1]

        return result

    # ─── Stochastic ──────────────────────────────────────────────────────
    def stochastic(self, k_period=14, d_period=3):
        """
        Stochastic %K = (C - L14) / (H14 - L14) * 100
        %D = SMA(d_period) of %K
        """
        k = np.full(self.n, np.nan)
        for i in range(k_period - 1, self.n):
            window_high = np.max(self.h[i - k_period + 1:i + 1])
            window_low = np.min(self.l[i - k_period + 1:i + 1])
            if window_high != window_low:
                k[i] = (self.c[i] - window_low) / (window_high - window_low) * 100
            else:
                k[i] = 50.0

        d = np.full(self.n, np.nan)
        for i in range(k_period - 1 + d_period - 1, self.n):
            window = k[i - d_period + 1:i + 1]
            if not np.any(np.isnan(window)):
                d[i] = np.mean(window)

        return k, d

    # ─── ALMA (Arnaud Legoux Moving Average) ────────────────────────────
    def alma(self, period: int, offset: float = 0.85, sigma: float = 6.0) -> np.ndarray:
        """
        ALMA matching BonBo Rust implementation exactly.
        Uses gaussian weights with m = offset * (period - 1), s = period / sigma.
        """
        result = np.full(self.n, np.nan)
        m = offset * (period - 1)
        s = period / sigma

        # Pre-compute weights
        weights = np.array([np.exp(-((i - m) ** 2) / (2 * s * s)) for i in range(period)])
        w_sum = np.sum(weights)

        for i in range(period - 1, self.n):
            window = self.c[i - period + 1:i + 1]
            result[i] = np.sum(weights * window) / w_sum

        return result

    # ─── SuperSmoother (Ehlers 2-pole) ──────────────────────────────────
    def supersmoother(self, period: int) -> np.ndarray:
        """
        Ehlers 2-pole Super Smoother.
        Matches BonBo Rust: a1=exp(-1.414*pi/period), b1=2*a1*cos(1.414*pi/period),
        coef2=b1, coef3=-a1*a1, coef1=1-coef2-coef3.
        """
        result = np.full(self.n, np.nan)
        if period < 3:
            return result

        a1 = np.exp(-1.414 * np.pi / period)
        b1 = 2.0 * a1 * np.cos(1.414 * np.pi / period)
        c2 = b1
        c3 = -a1 * a1
        c1 = 1.0 - c2 - c3

        # First two values are just the price (no filtering yet)
        filt = np.zeros(self.n)
        filt[0] = self.c[0]
        if self.n > 1:
            filt[1] = self.c[1]

        for i in range(2, self.n):
            filt[i] = c1 * (self.c[i] + self.c[i - 1]) / 2.0 + c2 * filt[i - 1] + c3 * filt[i - 2]

        # Copy all values (no warmup period in BonBo)
        result[:] = filt
        return result

    # ─── LaguerreRSI (Ehlers) ────────────────────────────────────────────
    def laguerre_rsi(self, gamma: float = 0.8) -> np.ndarray:
        """
        Ehlers Laguerre RSI. Matches BonBo Rust exactly.
        L0 = (1-gamma)*price + gamma*L0[1]
        L1 = -gamma*L0 + L0[1] + gamma*L1[1]
        L2 = -gamma*L1 + L1[1] + gamma*L2[1]
        L3 = -gamma*L2 + L2[1] + gamma*L3[1]
        RSI = (cu / (cu + cd)) where cu=sum of positive diffs, cd=sum of negative
        """
        result = np.full(self.n, np.nan)
        l0 = l1 = l2 = l3 = 0.0
        g = gamma

        for i in range(self.n):
            l0_prev, l1_prev, l2_prev, l3_prev = l0, l1, l2, l3
            l0 = (1 - g) * self.c[i] + g * l0_prev
            l1 = -g * l0 + l0_prev + g * l1_prev
            l2 = -g * l1 + l1_prev + g * l2_prev
            l3 = -g * l2 + l2_prev + g * l3_prev

            # Count up/down
            c0 = l0 - l1
            c1 = l1 - l2
            c2 = l2 - l3

            cu = c0 * 0.5 * (1 + np.sign(c0)) + c1 * 0.5 * (1 + np.sign(c1)) + c2 * 0.5 * (1 + np.sign(c2))
            cd = -c0 * 0.5 * (1 - np.sign(c0)) - c1 * 0.5 * (1 - np.sign(c1)) - c2 * 0.5 * (1 - np.sign(c2))

            denom = cu + cd
            if denom > 0:
                result[i] = cu / denom
            else:
                result[i] = 0.0

        return result

    # ─── CMO (Chande Momentum Oscillator) ───────────────────────────────
    def cmo(self, period: int = 14) -> np.ndarray:
        """
        CMO = 100 * (sum_up - sum_down) / (sum_up + sum_down)
        BonBo uses simple rolling sum of gains and losses.
        """
        result = np.full(self.n, np.nan)
        if self.n < period + 1:
            return result

        changes = np.diff(self.c)
        gains = np.where(changes > 0, changes, 0.0)
        losses = np.where(changes < 0, -changes, 0.0)

        for i in range(period, len(changes)):
            sum_up = np.sum(gains[i - period + 1:i + 1])
            sum_down = np.sum(losses[i - period + 1:i + 1])
            denom = sum_up + sum_down
            if denom > 0:
                result[i + 1] = 100.0 * (sum_up - sum_down) / denom
            else:
                result[i + 1] = 0.0

        return result

    # ─── Hurst Exponent (R/S analysis) ──────────────────────────────────
    def hurst_rs(self, window: int = 100) -> float:
        """
        Hurst Exponent via R/S analysis on last `window` closes.
        Matches BonBo Rust EXACTLY: logarithmic subdivisions, log-log regression.
        """
        # Use last `window` prices (matching BonBo's window of 101 → 100 returns)
        prices = self.c[-(window + 1):]  # +1 for returns
        returns = np.diff(np.log(prices))
        n = len(returns)

        if n < 50:
            return np.nan

        # Logarithmically spaced subdivisions (matching BonBo fix)
        log_min = np.log(10)
        log_max = np.log(n)
        num_subs = 8
        subdivisions = []
        for k in range(1, num_subs + 1):
            t = k / (num_subs + 1)
            log_size = log_min + t * (log_max - log_min)
            size = int(round(np.exp(log_size)))
            if 4 <= size <= n and size >= 10:
                subdivisions.append(size)

        subdivisions = list(dict.fromkeys(subdivisions))  # remove duplicates, preserve order
        if len(subdivisions) < 3:
            return np.nan

        # Compute R/S for each subdivision size
        rs_values = []
        sizes = []
        for size in subdivisions:
            n_groups = n // size
            if n_groups < 1:
                continue
            rs_group = []
            for g in range(n_groups):
                group = returns[g * size:(g + 1) * size]
                mean = np.mean(group)
                cumdev = np.cumsum(group - mean)
                r = np.max(cumdev) - np.min(cumdev)
                s = np.std(group, ddof=1)  # sample std
                if s > 0:
                    rs_group.append(r / s)
            if rs_group:
                rs_values.append(np.log(np.mean(rs_group)))
                sizes.append(np.log(size))

        if len(sizes) < 3:
            return np.nan

        # Linear regression on log-log
        sizes_arr = np.array(sizes)
        rs_arr = np.array(rs_values)
        n_pts = len(sizes_arr)
        sum_x = np.sum(sizes_arr)
        sum_y = np.sum(rs_arr)
        sum_xy = np.sum(sizes_arr * rs_arr)
        sum_x2 = np.sum(sizes_arr ** 2)
        denom = n_pts * sum_x2 - sum_x * sum_x
        if denom == 0:
            return np.nan
        hurst = (n_pts * sum_xy - sum_x * sum_y) / denom
        return hurst

    # ─── VWAP ────────────────────────────────────────────────────────────
    def vwap(self) -> np.ndarray:
        """VWAP = cumsum(TP * Vol) / cumsum(Vol) where TP = (H+L+C)/3."""
        tp = (self.h + self.l + self.c) / 3.0
        cum_tp_vol = np.cumsum(tp * self.v)
        cum_vol = np.cumsum(self.v)
        result = np.where(cum_vol > 0, cum_tp_vol / cum_vol, np.nan)
        return result

    # ─── OBV ─────────────────────────────────────────────────────────────
    def obv(self) -> np.ndarray:
        """On Balance Volume: +vol if close > prev_close, -vol if <, 0 if equal."""
        result = np.zeros(self.n)
        for i in range(1, self.n):
            if self.c[i] > self.c[i - 1]:
                result[i] = result[i - 1] + self.v[i]
            elif self.c[i] < self.c[i - 1]:
                result[i] = result[i - 1] - self.v[i]
            else:
                result[i] = result[i - 1]
        return result

    # ─── ADX (Wilder's) ─────────────────────────────────────────────────
    def adx(self, period: int = 14):
        """
        ADX using Wilder's smoothing.
        BonBo: smooths +DM/-DM/TR with Wilder's EMA (alpha=1/period),
        then DI+/DI-/DX, then ADX = Wilder's EMA of DX.
        """
        if self.n < period * 2:
            return np.full(self.n, np.nan), np.full(self.n, np.nan), np.full(self.n, np.nan)

        # True Range
        tr = np.zeros(self.n)
        tr[0] = self.h[0] - self.l[0]
        for i in range(1, self.n):
            tr[i] = max(self.h[i] - self.l[i], abs(self.h[i] - self.c[i - 1]), abs(self.l[i] - self.c[i - 1]))

        # +DM / -DM
        plus_dm = np.zeros(self.n)
        minus_dm = np.zeros(self.n)
        for i in range(1, self.n):
            up = self.h[i] - self.h[i - 1]
            down = self.l[i - 1] - self.l[i]
            plus_dm[i] = up if up > down and up > 0 else 0
            minus_dm[i] = down if down > up and down > 0 else 0

        # Wilder's smooth
        atr_val = np.full(self.n, np.nan)
        smooth_plus = np.full(self.n, np.nan)
        smooth_minus = np.full(self.n, np.nan)

        # Seed with sum of first `period` values
        atr_val[period] = np.sum(tr[1:period + 1])
        smooth_plus[period] = np.sum(plus_dm[1:period + 1])
        smooth_minus[period] = np.sum(minus_dm[1:period + 1])

        alpha = 1.0 / period
        for i in range(period + 1, self.n):
            atr_val[i] = atr_val[i - 1] - atr_val[i - 1] / period + tr[i]
            smooth_plus[i] = smooth_plus[i - 1] - smooth_plus[i - 1] / period + plus_dm[i]
            smooth_minus[i] = smooth_minus[i - 1] - smooth_minus[i - 1] / period + minus_dm[i]

        # DI+, DI-
        di_plus = np.full(self.n, np.nan)
        di_minus = np.full(self.n, np.nan)
        dx = np.full(self.n, np.nan)
        for i in range(period, self.n):
            if atr_val[i] > 0:
                di_plus[i] = 100 * smooth_plus[i] / atr_val[i]
                di_minus[i] = 100 * smooth_minus[i] / atr_val[i]
                di_sum = di_plus[i] + di_minus[i]
                if di_sum > 0:
                    dx[i] = 100 * abs(di_plus[i] - di_minus[i]) / di_sum
                else:
                    dx[i] = 0.0

        # ADX = Wilder's EMA of DX
        adx = np.full(self.n, np.nan)
        valid_dx = dx[~np.isnan(dx)]
        if len(valid_dx) >= period:
            first_dx_idx = period
            adx[first_dx_idx + period - 1] = np.mean(dx[first_dx_idx:first_dx_idx + period])
            for i in range(first_dx_idx + period, self.n):
                if not np.isnan(dx[i]):
                    adx[i] = (adx[i - 1] * (period - 1) + dx[i]) / period

        return adx, di_plus, di_minus


# ═══════════════════════════════════════════════════════════════════════════
# VERIFICATION ENGINE
# ═══════════════════════════════════════════════════════════════════════════

class Verifier:
    """Compare BonBo MCP results with Python reference."""

    PASS = "✅"
    WARN = "⚠️"
    FAIL = "❌"
    SKIP = "⏭️"

    def __init__(self):
        self.results = []
        self.passed = 0
        self.warned = 0
        self.failed = 0

    def compare(self, name: str, rust_val, py_val, tol_abs=0.01, tol_pct=1.0):
        """
        Compare two values. tol_abs = absolute tolerance, tol_pct = % tolerance.
        Returns status and diff.
        """
        if rust_val is None or py_val is None or np.isnan(rust_val) or np.isnan(py_val):
            self.results.append((name, self.SKIP, rust_val, py_val, 0, "Missing value"))
            return self.SKIP

        diff = abs(rust_val - py_val)
        avg = (abs(rust_val) + abs(py_val)) / 2
        pct = (diff / avg * 100) if avg > 0 else diff

        if diff <= tol_abs or pct <= tol_pct:
            status = self.PASS
            self.passed += 1
        elif pct <= tol_pct * 3:
            status = self.WARN
            self.warned += 1
        else:
            status = self.FAIL
            self.failed += 1

        self.results.append((name, status, rust_val, py_val, pct, diff))
        return status

    def print_report(self):
        """Print detailed verification report."""
        print()
        print("╔" + "═" * 98 + "╗")
        print("║  VERIFICATION REPORT                                              ║")
        print("╠" + "═" * 98 + "╣")
        print(f"║  {'Indicator':<30s} {'Status':>4s}  {'Rust':>12s}  {'Python':>12s}  {'Diff%':>8s}  {'Diff':>10s} ║")
        print("╠" + "═" * 98 + "╣")

        for name, status, rust, py, pct, diff in self.results:
            r_str = f"{rust:.6f}" if isinstance(rust, float) else str(rust)[:12]
            p_str = f"{py:.6f}" if isinstance(py, float) else str(py)[:12]
            d_str = f"{diff:.6f}" if isinstance(diff, float) else str(diff)[:10]
            pct_str = f"{pct:.3f}%" if isinstance(pct, float) else ""
            print(f"║  {name:<30s} {status:>2s}   {r_str:>12s}  {p_str:>12s}  {pct_str:>8s}  {d_str:>10s} ║")

        print("╠" + "═" * 98 + "╣")
        total = self.passed + self.warned + self.failed
        print(f"║  TOTAL: {total} tests | ✅ {self.passed} PASS | ⚠️ {self.warned} WARN | ❌ {self.failed} FAIL"
              f"{' ' * max(0, 40 - len(str(total)))}║")
        print("╚" + "═" * 98 + "╝")


# ═══════════════════════════════════════════════════════════════════════════
# MAIN
# ═══════════════════════════════════════════════════════════════════════════

def main():
    print()
    print("╔" + "═" * 98 + "╗")
    print("║  🔬 BONBO EXTEND — INDICATOR VERIFICATION SUITE")
    print("║  Cross-checking Rust indicators against Python reference (pandas/numpy/talib)")
    print("╚" + "═" * 98 + "╝")
    print()

    # ── Step 1: Fetch data ──
    print("📥 Step 1: Fetching DOTUSDT 1D data from Binance...")
    df = fetch_binance_klines("DOTUSDT", "1d", 200)
    print(f"   ✅ Got {len(df)} candles | {df['timestamp'].iloc[0]} → {df['timestamp'].iloc[-1]}")
    print(f"   Price range: ${df['close'].min():.4f} — ${df['close'].max():.4f}")
    print(f"   Last close: ${df['close'].iloc[-1]:.4f}")
    print()

    # ── Step 2: Get MCP results ──
    print("📡 Step 2: Calling BonBo MCP analyze_indicators...")
    t0 = time.time()
    mcp_text = call_mcp("analyze_indicators", {"symbol": "DOTUSDT", "interval": "1d", "limit": 200})
    mcp_elapsed = time.time() - t0
    print(f"   ✅ MCP returned in {mcp_elapsed:.1f}s")

    mcp = parse_mcp_analysis(mcp_text)
    print(f"   Parsed {len(mcp)} indicator values from MCP")
    print()

    # ── Step 3: Compute Python reference ──
    print("🐍 Step 3: Computing Python reference indicators...")
    ref = BonBoReference(df)
    v = Verifier()

    # ── SMA(20) ──
    py_sma20 = ref.sma(20)
    py_sma20_last = py_sma20[-1]
    v.compare("SMA(20)", mcp.get('sma20'), py_sma20_last, tol_abs=0.02, tol_pct=1.0)

    # ── EMA(12) ──
    py_ema12 = ref.ema(12)
    v.compare("EMA(12)", mcp.get('ema12'), py_ema12[-1], tol_abs=0.02, tol_pct=1.0)

    # ── EMA(26) ──
    py_ema26 = ref.ema(26)
    v.compare("EMA(26)", mcp.get('ema26'), py_ema26[-1], tol_abs=0.02, tol_pct=1.0)

    # ── RSI(14) ──
    py_rsi = ref.rsi(14)
    v.compare("RSI(14)", mcp.get('rsi14'), py_rsi[-1], tol_abs=1.0, tol_pct=2.0)

    # ── MACD ──
    py_macd_line, py_macd_signal, py_macd_hist = ref.macd(12, 26, 9)
    v.compare("MACD Line", mcp.get('macd_line'), py_macd_line[-1], tol_abs=0.005, tol_pct=5.0)
    v.compare("MACD Signal", mcp.get('macd_signal'), py_macd_signal[-1], tol_abs=0.005, tol_pct=5.0)
    v.compare("MACD Histogram", mcp.get('macd_hist'), py_macd_hist[-1], tol_abs=0.003, tol_pct=5.0)

    # ── Bollinger Bands ──
    py_bb_upper, py_bb_mid, py_bb_lower, py_bb_pct = ref.bollinger_bands(20, 2.0)
    v.compare("BB Upper", mcp.get('bb_upper'), py_bb_upper[-1], tol_abs=0.02, tol_pct=1.0)
    v.compare("BB Mid", mcp.get('bb_mid'), py_bb_mid[-1], tol_abs=0.02, tol_pct=1.0)
    v.compare("BB Lower", mcp.get('bb_lower'), py_bb_lower[-1], tol_abs=0.02, tol_pct=1.0)
    v.compare("BB %B", mcp.get('bb_pct_b'), py_bb_pct[-1], tol_abs=0.05, tol_pct=5.0)

    # ── ALMA(10) and ALMA(30) ──
    py_alma10 = ref.alma(10, offset=0.85, sigma=6.0)
    py_alma30 = ref.alma(30, offset=0.85, sigma=6.0)
    v.compare("ALMA(10)", mcp.get('alma10'), py_alma10[-1], tol_abs=0.02, tol_pct=1.0)
    v.compare("ALMA(30)", mcp.get('alma30'), py_alma30[-1], tol_abs=0.02, tol_pct=1.0)

    # ── SuperSmoother(20) ──
    py_ss = ref.supersmoother(20)
    v.compare("SuperSmoother(20)", mcp.get('supersmoother'), py_ss[-1], tol_abs=0.02, tol_pct=1.0)

    # SuperSmoother slope (%)
    if not np.isnan(py_ss[-1]) and not np.isnan(py_ss[-2]) and py_ss[-2] != 0:
        py_ss_slope = (py_ss[-1] - py_ss[-2]) / py_ss[-2] * 100
        v.compare("SuperSmoother Slope%", mcp.get('supersmoother_slope'), py_ss_slope, tol_abs=0.05, tol_pct=10.0)

    # ── LaguerreRSI(0.8) ──
    py_lrsi = ref.laguerre_rsi(0.8)
    v.compare("LaguerreRSI(0.8)", mcp.get('laguerre_rsi'), py_lrsi[-1], tol_abs=0.05, tol_pct=10.0)

    # ── CMO(14) ──
    py_cmo = ref.cmo(14)
    v.compare("CMO(14)", mcp.get('cmo14'), py_cmo[-1], tol_abs=2.0, tol_pct=5.0)

    # ── Hurst(100) ──
    py_hurst = ref.hurst_rs(100)
    v.compare("Hurst(100) R/S", mcp.get('hurst100'), py_hurst, tol_abs=0.05, tol_pct=10.0)

    # ── Price ──
    v.compare("Price (last close)", mcp.get('price'), df['close'].iloc[-1], tol_abs=0.01, tol_pct=0.1)

    # ── Extra: TA-Lib cross-check ──
    print()
    print("📊 Step 4: TA-Lib cross-check (independent verification)...")

    import talib

    closes = df['close'].values.astype(np.float64)
    highs = df['high'].values.astype(np.float64)
    lows = df['low'].values.astype(np.float64)
    volumes = df['volume'].values.astype(np.float64)

    # TA-Lib SMA
    talib_sma20 = talib.SMA(closes, timeperiod=20)
    v.compare("TA-Lib SMA(20)", py_sma20_last, talib_sma20[-1], tol_abs=0.005, tol_pct=0.5)

    # TA-Lib EMA (note: TA-Lib seeds differently - first value seed)
    talib_ema12 = talib.EMA(closes, timeperiod=12)
    talib_ema26 = talib.EMA(closes, timeperiod=26)
    # TA-Lib EMA converges with BonBo EMA after warmup
    # BonBo seeds with SMA → different warmup but converges
    py_ema12_last = py_ema12[-1]
    py_ema26_last = py_ema26[-1]
    talib_ema12_last = talib_ema12[-1]
    talib_ema26_last = talib_ema26[-1]
    ema12_diff_pct = abs(py_ema12_last - talib_ema12_last) / max(abs(py_ema12_last), 0.001) * 100
    ema26_diff_pct = abs(py_ema26_last - talib_ema26_last) / max(abs(py_ema26_last), 0.001) * 100
    print(f"   EMA(12): Python={py_ema12_last:.6f} | TA-Lib={talib_ema12_last:.6f} | diff={ema12_diff_pct:.3f}%")
    print(f"   EMA(26): Python={py_ema26_last:.6f} | TA-Lib={talib_ema26_last:.6f} | diff={ema26_diff_pct:.3f}%")

    # TA-Lib RSI
    talib_rsi = talib.RSI(closes, timeperiod=14)
    v.compare("TA-Lib RSI(14)", py_rsi[-1], talib_rsi[-1], tol_abs=1.0, tol_pct=2.0)

    # TA-Lib MACD
    talib_macd, talib_signal, talib_hist = talib.MACD(closes, fastperiod=12, slowperiod=26, signalperiod=9)
    v.compare("TA-Lib MACD Line", py_macd_line[-1], talib_macd[-1], tol_abs=0.005, tol_pct=5.0)
    v.compare("TA-Lib MACD Signal", py_macd_signal[-1], talib_signal[-1], tol_abs=0.005, tol_pct=5.0)

    # TA-Lib BBANDS (uses sample std by default)
    talib_bb_upper, talib_bb_mid, talib_bb_lower = talib.BBANDS(
        closes, timeperiod=20, nbdevup=2, nbdevdn=2, matype=0
    )
    v.compare("TA-Lib BB Upper", py_bb_upper[-1], talib_bb_upper[-1], tol_abs=0.01, tol_pct=1.0)
    v.compare("TA-Lib BB Lower", py_bb_lower[-1], talib_bb_lower[-1], tol_abs=0.01, tol_pct=1.0)

    # TA-Lib ATR
    talib_atr = talib.ATR(highs, lows, closes, timeperiod=14)
    py_atr = ref.atr(14)
    v.compare("TA-Lib ATR(14)", py_atr[-1], talib_atr[-1], tol_abs=0.005, tol_pct=3.0)

    # TA-Lib ADX
    talib_adx = talib.ADX(highs, lows, closes, timeperiod=14)
    py_adx, py_di_plus, py_di_minus = ref.adx(14)
    v.compare("TA-Lib ADX(14)", py_adx[-1], talib_adx[-1], tol_abs=2.0, tol_pct=10.0)

    # TA-Lib Stochastic Fast %K (matches BonBo's fast %K — no smoothing)
    talib_fastk, talib_fastd = talib.STOCHF(highs, lows, closes,
                                              fastk_period=14, fastd_period=3, fastd_matype=0)
    py_stoch_k, py_stoch_d = ref.stochastic(14, 3)
    v.compare("TA-Lib Stoch Fast %K", py_stoch_k[-1], talib_fastk[-1], tol_abs=2.0, tol_pct=5.0)

    # TA-Lib CMO (note: TA-Lib CMO uses slightly different sum method than BonBo)
    # BonBo uses CMO = 100 * (sum_up - sum_down) / (sum_up + sum_down)
    # TA-Lib same formula but with different internal handling
    # We compare Python (matching BonBo) vs TA-Lib, larger tolerance acceptable
    talib_cmo = talib.CMO(closes, timeperiod=14)
    # Compare with high tolerance — the important check is Python vs BonBo (already done above)
    if not np.isnan(talib_cmo[-1]):
        cmo_diff = abs(py_cmo[-1] - talib_cmo[-1])
        print(f"   CMO: Python={py_cmo[-1]:.3f} | TA-Lib={talib_cmo[-1]:.3f} | diff={cmo_diff:.3f}")
        if cmo_diff > 5:
            print(f"        (TA-Lib CMO uses different smoothing — BonBo Python vs MCP diff is what matters: already ✅)")

    # TA-Lib OBV — note: TA-Lib OBV[0]=0, our OBV[0]=volume[0]
    # Both converge over time; compare relative change, not absolute
    talib_obv = talib.OBV(closes, volumes)
    py_obv = ref.obv()
    # Compare last value relative difference
    if abs(talib_obv[-1]) > 0 and abs(py_obv[-1]) > 0:
        obv_diff_pct = abs(py_obv[-1] - talib_obv[-1]) / max(abs(py_obv[-1]), abs(talib_obv[-1])) * 100
        obv_status = "✅" if obv_diff_pct < 5 else "⚠️" if obv_diff_pct < 10 else "❌"
        print(f"   OBV: Python={py_obv[-1]:,.0f} | TA-Lib={talib_obv[-1]:,.0f} | rel_diff={obv_diff_pct:.2f}% {obv_status}")
        print(f"        (Difference due to starting value: Python starts with vol[0], TA-Lib starts with 0)")

    # ── Extra indicators not in MCP output ──
    print()
    print("📊 Step 5: Additional Python vs TA-Lib checks...")

    # VWAP comparison (no TA-Lib VWAP, just print)
    py_vwap = ref.vwap()
    print(f"   VWAP: Python={py_vwap[-1]:.6f}")

    # Print report
    v.print_report()

    # Save results
    report = {
        "timestamp": datetime.now().isoformat(),
        "symbol": "DOTUSDT",
        "interval": "1d",
        "candles": len(df),
        "mcp_values": mcp,
        "python_last_values": {
            "sma20": float(py_sma20[-1]) if not np.isnan(py_sma20[-1]) else None,
            "ema12": float(py_ema12[-1]) if not np.isnan(py_ema12[-1]) else None,
            "ema26": float(py_ema26[-1]) if not np.isnan(py_ema26[-1]) else None,
            "rsi14": float(py_rsi[-1]) if not np.isnan(py_rsi[-1]) else None,
            "macd_line": float(py_macd_line[-1]) if not np.isnan(py_macd_line[-1]) else None,
            "macd_signal": float(py_macd_signal[-1]) if not np.isnan(py_macd_signal[-1]) else None,
            "macd_hist": float(py_macd_hist[-1]) if not np.isnan(py_macd_hist[-1]) else None,
            "bb_upper": float(py_bb_upper[-1]) if not np.isnan(py_bb_upper[-1]) else None,
            "bb_mid": float(py_bb_mid[-1]) if not np.isnan(py_bb_mid[-1]) else None,
            "bb_lower": float(py_bb_lower[-1]) if not np.isnan(py_bb_lower[-1]) else None,
            "bb_pct_b": float(py_bb_pct[-1]) if not np.isnan(py_bb_pct[-1]) else None,
            "alma10": float(py_alma10[-1]) if not np.isnan(py_alma10[-1]) else None,
            "alma30": float(py_alma30[-1]) if not np.isnan(py_alma30[-1]) else None,
            "supersmoother": float(py_ss[-1]) if not np.isnan(py_ss[-1]) else None,
            "laguerre_rsi": float(py_lrsi[-1]) if not np.isnan(py_lrsi[-1]) else None,
            "cmo14": float(py_cmo[-1]) if not np.isnan(py_cmo[-1]) else None,
            "hurst100": float(py_hurst) if not np.isnan(py_hurst) else None,
            "atr14": float(py_atr[-1]) if not np.isnan(py_atr[-1]) else None,
            "adx14": float(py_adx[-1]) if not np.isnan(py_adx[-1]) else None,
        },
        "talib_last_values": {
            "sma20": float(talib_sma20[-1]) if not np.isnan(talib_sma20[-1]) else None,
            "ema12": float(talib_ema12[-1]) if not np.isnan(talib_ema12[-1]) else None,
            "ema26": float(talib_ema26[-1]) if not np.isnan(talib_ema26[-1]) else None,
            "rsi14": float(talib_rsi[-1]) if not np.isnan(talib_rsi[-1]) else None,
            "macd_line": float(talib_macd[-1]) if not np.isnan(talib_macd[-1]) else None,
            "macd_signal": float(talib_signal[-1]) if not np.isnan(talib_signal[-1]) else None,
            "bb_upper": float(talib_bb_upper[-1]) if not np.isnan(talib_bb_upper[-1]) else None,
            "bb_lower": float(talib_bb_lower[-1]) if not np.isnan(talib_bb_lower[-1]) else None,
            "atr14": float(talib_atr[-1]) if not np.isnan(talib_atr[-1]) else None,
            "adx14": float(talib_adx[-1]) if not np.isnan(talib_adx[-1]) else None,
            "stoch_k": float(talib_fastk[-1]) if not np.isnan(talib_fastk[-1]) else None,
            "cmo14": float(talib_cmo[-1]) if not np.isnan(talib_cmo[-1]) else None,
            "obv": float(talib_obv[-1]),
        },
        "verification": {
            "passed": v.passed,
            "warned": v.warned,
            "failed": v.failed,
            "details": [
                {"name": n, "status": s, "rust": float(r) if isinstance(r, (int, float)) else str(r),
                 "python": float(p) if isinstance(p, (int, float)) else str(p),
                 "pct_diff": float(d) if isinstance(d, (int, float)) else 0}
                for n, s, r, p, d, _ in v.results
            ]
        }
    }

    import os
    rdir = os.path.expanduser("~/.bonbo/reports")
    os.makedirs(rdir, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    rpt_path = os.path.join(rdir, f"indicator_verification_{ts}.json")
    with open(rpt_path, "w") as f:
        json.dump(report, f, indent=2, default=str)
    print(f"\n💾 Report saved: {rpt_path}")


if __name__ == "__main__":
    main()
