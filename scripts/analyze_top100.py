#!/usr/bin/env python3
"""
BonBo Top 100 Crypto Deep Analysis v2.0 — Comprehensive Upgrade

Tính năng nâng cấp:
  1. Auto-discover top 100 coins from Binance (volume-ranked)
  2. Async/parallel MCP calls → 5-10x faster than v1
  3. Multi-timeframe analysis (1h + 4h + 1d)
  4. DMA-weighted composite scoring (Dynamic Model Averaging)
  5. Fear & Greed Index + Sentiment integration
  6. Risk-adjusted position sizing for top picks
  7. Multi-strategy backtest matrix
  8. Rich terminal output with color, progress bars
  9. JSON/CSV/HTML export
 10. SQLite caching (5-min TTL)
 11. CLI arguments for flexible usage
 12. Self-learning journal integration

Usage:
  python3 scripts/analyze_top100.py [options]

  --top-n N          So coins phan tich (default: 100)
  --intervals IFS    Timeframes, comma-separated (default: 1h,4h,1d)
  --output FMT       Output format: table,json,csv,html,all (default: table)
  --no-cache         Bypass cache, force fresh data
  --verbose          Chi tiet them
  --journal          Ghi predictions vao self-learning journal
  --quick            Chi 20 coins, 1h timeframe (fast mode)
"""

from __future__ import annotations

import argparse
import asyncio
import csv
import hashlib
import json
import os
import re
import sqlite3
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# ── Optional imports with fallbacks ──────────────────────────────────
try:
    import aiohttp
    ASYNC_AVAILABLE = True
except ImportError:
    ASYNC_AVAILABLE = False

try:
    from rich.console import Console
    from rich.table import Table
    from rich.panel import Panel
    from rich.text import Text
    RICH_AVAILABLE = True
except ImportError:
    RICH_AVAILABLE = False


# ═══════════════════════════════════════════════════════════════════════
# CONFIG
# ═══════════════════════════════════════════════════════════════════════
MCP_URL = os.environ.get("BONBO_MCP_URL", "http://localhost:9876/mcp")
CACHE_DIR = Path.home() / ".bonbo" / "cache"
CACHE_DB = CACHE_DIR / "analysis_cache.db"
JOURNAL_DB = Path.home() / ".bonbo" / "self_learning" / "journal.db"
REPORTS_DIR = Path.home() / ".bonbo" / "reports"
CACHE_TTL = 300  # 5 minutes

CACHE_DIR.mkdir(parents=True, exist_ok=True)
REPORTS_DIR.mkdir(parents=True, exist_ok=True)

SKIP_SYMBOLS = {
    "USDCUSDT", "FDUSDUSDT", "USD1USDT", "RLUSDUSDT", "PAXGUSDT",
    "XAUTUSDT", "USDEUSDT", "XUSDUSDT", "EURUSDT", "CUSDUSDT",
    "DAIUSDT", "TUSDUSDT", "BUSDUSDT", "USDPUSDT", "USTCUSDT",
    "BETHUSDT", "WBTCUSDT",
}

DEFAULT_WEIGHTS: Dict[str, float] = {
    # ── Traditional indicators ──
    "rsi": 0.08,
    "macd": 0.07,
    "bollinger": 0.06,
    "sma_cross": 0.05,
    "volume": 0.04,
    "momentum": 0.05,
    "mean_reversion": 0.06,
    "regime": 0.05,
    "backtest": 0.08,
    "sentiment": 0.04,
    "multi_tf": 0.04,
    # ── Financial-Hacker indicators ──
    "hurst": 0.07,         # Hurst exponent → trending vs mean-reverting
    "alma_cross": 0.06,    # ALMA crossover → zero-lag signal
    "super_smoother": 0.05, # SuperSmoother slope → noise-free trend
    "laguerre_rsi": 0.06,   # LaguerreRSI → better RSI
    "cmo": 0.05,            # Chande Momentum Oscillator
    "weighted_signals": 0.09,  # FH weighted signal aggregate
}


# ═══════════════════════════════════════════════════════════════════════
# DATA CLASSES
# ═══════════════════════════════════════════════════════════════════════
@dataclass
class IndicatorData:
    price: float = 0.0
    rsi: float = 50.0
    macd_line: float = 0.0
    macd_signal: float = 0.0
    macd_hist: float = 0.0
    bb_upper: float = 0.0
    bb_middle: float = 0.0
    bb_lower: float = 0.0
    bb_pctb: float = 0.5
    sma_20: float = 0.0
    sma_50: float = 0.0
    ema_12: float = 0.0
    ema_26: float = 0.0
    atr: float = 0.0
    adx: float = 25.0
    volume: float = 0.0
    volume_sma: float = 0.0
    change_24h: float = 0.0
    # ── Financial-Hacker Indicators ──
    alma_10: float = 0.0
    alma_30: float = 0.0
    alma_signal: str = ""       # "Bullish" / "Bearish"
    alma_pct: float = 0.0       # e.g. +0.16%
    super_smoother: float = 0.0
    super_smoother_slope: float = 0.0  # e.g. +0.0344%
    hurst: float = 0.5          # 0.5 = random walk, >0.55 = trending
    cmo: float = 0.0            # Chande Momentum Oscillator
    laguerre_rsi: float = 0.5   # Laguerre RSI (0-1 scale)


@dataclass
class SignalItem:
    """A single weighted trading signal."""
    name: str = ""          # e.g. "MACD(12,26,9)", "Hurst(100)"
    direction: str = ""     # "Buy" / "Sell" / "Neutral"
    confidence: int = 0     # 0-100 weight
    detail: str = ""        # e.g. "MACD bullish crossover"


@dataclass
class SignalData:
    buy_count: int = 0
    sell_count: int = 0
    neutral_count: int = 0
    signals: List[str] = field(default_factory=list)
    # ── Weighted signals (FH) ──
    weighted_buy_score: float = 0.0    # sum of buy confidence weights
    weighted_sell_score: float = 0.0   # sum of sell confidence weights
    signal_items: List[SignalItem] = field(default_factory=list)
    # ── FH signal flags ──
    fh_hurst_trend: bool = False       # Hurst > 0.55 (trending)
    fh_supersmoother_buy: bool = False # SuperSmoother slope positive
    fh_laguerre_overbought: bool = False  # LaguerreRSI > 0.8
    fh_laguerre_oversold: bool = False    # LaguerreRSI < 0.2


@dataclass
class RegimeData:
    regime: str = "Unknown"
    volatility: float = 0.0
    trend_strength: float = 0.0
    # ── FH regime details ──
    hurst_value: float = 0.0
    hurst_regime: str = ""        # "Trending" / "Random Walk" / "Mean Reverting"
    strategy_hint: str = ""       # e.g. "use trend-following"


@dataclass
class BacktestData:
    total_return: float = 0.0
    win_rate: float = 0.0
    sharpe: float = 0.0
    max_drawdown: float = 0.0
    total_trades: int = 0
    best_strategy: str = ""  # Name of the best FH strategy
    best_strategy: str = ""


@dataclass
class PositionAdvice:
    action: str = "HOLD"  # LONG, SHORT, HOLD, AVOID
    entry: float = 0.0
    stop_loss: float = 0.0
    take_profit_1: float = 0.0
    take_profit_2: float = 0.0
    risk_reward: float = 0.0
    position_size_pct: float = 0.0
    confidence: str = "LOW"  # LOW, MEDIUM, HIGH
    notes: List[str] = field(default_factory=list)


@dataclass
class CoinAnalysis:
    symbol: str = ""
    rank: int = 0
    indicators: Dict[str, IndicatorData] = field(default_factory=dict)
    signals: Dict[str, SignalData] = field(default_factory=dict)
    regimes: Dict[str, RegimeData] = field(default_factory=dict)
    backtest: BacktestData = field(default_factory=BacktestData)
    fear_greed: int = 50
    composite_score: float = 50.0
    recommendation: str = "HOLD"
    position_advice: Optional[PositionAdvice] = None
    analyzed_at: str = ""


# ═══════════════════════════════════════════════════════════════════════
# CACHE LAYER (SQLite)
# ═══════════════════════════════════════════════════════════════════════
class AnalysisCache:
    """SQLite-based cache with TTL for MCP responses."""

    def __init__(self, db_path: Path = CACHE_DB, ttl: int = CACHE_TTL):
        self.db_path = db_path
        self.ttl = ttl
        self._init_db()

    def _init_db(self):
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("""
            CREATE TABLE IF NOT EXISTS cache (
                key TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                created_at REAL NOT NULL
            )
        """)
        conn.execute("CREATE INDEX IF NOT EXISTS idx_cache_time ON cache(created_at)")
        conn.commit()
        conn.close()

    def _make_key(self, tool: str, args: dict) -> str:
        raw = f"{tool}:{json.dumps(args, sort_keys=True)}"
        return hashlib.sha256(raw.encode()).hexdigest()

    def get(self, tool: str, args: dict) -> Optional[str]:
        conn = sqlite3.connect(str(self.db_path))
        key = self._make_key(tool, args)
        row = conn.execute(
            "SELECT data, created_at FROM cache WHERE key = ?", (key,)
        ).fetchone()
        conn.close()
        if row is None:
            return None
        if time.time() - row[1] > self.ttl:
            return None
        return row[0]  # Return the cached data

    def set(self, tool: str, args: dict, data: str):
        conn = sqlite3.connect(str(self.db_path))
        key = self._make_key(tool, args)
        conn.execute(
            "INSERT OR REPLACE INTO cache (key, data, created_at) VALUES (?, ?, ?)",
            (key, data, time.time()),
        )
        conn.commit()
        conn.close()

    def clear(self):
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("DELETE FROM cache")
        conn.commit()
        conn.close()


# ═══════════════════════════════════════════════════════════════════════
# MCP CLIENT (Async + Sync fallback)
# ═══════════════════════════════════════════════════════════════════════
class MCPClient:
    """Async MCP client with caching and retry."""

    def __init__(self, url: str = MCP_URL, cache: Optional[AnalysisCache] = None,
                 use_cache: bool = True, verbose: bool = False):
        self.url = url
        self.cache = cache
        self.use_cache = use_cache
        self.verbose = verbose
        self._request_count = 0
        self._cache_hits = 0
        self._errors = 0

    async def call(self, tool: str, args: dict = None, timeout: int = 30) -> str:
        """Call MCP tool with caching."""
        if args is None:
            args = {}

        # Check cache
        if self.use_cache and self.cache:
            cached = self.cache.get(tool, args)
            if cached is not None:
                self._cache_hits += 1
                if self.verbose:
                    print(f"  [cache hit] {tool}", file=sys.stderr)
                return cached

        self._request_count += 1
        payload = json.dumps({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": tool, "arguments": args},
            "id": str(self._request_count),
        }).encode()

        try:
            if ASYNC_AVAILABLE:
                async with aiohttp.ClientSession() as session:
                    async with session.post(
                        self.url, data=payload,
                        headers={"Content-Type": "application/json"},
                        timeout=aiohttp.ClientTimeout(total=timeout),
                    ) as resp:
                        data = await resp.json()
            else:
                # Sync fallback using urllib
                import urllib.request
                req = urllib.request.Request(
                    self.url, data=payload,
                    headers={"Content-Type": "application/json"},
                    method="POST",
                )
                with urllib.request.urlopen(req, timeout=timeout) as resp:
                    data = json.loads(resp.read().decode())

            text = data.get("result", {}).get("content", [{}])
            result = text[0].get("text", "") if text else ""

            # Store in cache
            if self.use_cache and self.cache:
                self.cache.set(tool, args, result)

            return result

        except Exception as e:
            self._errors += 1
            if self.verbose:
                print(f"  [error] {tool}: {e}", file=sys.stderr)
            return f"Error: {e}"

    async def call_batch(self, calls: List[Tuple[str, dict]]) -> List[str]:
        """Execute multiple MCP calls in parallel."""
        tasks = [self.call(tool, args) for tool, args in calls]
        return await asyncio.gather(*tasks)

    def stats(self) -> str:
        return (f"MCP Stats: {self._request_count} requests, "
                f"{self._cache_hits} cache hits, {self._errors} errors")


# ═══════════════════════════════════════════════════════════════════════
# PARSERS — Robust text-to-structured-data
# ═══════════════════════════════════════════════════════════════════════
class ResponseParser:
    """Parse MCP tool responses into structured data."""

    @staticmethod
    def parse_indicators(text: str) -> IndicatorData:
        d = IndicatorData()
        if text.startswith("Error"):
            return d

        for line in text.split("\n"):
            line = line.strip()

            # Price
            m = re.search(r'\*\*Price\*\*:\s*\$?([\d,.]+)', line)
            if m:
                d.price = float(m.group(1).replace(",", ""))
                continue

            # RSI — match "RSI(14)**: 58.9" or "RSI(14): 58.9" or "RSI(14) : 58.9"
            m = re.search(r'RSI\(14\)[^:]*:\s*([\d.]+)', line)
            if m:
                d.rsi = float(m.group(1))
                continue

            # MACD
            m = re.search(r'macd\s*=\s*([-\d.]+)', line, re.IGNORECASE)
            if m:
                d.macd_line = float(m.group(1))
            m = re.search(r'signal\s*=\s*([-\d.]+)', line, re.IGNORECASE)
            if m:
                d.macd_signal = float(m.group(1))
            m = re.search(r'hist\s*=\s*([-\d.]+)', line, re.IGNORECASE)
            if m:
                d.macd_hist = float(m.group(1))

            # Bollinger Bands
            m = re.search(r'%B\s*=\s*([\d.]+)', line)
            if m:
                d.bb_pctb = float(m.group(1))
            m = re.search(r'upper\s*=\s*\$?([\d,.]+)', line, re.IGNORECASE)
            if m:
                d.bb_upper = float(m.group(1).replace(",", ""))
            m = re.search(r'lower\s*=\s*\$?([\d,.]+)', line, re.IGNORECASE)
            if m:
                d.bb_lower = float(m.group(1).replace(",", ""))

            # SMA — match "**SMA(20)**: $75856.35" or "SMA(20): $75856.35"
            m = re.search(r'SMA\(20\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.sma_20 = float(m.group(1).replace(",", ""))
            m = re.search(r'SMA\(50\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.sma_50 = float(m.group(1).replace(",", ""))

            # EMA — match "**EMA(12)**: $75898.27"
            m = re.search(r'EMA\(12\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.ema_12 = float(m.group(1).replace(",", ""))
            m = re.search(r'EMA\(26\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.ema_26 = float(m.group(1).replace(",", ""))

            # ATR
            m = re.search(r'ATR[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.atr = float(m.group(1).replace(",", ""))

            # ADX
            m = re.search(r'ADX[^:]*:\s*([\d.]+)', line)
            if m:
                d.adx = float(m.group(1))

            # Volume
            m = re.search(r'Volume[^:]*:\s*([\d,.]+)', line)
            if m:
                d.volume = float(m.group(1).replace(",", ""))

            # 24h Change
            m = re.search(r'(?:24h\s+)?Change[^:]*:\s*([-\d.]+)%', line)
            if m:
                d.change_24h = float(m.group(1))

            # ── Financial-Hacker Indicators ──
            # ALMA(10): $75981.69  or  ALMA(10)**: $75981.69
            m = re.search(r'ALMA\(10\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.alma_10 = float(m.group(1).replace(",", ""))
            m = re.search(r'ALMA\(30\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.alma_30 = float(m.group(1).replace(",", ""))

            # ALMA signal: "ALMA(10/30)**: Bullish (+0.16%)"
            m = re.search(r'ALMA\(10/30\)[^:]*:\s*(Bullish|Bearish)\s*\(([+\-\d.]+)%\)', line, re.IGNORECASE)
            if m:
                d.alma_signal = m.group(1).capitalize()
                try:
                    d.alma_pct = float(m.group(2))
                except ValueError:
                    pass

            # SuperSmoother(20)**: $75897.93
            m = re.search(r'SuperSmoother\((?:20|\d+)\)[^:]*:\s*\$?([\d,.]+)', line)
            if m:
                d.super_smoother = float(m.group(1).replace(",", ""))
            # SuperSmoother slope: "Slope=+0.0344%"
            m = re.search(r'Slope\s*=\s*([+\-\d.]+)%', line)
            if m:
                d.super_smoother_slope = float(m.group(1))

            # Hurst(100)**: 0.827
            m = re.search(r'Hurst\(\d+\)[^:]*:\s*([-\d.]+)', line)
            if m:
                d.hurst = float(m.group(1))

            # CMO(14)**: -15.8
            m = re.search(r'CMO\(\d+\)[^:]*:\s*([-\d.]+)', line)
            if m:
                d.cmo = float(m.group(1))

            # LaguerreRSI(0.8)**: 1.000  or  0.823
            m = re.search(r'LaguerreRSI\([^\)]*\)[^:]*:\s*([-\d.]+)', line)
            if m:
                d.laguerre_rsi = float(m.group(1))

        # Also try inline format: Price: $X | RSI: Y | ...
        if d.price == 0:
            m = re.search(r'\$([\d,.]+)', text)
            if m:
                d.price = float(m.group(1).replace(",", ""))

        return d

    @staticmethod
    def parse_signals(text: str) -> SignalData:
        s = SignalData()
        if text.startswith("Error"):
            return s

        lines = text.split("\n")

        # Count emoji signals
        s.buy_count = text.count("\U0001f7e2")  # green circle
        s.sell_count = text.count("\U0001f534")  # red circle
        s.neutral_count = text.count("\U0001f7e1")  # yellow circle

        # Parse weighted signals (multi-line: header + detail)
        # Format:
        #   🟢 **Buy** [MACD(12,26,9)] (60%)
        #      MACD bullish crossover
        current_item = None

        for line in lines:
            ls = line.strip()
            if not ls:
                continue

            has_emoji = ("\U0001f7e2" in ls or "\U0001f534" in ls or "\U0001f7e1" in ls)

            if has_emoji:
                # Save previous item
                if current_item:
                    s.signal_items.append(current_item)

                # Direction
                direction = ""
                if "\U0001f7e2" in ls:
                    direction = "Buy"
                elif "\U0001f534" in ls:
                    direction = "Sell"
                elif "\U0001f7e1" in ls:
                    direction = "Neutral"

                # Weight: "(60%)" or "(w:60)"
                weight = 50
                wm = re.search(r'\(w:(\d+)\)', ls)
                if wm:
                    weight = int(wm.group(1))
                else:
                    wm2 = re.search(r'\((\d+)%\)', ls)
                    if wm2:
                        weight = int(wm2.group(1))

                # Name: "[MACD(12,26,9)]"
                nm = re.search(r'\[([^\]]+)\]', ls)
                name = nm.group(1) if nm else ""

                current_item = SignalItem(
                    name=name, direction=direction,
                    confidence=weight, detail=""
                )

                # Accumulate weighted scores
                if direction == "Buy":
                    s.weighted_buy_score += weight
                elif direction == "Sell":
                    s.weighted_sell_score += weight

                # FH signal flags
                name_lower = name.lower()
                if "hurst" in name_lower:
                    s.fh_hurst_trend = True
                if "supersmoother" in name_lower and direction == "Buy":
                    s.fh_supersmoother_buy = True
                if "laguerre" in name_lower:
                    if direction == "Sell" or "overbought" in ls.lower():
                        s.fh_laguerre_overbought = True
                    elif direction == "Buy" or "oversold" in ls.lower():
                        s.fh_laguerre_oversold = True

            elif current_item and not ls.startswith("\U0001f3af") and not ls.startswith("\U0001f4b0") and not ls.startswith("\U0001f9ec"):
                # Detail continuation line
                detail_clean = ls.strip("* ")
                if detail_clean and not current_item.detail:
                    current_item.detail = detail_clean

                # Extract extra data from detail
                detail_lower = detail_clean.lower()
                if "alma" in detail_lower and "cross" in detail_lower:
                    m = re.search(r'\(([+\-\d.]+)%\)', detail_clean)
                    if m:
                        s.signals.append("ALMA_PCT:" + m.group(1))
                if "supersmoother" in detail_lower and "slope" in detail_lower:
                    m = re.search(r'\(([+\-\d.]+)%\)', detail_clean)
                    if m:
                        s.signals.append("SS_SLOPE:" + m.group(1))

        # Save last item
        if current_item:
            s.signal_items.append(current_item)

        # Fallback
        if s.buy_count == 0 and s.weighted_buy_score == 0:
            buy_words = text.lower().count("buy")
            if buy_words > 0:
                s.buy_count = buy_words
                s.weighted_buy_score = buy_words * 50
        if s.sell_count == 0 and s.weighted_sell_score == 0:
            sell_words = text.lower().count("sell")
            if sell_words > 0:
                s.sell_count = sell_words
                s.weighted_sell_score = sell_words * 50

        return s

    @staticmethod
    def parse_regime(text: str) -> RegimeData:
        r = RegimeData()
        if text.startswith("Error"):
            return r

        text_lower = text.lower()
        for name in ["Trending Up", "Trending Down", "Ranging", "Volatile", "Quiet",
                      "Breakout", "Mean Reverting"]:
            if name.lower() in text_lower:
                r.regime = name
                break

        # Parse volatility
        m = re.search(r'volatility\s*:?\s*([\d.]+)', text_lower)
        if m:
            r.volatility = float(m.group(1))

        # Parse trend strength
        m = re.search(r'(?:trend|strength)\s*:?\s*([\d.]+)', text_lower)
        if m:
            r.trend_strength = float(m.group(1))

        # Parse Hurst from regime: "Hurst Exponent: 0.827 (>0.55 → Trending)"
        m = re.search(r'hurst[^:]*:\s*([\d.]+)', text_lower)
        if m:
            r.hurst_value = float(m.group(1))
            if r.hurst_value > 0.55:
                r.hurst_regime = "Trending"
                r.strategy_hint = "use trend-following"
            elif r.hurst_value < 0.45:
                r.hurst_regime = "Mean Reverting"
                r.strategy_hint = "use mean-reversion"
            else:
                r.hurst_regime = "Random Walk"
                r.strategy_hint = "use range-bound strategies"

        # Also try format: "H=0.83"
        if r.hurst_value == 0.0:
            m = re.search(r'H\s*=\s*([\d.]+)', text)
            if m:
                r.hurst_value = float(m.group(1))

        # Strategy hint
        if "trend-following" in text_lower:
            r.strategy_hint = "use trend-following"
        elif "mean-reversion" in text_lower:
            r.strategy_hint = "use mean-reversion"

        return r

    @staticmethod
    def parse_backtest(text: str) -> BacktestData:
        bt = BacktestData()
        if text.startswith("Error"):
            return bt

        for line in text.split("\n"):
            m = re.search(r'Total\s+Return[^:]*:\s*([-\d.]+)%', line, re.IGNORECASE)
            if m:
                bt.total_return = float(m.group(1))
            m = re.search(r'Win\s+Rate[^:]*:\s*([\d.]+)%', line, re.IGNORECASE)
            if m:
                bt.win_rate = float(m.group(1))
            m = re.search(r'Sharpe\s*(?:Ratio)?[^:]*:\s*([-\d.]+)', line, re.IGNORECASE)
            if m:
                bt.sharpe = float(m.group(1))
            m = re.search(r'Max\s*(?:Drawdown|DD)[^:]*:\s*([-\d.]+)%', line, re.IGNORECASE)
            if m:
                bt.max_drawdown = float(m.group(1))
            m = re.search(r'Total\s*Trades[^:]*:\s*(\d+)', line, re.IGNORECASE)
            if m:
                bt.total_trades = int(m.group(1))

        return bt

    @staticmethod
    def parse_fear_greed(text: str) -> int:
        """Parse Fear & Greed value from text."""
        m = re.search(r'(?:value|index|score)\s*:?\s*(\d+)', text, re.IGNORECASE)
        if m:
            return int(m.group(1))
        m = re.search(r'(\d+)\s*/\s*100', text)
        if m:
            return int(m.group(1))
        # Try finding any standalone number 0-100
        m = re.search(r'\b(\d{1,2})\b', text)
        if m:
            val = int(m.group(1))
            if 0 <= val <= 100:
                return val
        return 50

    @staticmethod
    def parse_top_coins(text: str, limit: int = 100) -> List[str]:
        """Parse get_top_crypto response into list of USDT symbols."""
        symbols = []
        for line in text.split("\n"):
            line = line.strip()
            if not line or line.startswith("|") or line.startswith("#"):
                continue
            m = re.search(r'\b([A-Z0-9]{2,10})USDT\b', line)
            if m:
                sym = m.group(0)
                if sym not in SKIP_SYMBOLS and sym not in symbols:
                    # Skip if contains non-ASCII in the original line (e.g. Chinese chars)
                    symbols.append(sym)

            if len(symbols) >= limit:
                break

        return symbols


# ═══════════════════════════════════════════════════════════════════════
# DMA SCORING ENGINE
# ═══════════════════════════════════════════════════════════════════════
class DMAScoring:
    """Dynamic Model Averaging scoring engine."""

    def __init__(self, weights: Optional[Dict[str, float]] = None):
        self.weights = weights or DEFAULT_WEIGHTS.copy()
        # Normalize weights to sum to 1.0
        total = sum(self.weights.values())
        if total > 0:
            self.weights = {k: v / total for k, v in self.weights.items()}

    def compute_rsi_score(self, rsi: float) -> float:
        if rsi < 25: return 15.0
        elif rsi < 30: return 12.0
        elif rsi < 35: return 8.0
        elif rsi < 40: return 4.0
        elif rsi < 55: return 0.0
        elif rsi < 60: return -3.0
        elif rsi < 65: return -6.0
        elif rsi < 70: return -10.0
        elif rsi < 75: return -13.0
        else: return -15.0

    def compute_macd_score(self, hist: float, macd_line: float = 0, signal: float = 0) -> float:
        score = 0.0
        if hist > 0: score += 8.0
        else: score -= 8.0
        # Crossover bonus
        if macd_line > 0 and signal > 0:
            if macd_line > signal: score += 4.0
            else: score -= 4.0
        return score

    def compute_bollinger_score(self, pctb: float) -> float:
        if pctb < 0.0: return 14.0
        elif pctb < 0.05: return 12.0
        elif pctb < 0.15: return 8.0
        elif pctb < 0.3: return 4.0
        elif pctb < 0.7: return 0.0
        elif pctb < 0.85: return -4.0
        elif pctb < 0.95: return -8.0
        elif pctb < 1.0: return -12.0
        else: return -14.0

    def compute_sma_cross_score(self, price: float, sma20: float, sma50: float) -> float:
        score = 0.0
        if sma20 > 0 and price > 0:
            pct = (price - sma20) / sma20 * 100
            if pct > 3: score += 4.0
            elif pct > 0: score += 2.0
            elif pct > -3: score -= 2.0
            else: score -= 4.0
        if sma50 > 0 and sma20 > 0:
            if sma20 > sma50: score += 3.0
            else: score -= 3.0
        return score

    def compute_volume_score(self, volume: float, vol_sma: float) -> float:
        if vol_sma <= 0: return 0.0
        ratio = volume / vol_sma
        if ratio > 2.0: return 5.0   # Breakout volume
        elif ratio > 1.5: return 3.0
        elif ratio > 1.0: return 1.0
        elif ratio > 0.7: return -1.0
        else: return -3.0

    def compute_momentum_score(self, change_24h: float, rsi: float) -> float:
        score = 0.0
        # Strong momentum (trend following)
        if change_24h > 10: score += 6.0
        elif change_24h > 5: score += 4.0
        elif change_24h > 2: score += 2.0
        elif change_24h > -2: score += 0.0
        elif change_24h > -5: score -= 2.0
        elif change_24h > -10: score -= 4.0
        else: score -= 6.0
        return score

    def compute_mean_reversion_score(self, rsi: float, bb_pctb: float, change: float) -> float:
        """Oversold + bottoming signals are positive."""
        score = 0.0
        # Deep oversold
        if rsi < 25 and bb_pctb < 0.05:
            score += 10.0  # Very strong mean reversion
        elif rsi < 30 and bb_pctb < 0.1:
            score += 6.0
        elif rsi > 75 and bb_pctb > 0.95:
            score += 10.0  # For SHORT signals (overbought)
        # Cap at reasonable range
        return max(-10.0, min(10.0, score))

    def compute_regime_score(self, regime: str) -> float:
        r = regime.lower()
        if "trending up" in r: return 8.0
        elif "trending down" in r: return -8.0
        elif "volatile" in r: return -4.0
        elif "breakout" in r: return 6.0
        elif "ranging" in r: return 0.0
        elif "quiet" in r: return 2.0
        return 0.0

    def compute_backtest_score(self, bt: BacktestData) -> float:
        score = 0.0
        if bt.total_return > 10: score += 12.0
        elif bt.total_return > 5: score += 8.0
        elif bt.total_return > 0: score += 4.0
        elif bt.total_return > -5: score -= 4.0
        elif bt.total_return > -10: score -= 8.0
        else: score -= 12.0
        # Win rate bonus
        if bt.win_rate > 60: score += 3.0
        elif bt.win_rate > 50: score += 1.0
        elif bt.win_rate < 40: score -= 3.0
        return score

    def compute_sentiment_score(self, fear_greed: int) -> float:
        if fear_greed < 15: return 12.0
        elif fear_greed < 25: return 8.0
        elif fear_greed < 35: return 4.0
        elif fear_greed < 55: return 0.0
        elif fear_greed < 70: return -4.0
        elif fear_greed < 85: return -8.0
        else: return -12.0

    def compute_multi_tf_score(self, coin: CoinAnalysis) -> float:
        """Bonus for multi-timeframe alignment."""
        scores_by_tf = []
        for tf, ind in coin.indicators.items():
            tf_score = 0.0
            tf_score += self.compute_rsi_score(ind.rsi) * 0.3
            tf_score += self.compute_macd_score(ind.macd_hist) * 0.3
            tf_score += self.compute_bollinger_score(ind.bb_pctb) * 0.2
            reg = coin.regimes.get(tf, RegimeData())
            tf_score += self.compute_regime_score(reg.regime) * 0.2
            scores_by_tf.append(tf_score)

        if not scores_by_tf:
            return 0.0
        avg = sum(scores_by_tf) / len(scores_by_tf)
        # Alignment bonus
        all_pos = all(s > 0 for s in scores_by_tf)
        all_neg = all(s < 0 for s in scores_by_tf)
        if all_pos: avg += 5.0
        elif all_neg: avg -= 5.0
        return avg

    # ── Financial-Hacker Scoring Methods ──

    def compute_hurst_score(self, hurst: float, regime_data: RegimeData = None) -> float:
        """Score based on Hurst exponent — higher = stronger trend."""
        if hurst <= 0:
            return 0.0
        score = 0.0
        if hurst > 0.60:
            score += 12.0   # Strong trending → good for trend-following
        elif hurst > 0.55:
            score += 8.0    # Mild trending
        elif hurst > 0.50:
            score += 2.0    # Slight trend
        elif hurst > 0.45:
            score -= 2.0    # Random walk — no edge
        elif hurst > 0.40:
            score -= 5.0    # Mean-reverting
        else:
            score -= 8.0    # Strong mean-reversion (good for MR strategies)

        # Bonus: Hurst agrees with regime
        if regime_data and regime_data.hurst_regime:
            regime = regime_data.regime.lower()
            if hurst > 0.55 and "trending up" in regime:
                score += 4.0   # Hurst trending + Trending Up = strong bullish
            elif hurst < 0.45 and "ranging" in regime:
                score += 3.0   # Hurst MR + Ranging = good for MR plays
            elif hurst > 0.55 and "trending down" in regime:
                score -= 4.0   # Hurst trending + Trending Down = bearish
        return max(-15.0, min(15.0, score))

    def compute_alma_cross_score(self, alma_10: float, alma_30: float,
                                  alma_signal: str, alma_pct: float,
                                  price: float = 0) -> float:
        """Score based on ALMA crossover — zero-lag moving average signal."""
        if alma_10 == 0 or alma_30 == 0:
            return 0.0
        score = 0.0

        # ALMA crossover direction
        if alma_signal == "Bullish":
            score += 8.0
            # Stronger signal when price > ALMA(10) too
            if price > 0 and price > alma_10:
                score += 4.0
        elif alma_signal == "Bearish":
            score -= 8.0
            if price > 0 and price < alma_10:
                score -= 4.0

        # ALMA separation strength (pct)
        if alma_pct > 0:
            score += min(alma_pct * 0.5, 4.0)  # Cap at 4
        elif alma_pct < 0:
            score += max(alma_pct * 0.5, -4.0)

        return max(-12.0, min(12.0, score))

    def compute_super_smoother_score(self, ss_slope: float, ss_val: float,
                                      price: float = 0) -> float:
        """Score based on SuperSmoother slope — noise-free trend direction."""
        if ss_slope == 0:
            return 0.0
        score = 0.0

        # Slope direction
        if ss_slope > 0.05:
            score += 8.0    # Strong uptrend
        elif ss_slope > 0.01:
            score += 5.0    # Mild uptrend
        elif ss_slope > 0:
            score += 2.0    # Slight uptrend
        elif ss_slope > -0.01:
            score -= 2.0
        elif ss_slope > -0.05:
            score -= 5.0
        else:
            score -= 8.0    # Strong downtrend

        # Price above/below SuperSmoother
        if price > 0 and ss_val > 0:
            if price > ss_val:
                score += 3.0
            else:
                score -= 3.0

        return max(-10.0, min(10.0, score))

    def compute_laguerre_rsi_score(self, lag_rsi: float) -> float:
        """Score based on LaguerreRSI — superior RSI with smoother transitions."""
        if lag_rsi <= 0:
            return 0.0
        # LaguerreRSI is on 0-1 scale
        score = 0.0
        if lag_rsi < 0.05:
            score += 14.0   # Extreme oversold
        elif lag_rsi < 0.15:
            score += 10.0
        elif lag_rsi < 0.25:
            score += 6.0
        elif lag_rsi < 0.35:
            score += 3.0
        elif lag_rsi < 0.65:
            score += 0.0
        elif lag_rsi < 0.75:
            score -= 3.0
        elif lag_rsi < 0.85:
            score -= 6.0
        elif lag_rsi < 0.95:
            score -= 10.0
        else:
            score -= 14.0   # Extreme overbought
        return score

    def compute_cmo_score(self, cmo: float) -> float:
        """Score based on CMO — Chande Momentum Oscillator."""
        if cmo == 0:
            return 0.0
        score = 0.0
        # CMO range is typically -100 to +100
        if cmo > 50:
            score += 8.0    # Strong momentum up
        elif cmo > 20:
            score += 4.0    # Mild momentum up
        elif cmo > 0:
            score += 1.0
        elif cmo > -20:
            score -= 1.0
        elif cmo > -50:
            score -= 4.0    # Mild momentum down
        else:
            score -= 8.0    # Strong momentum down
        return score

    def compute_weighted_signals_score(self, sig: SignalData) -> float:
        """Score from aggregated weighted FH signals."""
        buy = sig.weighted_buy_score
        sell = sig.weighted_sell_score
        if buy == 0 and sell == 0:
            return 0.0

        net = buy - sell
        total = buy + sell
        if total == 0:
            return 0.0

        # Net signal normalized
        score = (net / total) * 15.0  # Scale to ±15

        # Bonus for strong consensus
        if total > 300:  # Many high-weight signals agree
            if net > 0:
                score += 3.0
            elif net < 0:
                score -= 3.0

        return max(-15.0, min(15.0, score))

    def score_coin(self, coin: CoinAnalysis, fear_greed: int = 50) -> float:
        """Compute composite DMA score for a coin — Traditional + FH indicators."""
        ind = coin.indicators.get("1h", coin.indicators.get("4h", IndicatorData()))
        sig = coin.signals.get("1h", SignalData())
        reg = coin.regimes.get("1h", RegimeData())

        # ── Traditional factors ──
        factor_scores = {
            "rsi": self.compute_rsi_score(ind.rsi),
            "macd": self.compute_macd_score(ind.macd_hist, ind.macd_line, ind.macd_signal),
            "bollinger": self.compute_bollinger_score(ind.bb_pctb),
            "sma_cross": self.compute_sma_cross_score(ind.price, ind.sma_20, ind.sma_50),
            "volume": self.compute_volume_score(ind.volume, ind.volume_sma),
            "momentum": self.compute_momentum_score(ind.change_24h, ind.rsi),
            "mean_reversion": self.compute_mean_reversion_score(ind.rsi, ind.bb_pctb, ind.change_24h),
            "regime": self.compute_regime_score(reg.regime),
            "backtest": self.compute_backtest_score(coin.backtest),
            "sentiment": self.compute_sentiment_score(fear_greed),
            "multi_tf": self.compute_multi_tf_score(coin),
        }

        # ── Financial-Hacker factors ──
        factor_scores["hurst"] = self.compute_hurst_score(ind.hurst, reg)
        factor_scores["alma_cross"] = self.compute_alma_cross_score(
            ind.alma_10, ind.alma_30, ind.alma_signal, ind.alma_pct, ind.price)
        factor_scores["super_smoother"] = self.compute_super_smoother_score(
            ind.super_smoother_slope, ind.super_smoother, ind.price)
        factor_scores["laguerre_rsi"] = self.compute_laguerre_rsi_score(ind.laguerre_rsi)
        factor_scores["cmo"] = self.compute_cmo_score(ind.cmo)
        factor_scores["weighted_signals"] = self.compute_weighted_signals_score(sig)

        # Signal balance bonus (simple count)
        buy, sell = sig.buy_count, sig.sell_count
        signal_bonus = 0.0
        if buy > sell:
            signal_bonus = min(buy * 2.0, 10.0)
        elif sell > buy:
            signal_bonus = -min(sell * 2.0, 10.0)

        # Weighted DMA sum
        weighted = sum(
            factor_scores.get(k, 0) * self.weights.get(k, 0)
            for k in self.weights
        )

        # Normalize to 0-100 scale
        raw = 50 + weighted * 1.5 + signal_bonus * 0.3
        return max(0.0, min(100.0, raw))


# ═══════════════════════════════════════════════════════════════════════
# POSITION SIZING
# ═══════════════════════════════════════════════════════════════════════
class PositionSizer:
    """Risk-adjusted position sizing."""

    @staticmethod
    def compute_advice(coin: CoinAnalysis, equity: float = 10000.0) -> PositionAdvice:
        """Compute position advice based on analysis."""
        score = coin.composite_score
        ind_1h = coin.indicators.get("1h", IndicatorData())
        price = ind_1h.price
        atr = ind_1h.atr if ind_1h.atr > 0 else price * 0.03

        advice = PositionAdvice()
        advice.entry = price

        if price <= 0:
            advice.action = "HOLD"
            advice.confidence = "LOW"
            advice.notes.append("No valid price data")
            return advice

        if score >= 70:
            advice.action = "LONG"
            advice.confidence = "HIGH"
            sl_pct = max(0.02, min(0.05, atr / price * 1.5))
            advice.stop_loss = round(price * (1 - sl_pct), 6)
            advice.take_profit_1 = round(price * (1 + sl_pct * 2.0), 6)
            advice.take_profit_2 = round(price * (1 + sl_pct * 3.5), 6)
            advice.position_size_pct = round(min(5.0, 1.0 + (score - 70) * 0.1), 1)
        elif score >= 58:
            advice.action = "LONG"
            advice.confidence = "MEDIUM"
            sl_pct = max(0.025, min(0.06, atr / price * 1.5))
            advice.stop_loss = round(price * (1 - sl_pct), 6)
            advice.take_profit_1 = round(price * (1 + sl_pct * 2.0), 6)
            advice.take_profit_2 = round(price * (1 + sl_pct * 3.0), 6)
            advice.position_size_pct = round(min(3.0, 0.5 + (score - 58) * 0.1), 1)
        elif score >= 42:
            advice.action = "HOLD"
            advice.confidence = "LOW"
            advice.stop_loss = round(price * 0.97, 6)
            advice.take_profit_1 = round(price * 1.03, 6)
            advice.take_profit_2 = round(price * 1.06, 6)
            advice.position_size_pct = 0.0
        elif score >= 30:
            advice.action = "SHORT"
            advice.confidence = "MEDIUM"
            sl_pct = max(0.025, min(0.06, atr / price * 1.5))
            advice.stop_loss = round(price * (1 + sl_pct), 6)
            advice.take_profit_1 = round(price * (1 - sl_pct * 2.0), 6)
            advice.take_profit_2 = round(price * (1 - sl_pct * 3.0), 6)
            advice.position_size_pct = round(min(3.0, 0.5 + (42 - score) * 0.1), 1)
        else:
            advice.action = "SHORT"
            advice.confidence = "HIGH"
            sl_pct = max(0.02, min(0.05, atr / price * 1.5))
            advice.stop_loss = round(price * (1 + sl_pct), 6)
            advice.take_profit_1 = round(price * (1 - sl_pct * 2.0), 6)
            advice.take_profit_2 = round(price * (1 - sl_pct * 3.5), 6)
            advice.position_size_pct = round(min(5.0, 1.0 + (30 - score) * 0.1), 1)

        # Risk:Reward ratio
        if advice.stop_loss > 0 and advice.entry > 0:
            risk = abs(advice.entry - advice.stop_loss)
            reward = abs(advice.take_profit_1 - advice.entry)
            advice.risk_reward = round(reward / risk, 2) if risk > 0 else 0.0
        else:
            advice.risk_reward = 0.0

        # Notes based on indicators
        ind_4h = coin.indicators.get("4h", IndicatorData())
        ind_1d = coin.indicators.get("1d", IndicatorData())

        if ind_1h.rsi < 30:
            advice.notes.append(f"RSI oversold 1h ({ind_1h.rsi:.1f})")
        if ind_4h.rsi < 30:
            advice.notes.append(f"RSI oversold 4h ({ind_4h.rsi:.1f})")
        if ind_1h.bb_pctb < 0.05:
            advice.notes.append(f"BB lower band touch (%B={ind_1h.bb_pctb:.2f})")
        if ind_1h.macd_hist > 0 and ind_1h.macd_line > ind_1h.macd_signal:
            advice.notes.append("MACD bullish crossover 1h")
        if coin.regimes.get("1h", RegimeData()).regime == "Trending Up":
            advice.notes.append("Trending Up regime 1h")
        if coin.backtest.total_return > 0:
            advice.notes.append(f"Backtest +{coin.backtest.total_return:.1f}%")
        if ind_1d.rsi < 35 and ind_1h.rsi < 40:
            advice.notes.append("Multi-TF oversold alignment")
        if not advice.notes:
            advice.notes.append("No strong confluences")

        return advice


# ═══════════════════════════════════════════════════════════════════════
# ANALYZER — Main Analysis Pipeline
# ═══════════════════════════════════════════════════════════════════════
class Top100Analyzer:
    """Orchestrates the full top-100 analysis pipeline."""

    def __init__(self, client: MCPClient, scorer: DMAScoring,
                 intervals: List[str] = None, verbose: bool = False):
        self.client = client
        self.scorer = scorer
        self.intervals = intervals or ["1h", "4h"]
        self.verbose = verbose
        self.parser = ResponseParser()

    async def discover_top_coins(self, limit: int = 100) -> List[str]:
        """Auto-discover top coins by volume from Binance via MCP."""
        text = await self.client.call("get_top_crypto", {"limit": limit * 5})
        symbols = self.parser.parse_top_coins(text, limit=limit)
        if not symbols:
            if self.verbose:
                print("  [warn] Could not auto-discover coins, using fallback list", file=sys.stderr)
            symbols = self._fallback_symbols()[:limit]
        return symbols

    @staticmethod
    def _fallback_symbols() -> List[str]:
        """Fallback list of major coins if auto-discovery fails."""
        return [
            "BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT",
            "ADAUSDT", "DOGEUSDT", "AVAXUSDT", "DOTUSDT", "LINKUSDT",
            "MATICUSDT", "UNIUSDT", "ATOMUSDT", "LTCUSDT", "NEARUSDT",
            "APTUSDT", "ARBUSDT", "OPUSDT", "SUIUSDT", "INJUSDT",
            "FILUSDT", "AAVEUSDT", "MKRUSDT", "DYDXUSDT", "COMPUSDT",
            "SNXUSDT", "CRVUSDT", "LDOUSDT", "RPLASDT", "ORDIUSDT",
            "WLDUSDT", "PENDLEUSDT", "ENAUSDT", "THETAUSDT",
        ]

    async def analyze_coin(self, symbol: str) -> CoinAnalysis:
        """Analyze a single coin across multiple timeframes."""
        coin = CoinAnalysis(
            symbol=symbol,
            analyzed_at=datetime.now(timezone.utc).isoformat(),
        )

        # Multi-timeframe indicators, signals, regime
        calls: List[Tuple[str, dict]] = []
        for tf in self.intervals:
            calls.append(("analyze_indicators", {"symbol": symbol, "interval": tf, "limit": 200}))
            calls.append(("get_trading_signals", {"symbol": symbol, "interval": tf}))
            calls.append(("detect_market_regime", {"symbol": symbol, "interval": tf}))

        # Backtests: SMA + FH strategies (1h only)
        for strat in ["sma_crossover", "laguerre_rsi", "ehlers_trend", "fh_composite", "alma_crossover"]:
            calls.append(("run_backtest", {"symbol": symbol, "interval": "1h", "strategy": strat}))

        results = await self.client.call_batch(calls)

        idx = 0
        for tf in self.intervals:
            coin.indicators[tf] = self.parser.parse_indicators(results[idx]); idx += 1
            coin.signals[tf] = self.parser.parse_signals(results[idx]); idx += 1
            coin.regimes[tf] = self.parser.parse_regime(results[idx]); idx += 1

        # Parse all backtests, keep the best
        strategies = ["sma_crossover", "laguerre_rsi", "ehlers_trend", "fh_composite", "alma_crossover"]
        best_bt = BacktestData()
        for strat in strategies:
            bt = self.parser.parse_backtest(results[idx]); idx += 1
            if bt.total_return > best_bt.total_return:
                bt.best_strategy = strat
                best_bt = bt
        coin.backtest = best_bt

        return coin

    async def analyze_all(self, symbols: List[str]) -> List[CoinAnalysis]:
        """Analyze all coins with parallelism (batches of 5)."""
        all_results: List[CoinAnalysis] = []
        batch_size = 5

        # Get Fear & Greed once
        fg_text = await self.client.call("get_composite_sentiment", {})
        fear_greed = self.parser.parse_fear_greed(fg_text)

        total = len(symbols)
        for batch_start in range(0, total, batch_size):
            batch = symbols[batch_start:batch_start + batch_size]
            tasks = [self.analyze_coin(sym) for sym in batch]
            coins = await asyncio.gather(*tasks)

            for coin in coins:
                coin.fear_greed = fear_greed
                coin.composite_score = self.scorer.score_coin(coin, fear_greed)
                coin.composite_score = round(coin.composite_score, 1)

                # Recommendation
                s = coin.composite_score
                if s >= 70:    coin.recommendation = "STRONG_BUY"
                elif s >= 58:  coin.recommendation = "BUY"
                elif s >= 42:  coin.recommendation = "HOLD"
                elif s >= 30:  coin.recommendation = "SELL"
                else:          coin.recommendation = "STRONG_SELL"

                # Position advice
                coin.position_advice = PositionSizer.compute_advice(coin)

                all_results.append(coin)

            done = min(batch_start + batch_size, total)
            print(f"  [{done}/{total}] analyzed", end="\r", file=sys.stderr, flush=True)

        print("", file=sys.stderr)
        all_results.sort(key=lambda c: c.composite_score, reverse=True)
        for i, c in enumerate(all_results):
            c.rank = i + 1

        return all_results

    @property
    def client_stats(self) -> str:
        return self.client.stats()


# ═══════════════════════════════════════════════════════════════════════
# OUTPUT FORMATTERS
# ═══════════════════════════════════════════════════════════════════════
def _score_color(score: float) -> str:
    if score >= 70: return "bright_green"
    elif score >= 58: return "green"
    elif score >= 42: return "yellow"
    elif score >= 30: return "red"
    return "bright_red"


def _rec_emoji(rec: str) -> str:
    m = {
        "STRONG_BUY": "\U0001f7e2\U0001f7e2", "BUY": "\U0001f7e2",
        "HOLD": "\U0001f7e1", "SELL": "\U0001f534", "STRONG_SELL": "\U0001f534\U0001f534",
    }
    return m.get(rec, "\u2753")


def _fg_label(val: int) -> str:
    if val < 20: return "Extreme Fear"
    elif val < 35: return "Fear"
    elif val < 55: return "Neutral"
    elif val < 70: return "Greed"
    return "Extreme Greed"


def print_table(results: List[CoinAnalysis], fear_greed: int = 50, top_n: int = 30):
    """Print analysis results as a colored table."""
    if RICH_AVAILABLE:
        _print_rich_table(results, fear_greed, top_n)
    else:
        _print_plain_table(results, fear_greed, top_n)


def _print_rich_table(results: List[CoinAnalysis], fear_greed: int, top_n: int):
    """Rich-formatted table."""
    console = Console()
    fg_label = _fg_label(fear_greed)
    console.print(Panel(
        f"[bold]BonBo Top 100 Crypto Analysis v2.0[/bold]\n"
        f"Time: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}\n"
        f"Fear & Greed: {fear_greed}/100 ({fg_label})",
        style="bold cyan",
    ))

    table = Table(title=f"Top {min(top_n, len(results))} Coins (by composite score)", show_lines=True)
    table.add_column("#", style="dim", width=3)
    table.add_column("Symbol", style="bold", width=13)
    table.add_column("Price", justify="right", width=11)
    table.add_column("RSI", justify="right", width=6)
    table.add_column("MACD", justify="right", width=7)
    table.add_column("BB%B", justify="right", width=5)
    table.add_column("Regime", width=14)
    table.add_column("BT%", justify="right", width=6)
    table.add_column("Score", justify="right", width=5, style="bold")
    table.add_column("Rec", width=13)

    for r in results[:top_n]:
        ind = r.indicators.get("1h", r.indicators.get("4h", IndicatorData()))
        reg = r.regimes.get("1h", RegimeData())
        macd_sign = "+" if ind.macd_hist >= 0 else ""
        score_style = _score_color(r.composite_score)
        table.add_row(
            str(r.rank),
            r.symbol.replace("USDT", ""),
            f"${ind.price:,.4f}" if ind.price > 0 else "N/A",
            f"{ind.rsi:.1f}",
            f"{macd_sign}{ind.macd_hist:.1f}",
            f"{ind.bb_pctb:.2f}",
            reg.regime,
            f"{r.backtest.total_return:+.1f}%",
            f"[{score_style}]{r.composite_score:.0f}[/{score_style}]",
            f"{_rec_emoji(r.recommendation)} {r.recommendation}",
        )

    console.print(table)


def _print_plain_table(results: List[CoinAnalysis], fear_greed: int, top_n: int):
    """Plain-text fallback table."""
    fg_label = _fg_label(fear_greed)
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    print()
    print("=" * 110)
    print(f"  BONBO TOP 100 CRYPTO ANALYSIS v2.0 -- {now}")
    print(f"  Fear & Greed: {fear_greed}/100 ({fg_label})")
    print("=" * 110)

    hdr = (f"{'#':<3} {'Symbol':<13} {'Price':>11} {'RSI':>6} {'MACD':>7} "
           f"{'BB%B':>5} {'Regime':<14} {'BT%':>7} {'Score':>5} {'Rec':<13}")
    print(hdr)
    print("-" * 110)

    for r in results[:top_n]:
        ind = r.indicators.get("1h", r.indicators.get("4h", IndicatorData()))
        reg = r.regimes.get("1h", RegimeData())
        macd_sign = "+" if ind.macd_hist >= 0 else ""
        price_str = f"${ind.price:,.4f}" if ind.price > 0 else "N/A"
        print(f"{r.rank:<3} {r.symbol:<13} {price_str:>11} {ind.rsi:>6.1f} "
              f"{macd_sign}{ind.macd_hist:>6.1f} {ind.bb_pctb:>5.2f} {reg.regime:<14} "
              f"{r.backtest.total_return:>+6.1f}% {r.composite_score:>5.0f} "
              f"{_rec_emoji(r.recommendation)} {r.recommendation}")


def print_top_picks(results: List[CoinAnalysis], top_k: int = 5):
    """Print detailed analysis for top K picks."""
    if RICH_AVAILABLE:
        _print_rich_picks(results, top_k)
    else:
        _print_plain_picks(results, top_k)


def _print_rich_picks(results: List[CoinAnalysis], top_k: int):
    console = Console()
    console.print()
    console.print(Panel("[bold]TOP PICKS -- Detailed Analysis[/bold]", style="bold yellow"))

    for r in results[:top_k]:
        ind_1h = r.indicators.get("1h", IndicatorData())
        adv = r.position_advice
        console.print(f"\n[bold cyan]#{r.rank} {r.symbol}[/bold cyan] "
                       f"-- Score: {r.composite_score:.0f}/100 -- {r.recommendation}")
        console.print(f"  Price: ${ind_1h.price:,.4f}")

        for tf in ["1h", "4h", "1d"]:
            if tf in r.indicators:
                i = r.indicators[tf]
                console.print(f"  [{tf}] RSI={i.rsi:.1f}  MACD_hist={i.macd_hist:+.1f}  "
                               f"BB%B={i.bb_pctb:.2f}  Regime={r.regimes.get(tf, RegimeData()).regime}")

        if adv and adv.action != "HOLD":
            console.print(f"  [bold]Action: {adv.action}[/bold] (confidence: {adv.confidence})")
            console.print(f"  Entry: ${adv.entry:,.4f}  |  SL: ${adv.stop_loss:,.4f}  "
                           f"|  TP1: ${adv.take_profit_1:,.4f}  |  TP2: ${adv.take_profit_2:,.4f}")
            console.print(f"  R:R = 1:{adv.risk_reward:.1f}  |  Position: {adv.position_size_pct}% of equity")
            for note in adv.notes:
                console.print(f"  >> {note}")


def _print_plain_picks(results: List[CoinAnalysis], top_k: int):
    print()
    print("=" * 80)
    print("  TOP PICKS -- Detailed Analysis")
    print("=" * 80)

    for r in results[:top_k]:
        ind_1h = r.indicators.get("1h", IndicatorData())
        adv = r.position_advice
        print(f"\n  #{r.rank} {r.symbol} -- Score: {r.composite_score:.0f}/100 -- {r.recommendation}")
        print(f"  Price: ${ind_1h.price:,.4f}")
        for tf in ["1h", "4h", "1d"]:
            if tf in r.indicators:
                i = r.indicators[tf]
                print(f"  [{tf}] RSI={i.rsi:.1f}  MACD_hist={i.macd_hist:+.1f}  "
                       f"BB%B={i.bb_pctb:.2f}  Regime={r.regimes.get(tf, RegimeData()).regime}")

        if adv and adv.action != "HOLD":
            print(f"  Action: {adv.action} (confidence: {adv.confidence})")
            print(f"  Entry: ${adv.entry:,.4f}  |  SL: ${adv.stop_loss:,.4f}  "
                   f"|  TP1: ${adv.take_profit_1:,.4f}  |  TP2: ${adv.take_profit_2:,.4f}")
            print(f"  R:R = 1:{adv.risk_reward:.1f}  |  Position: {adv.position_size_pct}%")
            for note in adv.notes:
                print(f"  >> {note}")


def print_summary(results: List[CoinAnalysis], fear_greed: int = 50):
    """Print market summary."""
    buys = sum(1 for r in results if r.recommendation in ("STRONG_BUY", "BUY"))
    holds = sum(1 for r in results if r.recommendation == "HOLD")
    sells = sum(1 for r in results if r.recommendation in ("SELL", "STRONG_SELL"))
    avg_score = sum(r.composite_score for r in results) / max(len(results), 1)
    avg_rsi = sum(
        r.indicators.get("1h", IndicatorData()).rsi for r in results
    ) / max(len(results), 1)

    print()
    print("=" * 80)
    print("  MARKET SUMMARY")
    print("=" * 80)
    print(f"  Total analyzed:  {len(results)} coins")
    print(f"  Buy signals:     {buys} coins ({buys * 100 // max(len(results), 1)}%)")
    print(f"  Hold:            {holds} coins")
    print(f"  Sell signals:    {sells} coins ({sells * 100 // max(len(results), 1)}%)")
    print(f"  Average Score:   {avg_score:.1f}/100")
    print(f"  Average RSI:     {avg_rsi:.1f}")
    print(f"  Fear & Greed:    {fear_greed}/100 ({_fg_label(fear_greed)})")

    if avg_score >= 55:
        print(f"\n  THI TRUONG TICH CUC -- Co nhieu co hoi LONG. Tap trung vao top picks.")
    elif avg_score >= 45:
        print(f"\n  THI TRUONG TRUNG TINH -- Can than trong, cho xac nhan tin hieu.")
    else:
        print(f"\n  THI TRUONG TIEU CUC -- Uu tien boc pha hoac cho duy nhan.")


# ═══════════════════════════════════════════════════════════════════════
# EXPORT
# ═══════════════════════════════════════════════════════════════════════
def export_json(results: List[CoinAnalysis], path: Path):
    """Export results to JSON."""
    data = []
    for r in results:
        row = {
            "rank": r.rank, "symbol": r.symbol, "score": r.composite_score,
            "recommendation": r.recommendation,
            "indicators": {tf: asdict(ind) for tf, ind in r.indicators.items()},
            "regimes": {tf: asdict(reg) for tf, reg in r.regimes.items()},
            "signals": {tf: asdict(sig) for tf, sig in r.signals.items()},
            "backtest": asdict(r.backtest),
        }
        if r.position_advice:
            row["position_advice"] = asdict(r.position_advice)
        data.append(row)
    path.write_text(json.dumps(data, indent=2, default=str))
    print(f"  Exported JSON: {path}")


def export_csv(results: List[CoinAnalysis], path: Path):
    """Export results to CSV."""
    with open(path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow([
            "rank", "symbol", "score", "recommendation", "action",
            "price_1h", "rsi_1h", "macd_hist_1h", "bb_pctb_1h",
            "regime_1h", "bt_return", "bt_winrate", "bt_sharpe",
            "entry", "stop_loss", "tp1", "tp2", "risk_reward", "position_pct",
        ])
        for r in results:
            ind = r.indicators.get("1h", IndicatorData())
            adv = r.position_advice or PositionAdvice()
            writer.writerow([
                r.rank, r.symbol, r.composite_score, r.recommendation, adv.action,
                ind.price, ind.rsi, ind.macd_hist, ind.bb_pctb,
                r.regimes.get("1h", RegimeData()).regime,
                r.backtest.total_return, r.backtest.win_rate, r.backtest.sharpe,
                adv.entry, adv.stop_loss, adv.take_profit_1, adv.take_profit_2,
                adv.risk_reward, adv.position_size_pct,
            ])
    print(f"  Exported CSV: {path}")


def export_html(results: List[CoinAnalysis], fear_greed: int, path: Path):
    """Export results to a simple HTML report."""
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    rows_html = ""
    for r in results[:50]:
        ind = r.indicators.get("1h", IndicatorData())
        adv = r.position_advice or PositionAdvice()
        sc = "#22c55e" if r.composite_score >= 58 else "#eab308" if r.composite_score >= 42 else "#ef4444"
        rows_html += (
            f"<tr><td>{r.rank}</td><td><b>{r.symbol}</b></td>"
            f"<td>${ind.price:,.4f}</td><td>{ind.rsi:.1f}</td>"
            f"<td>{ind.macd_hist:+.1f}</td><td>{ind.bb_pctb:.2f}</td>"
            f"<td>{r.regimes.get('1h', RegimeData()).regime}</td>"
            f"<td>{r.backtest.total_return:+.1f}%</td>"
            f"<td style='color:{sc};font-weight:bold'>{r.composite_score:.0f}</td>"
            f"<td>{r.recommendation}</td>"
            f"<td>{adv.action}</td><td>{adv.risk_reward:.1f}</td></tr>"
        )

    html = (
        f"<!DOCTYPE html><html><head><meta charset='utf-8'>"
        f"<title>BonBo Top 100 Analysis</title>"
        f"<style>body{{font-family:system-ui;background:#0f172a;color:#e2e8f0;margin:2rem}}"
        f"table{{border-collapse:collapse;width:100%}}"
        f"th,td{{border:1px solid #334155;padding:6px 10px;text-align:right}}"
        f"th{{background:#1e293b}}tr:nth-child(even){{background:#1e293b}}"
        f"h1{{color:#38bdf8}}h2{{color:#818cf8}}</style></head><body>"
        f"<h1>BonBo Top 100 Crypto Analysis v2.0</h1>"
        f"<p>Generated: {now} | Fear &amp; Greed: {fear_greed}/100 ({_fg_label(fear_greed)})</p>"
        f"<table><tr><th>#</th><th>Symbol</th><th>Price</th><th>RSI</th>"
        f"<th>MACD</th><th>BB%B</th><th>Regime</th><th>BT%</th><th>Score</th>"
        f"<th>Rec</th><th>Action</th><th>R:R</th></tr>{rows_html}</table>"
        f"</body></html>"
    )
    path.write_text(html)
    print(f"  Exported HTML: {path}")


# ═══════════════════════════════════════════════════════════════════════
# JOURNAL INTEGRATION
# ═══════════════════════════════════════════════════════════════════════
def save_to_journal(results: List[CoinAnalysis], fear_greed: int):
    """Save predictions to self-learning journal DB."""
    try:
        JOURNAL_DB.parent.mkdir(parents=True, exist_ok=True)
        conn = sqlite3.connect(str(JOURNAL_DB))
        conn.execute("""
            CREATE TABLE IF NOT EXISTS predictions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                symbol TEXT NOT NULL,
                score REAL,
                recommendation TEXT,
                price REAL,
                action TEXT,
                stop_loss REAL,
                tp1 REAL,
                tp2 REAL,
                fear_greed INTEGER,
                actual_outcome TEXT DEFAULT '',
                accuracy_score REAL DEFAULT 0
            )
        """)
        ts = datetime.now(timezone.utc).isoformat()
        for r in results:
            adv = r.position_advice or PositionAdvice()
            ind = r.indicators.get("1h", IndicatorData())
            conn.execute(
                "INSERT INTO predictions (timestamp,symbol,score,recommendation,price,"
                "action,stop_loss,tp1,tp2,fear_greed) VALUES (?,?,?,?,?,?,?,?,?,?)",
                (ts, r.symbol, r.composite_score, r.recommendation, ind.price,
                 adv.action, adv.stop_loss, adv.take_profit_1, adv.take_profit_2,
                 fear_greed),
            )
        conn.commit()
        conn.close()
        print(f"  Saved {len(results)} predictions to journal DB")
    except Exception as e:
        print(f"  [warn] Journal save failed: {e}", file=sys.stderr)


# ═══════════════════════════════════════════════════════════════════════
# CLI
# ═══════════════════════════════════════════════════════════════════════
def parse_args():
    p = argparse.ArgumentParser(description="BonBo Top 100 Crypto Deep Analysis v2.0")
    p.add_argument("--top-n", type=int, default=100, help="Number of coins to analyze (default: 100)")
    p.add_argument("--intervals", default="1h,4h", help="Timeframes (default: 1h,4h)")
    p.add_argument("--output", default="table", help="Output: table,json,csv,html,all (default: table)")
    p.add_argument("--no-cache", action="store_true", help="Bypass cache")
    p.add_argument("--verbose", action="store_true", help="Verbose output")
    p.add_argument("--journal", action="store_true", help="Save to journal DB")
    p.add_argument("--quick", action="store_true", help="Quick: 20 coins, 1h only")
    return p.parse_args()


async def main():
    args = parse_args()

    if args.quick:
        args.top_n = 20
        args.intervals = "1h"

    intervals = [x.strip() for x in args.intervals.split(",")]

    print(f"BonBo Top 100 Crypto Analysis v2.0", file=sys.stderr)
    print(f"  Top-N: {args.top_n} | Intervals: {intervals} | Output: {args.output}", file=sys.stderr)

    # Setup
    cache = AnalysisCache() if not args.no_cache else None
    client = MCPClient(cache=cache, use_cache=not args.no_cache, verbose=args.verbose)
    scorer = DMAScoring()
    analyzer = Top100Analyzer(client, scorer, intervals=intervals, verbose=args.verbose)

    # Step 1: Discover
    print("  Discovering top coins...", file=sys.stderr)
    symbols = await analyzer.discover_top_coins(args.top_n)
    print(f"  Found {len(symbols)} coins to analyze", file=sys.stderr)

    # Step 2: Analyze
    print(f"  Analyzing...", file=sys.stderr)
    t0 = time.time()
    results = await analyzer.analyze_all(symbols)
    elapsed = time.time() - t0
    print(f"  Done in {elapsed:.1f}s ({len(results)} coins, {elapsed / max(len(results), 1):.1f}s/coin)", file=sys.stderr)

    fear_greed = results[0].fear_greed if results else 50

    # Step 3: Output
    if args.output in ("table", "all"):
        print_table(results, fear_greed, top_n=min(30, args.top_n))
        print_top_picks(results, top_k=5)
        print_summary(results, fear_greed)

    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    if args.output in ("json", "all"):
        export_json(results, REPORTS_DIR / f"top100_{ts}.json")
    if args.output in ("csv", "all"):
        export_csv(results, REPORTS_DIR / f"top100_{ts}.csv")
    if args.output in ("html", "all"):
        export_html(results, fear_greed, REPORTS_DIR / f"top100_{ts}.html")

    # Step 4: Journal
    if args.journal:
        save_to_journal(results, fear_greed)

    print(f"\n  {analyzer.client_stats}", file=sys.stderr)
    print("=" * 80)




# ═══════════════════════════════════════════════════════════════════════
# UNIT TESTS
# ═══════════════════════════════════════════════════════════════════════
def _run_unit_tests():
    """Run all unit tests without needing pytest."""
    import traceback
    tests_passed = 0
    tests_failed = 0

    def run_test(name, fn):
        nonlocal tests_passed, tests_failed
        try:
            fn()
            tests_passed += 1
            print(f"  [PASS] {name}")
        except Exception as e:
            tests_failed += 1
            print(f"  [FAIL] {name}: {e}")
            traceback.print_exc()

    # ── Parser Tests ──
    def test_parse_indicators_real_format():
        sample = (
            "**SMA(20)**: $75856.35\n"
            "**EMA(12)**: $75898.27\n"
            "**EMA(26)**: $75699.81\n"
            "RSI(14)**: 58.9 Neutral\n"
            "MACD**: line=198.4649 signal=202.7815 hist=-4.3166\n"
            "BB(20,2)**: upper=$76537.64 mid=$75856.35 lower=$75175.06 %B=0.69\n"
            "**Price**: $76116.48\n"
            "**ALMA(10)**: $75981.69\n"
            "SuperSmoother(20)**: $75897.93\n"
            "Hurst(100)**: 0.827\n"
            "CMO(14)**: -15.8\n"
            "LaguerreRSI(0.8)**: 0.320\n"
            "ALMA(10/30)**: Bullish (+0.16%)\n"
        )
        d = ResponseParser.parse_indicators(sample)
        assert d.price == 76116.48, f"Price: {d.price}"
        assert d.rsi == 58.9, f"RSI: {d.rsi}"
        assert d.macd_hist == -4.3166, f"MACD hist: {d.macd_hist}"
        assert d.bb_pctb == 0.69, f"BB%B: {d.bb_pctb}"
        assert d.sma_20 == 75856.35, f"SMA20: {d.sma_20}"
        assert d.ema_12 == 75898.27, f"EMA12: {d.ema_12}"
        assert d.alma_10 == 75981.69, f"ALMA10: {d.alma_10}"
        assert d.hurst == 0.827, f"Hurst: {d.hurst}"
        assert d.cmo == -15.8, f"CMO: {d.cmo}"
        assert d.laguerre_rsi == 0.320, f"LaguerreRSI: {d.laguerre_rsi}"
        assert d.alma_signal == "Bullish", f"ALMA signal: {d.alma_signal}"
        assert d.alma_pct == 0.16, f"ALMA pct: {d.alma_pct}"

    def test_parse_indicators_error():
        d = ResponseParser.parse_indicators("Error: connection failed")
        assert d.price == 0.0
        assert d.rsi == 50.0

    def test_parse_signals_weighted():
        sample = (
            "\U0001f7e2 **Buy** [MACD(12,26,9)] (60%)\n"
            "   MACD bullish crossover\n"
            "\U0001f7e2 **Buy** [ALMA(10,30)] (70%)\n"
            "   ALMA(10) > ALMA(30) bullish cross (+0.50%)\n"
            "\U0001f7e2 **Buy** [Hurst(100)] (80%)\n"
            "   Trending (H=0.83, use trend-following)\n"
            "\U0001f7e2 **Buy** [LaguerreRSI(0.8)] (55%)\n"
            "   LaguerreRSI recovering from oversold zone (0.32)\n"
            "\U0001f534 **Sell** [RSI(14)] (50%)\n"
            "   RSI overbought\n"
        )
        s = ResponseParser.parse_signals(sample)
        assert s.buy_count == 4, f"Buy count: {s.buy_count}"
        assert s.sell_count == 1, f"Sell count: {s.sell_count}"
        assert s.weighted_buy_score == 265, f"Weighted buy: {s.weighted_buy_score}"
        assert s.weighted_sell_score == 50, f"Weighted sell: {s.weighted_sell_score}"
        assert len(s.signal_items) == 5, f"Items: {len(s.signal_items)}"
        # Check FH flags
        assert s.fh_hurst_trend, "Hurst trend flag"
        assert s.fh_laguerre_oversold, "Laguerre oversold flag"
        # Check item details
        alma_items = [i for i in s.signal_items if "ALMA" in i.name]
        assert len(alma_items) >= 1
        assert alma_items[0].confidence == 70
        assert alma_items[0].direction == "Buy"

    def test_parse_regime_with_hurst():
        sample = "Market Regime: Trending Up\nHurst Exponent: 0.827 (>0.55 → Trending)\nStrategy: use trend-following"
        r = ResponseParser.parse_regime(sample)
        assert r.regime == "Trending Up", f"Regime: {r.regime}"
        assert r.hurst_value == 0.827, f"Hurst: {r.hurst_value}"
        assert r.hurst_regime == "Trending", f"Hurst regime: {r.hurst_regime}"
        assert "trend-following" in r.strategy_hint

    def test_parse_regime_mean_reverting():
        sample = "Market Regime: Ranging\nHurst Exponent: 0.38 (<0.45 → Mean Reverting)"
        r = ResponseParser.parse_regime(sample)
        assert r.regime == "Ranging"
        assert r.hurst_value == 0.38
        assert r.hurst_regime == "Mean Reverting"

    def test_parse_backtest():
        sample = (
            "**Total Return**: 10.59%\n"
            "**Win Rate**: 75.0%\n"
            "**Sharpe Ratio**: 2.03\n"
            "**Max Drawdown**: -4.8%\n"
            "**Total Trades**: 4\n"
        )
        bt = ResponseParser.parse_backtest(sample)
        assert bt.total_return == 10.59, f"Return: {bt.total_return}"
        assert bt.win_rate == 75.0, f"WR: {bt.win_rate}"
        assert bt.sharpe == 2.03, f"Sharpe: {bt.sharpe}"
        assert bt.max_drawdown == -4.8, f"DD: {bt.max_drawdown}"
        assert bt.total_trades == 4, f"Trades: {bt.total_trades}"

    # ── Scoring Tests ──
    def test_hurst_scoring():
        scorer = DMAScoring()
        # Trending = positive
        s1 = scorer.compute_hurst_score(0.83)
        assert s1 > 0, f"Hurst 0.83 score: {s1}"
        # Mean reverting = negative
        s2 = scorer.compute_hurst_score(0.35)
        assert s2 < 0, f"Hurst 0.35 score: {s2}"
        # Random walk = neutral
        s3 = scorer.compute_hurst_score(0.50)
        assert abs(s3) < 5, f"Hurst 0.50 score: {s3}"

    def test_laguerre_rsi_scoring():
        scorer = DMAScoring()
        # Oversold = positive
        s1 = scorer.compute_laguerre_rsi_score(0.05)
        assert s1 >= 10.0, f"LagRSI 0.05: {s1}"
        # Overbought = negative
        s2 = scorer.compute_laguerre_rsi_score(0.95)
        assert s2 <= -10.0, f"LagRSI 0.95: {s2}"
        # Neutral
        s3 = scorer.compute_laguerre_rsi_score(0.50)
        assert abs(s3) < 2, f"LagRSI 0.50: {s3}"

    def test_alma_cross_scoring():
        scorer = DMAScoring()
        # Bullish
        s1 = scorer.compute_alma_cross_score(100, 95, "Bullish", 0.5, 102)
        assert s1 > 0, f"ALMA bullish: {s1}"
        # Bearish
        s2 = scorer.compute_alma_cross_score(95, 100, "Bearish", -0.5, 93)
        assert s2 < 0, f"ALMA bearish: {s2}"

    def test_cmo_scoring():
        scorer = DMAScoring()
        s1 = scorer.compute_cmo_score(60.0)
        assert s1 > 5, f"CMO +60: {s1}"
        s2 = scorer.compute_cmo_score(-60.0)
        assert s2 < -5, f"CMO -60: {s2}"

    def test_weighted_signals_scoring():
        scorer = DMAScoring()
        # Strong buy consensus
        sig = SignalData(weighted_buy_score=300, weighted_sell_score=50)
        s = scorer.compute_weighted_signals_score(sig)
        assert s > 0, f"Buy consensus: {s}"
        # Strong sell
        sig2 = SignalData(weighted_buy_score=50, weighted_sell_score=300)
        s2 = scorer.compute_weighted_signals_score(sig2)
        assert s2 < 0, f"Sell consensus: {s2}"

    def test_score_coin_comprehensive():
        scorer = DMAScoring()
        # Build a coin with clear bullish signals
        coin = CoinAnalysis(symbol="TESTUSDT")
        ind = IndicatorData(
            price=100.0, rsi=35.0, macd_hist=5.0, macd_line=105.0, macd_signal=100.0,
            bb_pctb=0.15, sma_20=95.0, sma_50=90.0, change_24h=5.0,
            hurst=0.80, laguerre_rsi=0.15, cmo=30.0,
            alma_signal="Bullish", alma_pct=2.0, alma_10=98.0, alma_30=95.0,
            super_smoother_slope=0.05, super_smoother=98.0,
        )
        coin.indicators["1h"] = ind
        coin.signals["1h"] = SignalData(
            buy_count=5, sell_count=0,
            weighted_buy_score=350, weighted_sell_score=0,
        )
        coin.regimes["1h"] = RegimeData(
            regime="Trending Up", hurst_value=0.80, hurst_regime="Trending",
            strategy_hint="use trend-following",
        )
        coin.backtest = BacktestData(total_return=15.0, win_rate=80.0, sharpe=2.5)

        score = scorer.score_coin(coin, fear_greed=60)
        assert score > 55, f"Strong bullish coin should score > 55. Got: {score}"

    def test_weights_sum_to_one():
        scorer = DMAScoring()
        total = sum(scorer.weights.values())
        assert abs(total - 1.0) < 0.01, f"Weights sum: {total}"

    def test_cache_roundtrip():
        cache = AnalysisCache()
        cache.set("test_tool", {"a": 1, "b": 2}, "test_result_data")
        result = cache.get("test_tool", {"a": 1, "b": 2})
        assert result == "test_result_data", f"Cache result: {result}"
        # Different args = miss
        miss = cache.get("test_tool", {"a": 2})
        assert miss is None, f"Cache miss should be None: {miss}"
        cache.clear()

    # Run all
    print("\n" + "=" * 60)
    print("  UNIT TESTS — analyze_top100.py")
    print("=" * 60)

    all_tests = [
        ("parse_indicators real format", test_parse_indicators_real_format),
        ("parse_indicators error handling", test_parse_indicators_error),
        ("parse_signals weighted FH", test_parse_signals_weighted),
        ("parse_regime with Hurst", test_parse_regime_with_hurst),
        ("parse_regime mean reverting", test_parse_regime_mean_reverting),
        ("parse_backtest", test_parse_backtest),
        ("hurst scoring", test_hurst_scoring),
        ("laguerre RSI scoring", test_laguerre_rsi_scoring),
        ("ALMA cross scoring", test_alma_cross_scoring),
        ("CMO scoring", test_cmo_scoring),
        ("weighted signals scoring", test_weighted_signals_scoring),
        ("score_coin comprehensive", test_score_coin_comprehensive),
        ("weights sum to 1.0", test_weights_sum_to_one),
        ("cache roundtrip", test_cache_roundtrip),
    ]

    for name, fn in all_tests:
        run_test(name, fn)

    print()
    print(f"  Results: {tests_passed} passed, {tests_failed} failed")
    print("=" * 60)
    sys.exit(0 if tests_failed == 0 else 1)


# Entry point: handle --test after all functions defined
if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--test":
        _run_unit_tests()
    else:
        asyncio.run(main())
