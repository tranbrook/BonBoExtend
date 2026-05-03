#!/usr/bin/env python3
"""
BonBo Top-100 Coin Analyzer
============================
Phân tích top 100 coin Binance Futures dùng bonbo-extend-mcp.

Áp dụng 10 cải thiện từ trading-process-improvement.md:
  #1  Multi-Timeframe Analysis (1H + 15M confluence)
  #2  Hybrid Hurst Regime Detection (short/long divergence)
  #3  Signal Aggregation (weighted confluence scoring)
  #4  Dynamic Scanning (top-volume + hot-movers merge)
  #5  Smart Position Sizing (Kelly + ATR + regime-conditional)
  #6  ATR-Based Stop Loss (regime-adjusted multiplier)
  #7  Hurst Divergence Handling (confidence adjustment)
  #8  LaguerreRSI Dual-Gamma (fast/slow divergence)
  #9  Correlation / Portfolio filter
  #10 Multi-Strategy recommendation per regime

Usage:
    python3 analyze_top100.py
    python3 analyze_top100.py --top 50
    python3 analyze_top100.py --quick          # skip deep analysis
    python3 analyze_top100.py --output report.md
"""

import subprocess
import json
import sys
import os
import time
import argparse
import re
from dataclasses import dataclass, field, asdict
from typing import Optional
from datetime import datetime, timezone
from concurrent.futures import ThreadPoolExecutor, as_completed

# ─── Configuration ─────────────────────────────────────────────────────────────

MCP_BINARY = os.path.expanduser(
    "~/BonBoExtend/target/release/bonbo-extend-mcp"
)
ENV_LOG_LEVEL = "BONBO_EXTEND_LOG=error"
DEFAULT_TOP = 100
MIN_VOLUME_USD = 1_000_000       # lọc coin thanh khoản thấp
MIN_SCORE_TO_DETAIL = 55         # ngưỡng phân tích sâu
DEEP_ANALYSIS_TOP_N = 20         # giới hạn deep analysis để tránh rate limit
REQUEST_DELAY = 0.05             # giây giữa các MCP call


# ─── Data Classes ──────────────────────────────────────────────────────────────

@dataclass
class MarketContext:
    """Bối cảnh thị trường (Step 2 - Sentiment)."""
    fear_greed_value: int = 0
    fear_greed_label: str = ""
    composite_sentiment: float = 0.0
    composite_label: str = ""
    whale_alerts: str = ""

    @property
    def is_fear(self) -> bool:
        return self.fear_greed_value < 40

    @property
    def is_greed(self) -> bool:
        return self.fear_greed_value > 70

    @property
    def is_contrarian_buy_zone(self) -> bool:
        return self.fear_greed_value < 25


@dataclass
class HurstInfo:
    """Hurst regime data (#2, #7)."""
    hurst_short: float = 0.0      # H(50)
    hurst_long: float = 0.0       # H(100)
    hurst_divergence: float = 0.0 # |short - long|
    regime: str = "Unknown"       # Trending / Mean-Reverting / Random Walk
    confidence_factor: float = 1.0

    def compute_confidence(self):
        """#7: Hurst divergence → confidence adjustment."""
        self.hurst_divergence = abs(self.hurst_short - self.hurst_long)
        if self.hurst_divergence > 0.15:
            self.confidence_factor = 0.5
        elif self.hurst_divergence > 0.10:
            self.confidence_factor = 0.75
        else:
            self.confidence_factor = 1.0


@dataclass
class IndicatorData:
    """Parsed indicator values."""
    # Traditional
    sma20: float = 0.0
    ema12: float = 0.0
    ema26: float = 0.0
    rsi: float = 0.0
    macd_hist: float = 0.0
    bb_upper: float = 0.0
    bb_mid: float = 0.0
    bb_lower: float = 0.0
    bb_pct_b: float = 0.0

    # Financial-Hacker
    alma10: float = 0.0
    alma30: float = 0.0
    alma_cross_pct: float = 0.0
    supersmoother_slope: float = 0.0
    hurst_val: float = 0.0
    hurst_short: float = 0.0
    cmo: float = 0.0
    laguerre_fast: float = 0.0     # γ=0.3
    laguerre_slow: float = 0.0     # γ=0.6
    laguerre_divergence: float = 0.0  # #8: fast - slow
    atr: float = 0.0
    atr_sl_long: float = 0.0
    atr_sl_short: float = 0.0

    # Hurst info
    hurst: HurstInfo = field(default_factory=HurstInfo)


@dataclass
class SignalResult:
    """Parsed trading signals (#3 aggregation)."""
    buy_count: int = 0
    sell_count: int = 0
    neutral_count: int = 0
    weighted_buy_score: float = 0.0
    weighted_sell_score: float = 0.0
    signals_detail: list = field(default_factory=list)


@dataclass
class SupportResistance:
    """S/R levels."""
    resistances: list = field(default_factory=list)
    supports: list = field(default_factory=list)


@dataclass
class CoinAnalysis:
    """Complete analysis result for one coin."""
    symbol: str = ""
    price: float = 0.0
    change_24h: float = 0.0
    volume_24h: float = 0.0
    high_24h: float = 0.0
    low_24h: float = 0.0

    # Quick scan score
    scan_score: int = 0
    scan_regime: str = ""

    # Multi-timeframe (#1)
    indicators_1h: Optional[IndicatorData] = None
    signals_1h: Optional[SignalResult] = None
    indicators_15m: Optional[IndicatorData] = None
    signals_15m: Optional[SignalResult] = None

    # Regime
    regime_1h: str = ""
    regime_15m: str = ""

    # S/R
    sr: Optional[SupportResistance] = None

    # Scoring (#3 aggregation)
    composite_score: float = 0.0
    action: str = "HOLD"  # STRONG_BUY / BUY / HOLD / SELL / STRONG_SELL

    # Risk (#5, #6)
    recommended_sl: float = 0.0
    recommended_tp: float = 0.0
    risk_reward_ratio: float = 0.0
    position_size_pct: float = 0.0  # % equity
    regime_size_multiplier: float = 1.0

    # Strategy (#10)
    recommended_strategy: str = ""
    entry_type: str = ""  # PULLBACK / BREAKOUT / MOMENTUM
    confidence: float = 0.0

    # Warnings
    warnings: list = field(default_factory=list)


# ─── MCP Client ────────────────────────────────────────────────────────────────

class MCPClient:
    """JSON-RPC client cho bonbo-extend-mcp via stdio."""

    def __init__(self, binary_path: str):
        self.binary_path = binary_path
        self.proc: Optional[subprocess.Popen] = None
        self._request_id = 0
        self._initialized = False

    def start(self):
        env = os.environ.copy()
        env["BONBO_EXTEND_LOG"] = "error"
        self.proc = subprocess.Popen(
            [self.binary_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=env,
            bufsize=0,
        )
        # Initialize
        self._call("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "top100-analyzer", "version": "1.0"},
        })
        self._initialized = True

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                self.proc.stdin.close()
                self.proc.terminate()
                self.proc.wait(timeout=5)
            except Exception:
                self.proc.kill()

    def call_tool(self, name: str, arguments: dict = None) -> dict:
        """Call MCP tool, return parsed result dict."""
        if arguments is None:
            arguments = {}
        result = self._call("tools/call", {
            "name": name,
            "arguments": arguments,
        })
        return result

    def _call(self, method: str, params: dict) -> dict:
        self._request_id += 1
        request = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": method,
            "params": params,
        }
        line = json.dumps(request) + "\n"
        self.proc.stdin.write(line.encode())
        self.proc.stdin.flush()

        response_line = self.proc.stdout.readline().decode().strip()
        if not response_line:
            return {"error": "empty response"}

        response = json.loads(response_line)

        if "error" in response:
            return {"error": response["error"].get("message", "unknown")}

        result = response.get("result", {})

        # Extract text content from MCP tool results
        if "content" in result and isinstance(result["content"], list):
            texts = [
                c.get("text", "") for c in result["content"] if c.get("type") == "text"
            ]
            return {"text": "\n".join(texts)}

        return result

    def call_tool_batch(self, calls: list[tuple[str, dict]]) -> list[dict]:
        """Send multiple tool calls sequentially (MCP stdio = sequential)."""
        results = []
        for name, args in calls:
            results.append(self.call_tool(name, args))
            time.sleep(REQUEST_DELAY)
        return results


# ─── Text Parsers ──────────────────────────────────────────────────────────────

def parse_price(text: str) -> dict:
    """Parse get_crypto_price output."""
    result = {}
    m = re.search(r"Price:\s*\$?([\d.]+)", text)
    if m:
        result["price"] = float(m.group(1))
    m = re.search(r"24h Change:\s*([-\d.]+)%", text)
    if m:
        result["change_24h"] = float(m.group(1))
    m = re.search(r"High:\s*\$?([\d.]+)", text)
    if m:
        result["high_24h"] = float(m.group(1))
    m = re.search(r"Low:\s*\$?([\d.]+)", text)
    if m:
        result["low_24h"] = float(m.group(1))
    m = re.search(r"Volume:\s*[\d.]+\s*\w+\s*\|\s*\$?([\d.]+)", text)
    if m:
        result["volume_usd"] = float(m.group(1))
    return result


def parse_top_crypto(text: str) -> list[dict]:
    """Parse get_top_crypto output → list of {symbol, price, change, volume}."""
    coins = []
    for line in text.split("\n"):
        # Match: | 1 | BTCUSDT | $77221.36 | 📉-2.278 | $1149494481.93 |
        m = re.match(
            r"\|\s*(\d+)\s*\|\s*(\w+)\s*\|\s*\$?([\d.eE+-]+)\s*\|\s*(?:📈|📉)?\s*([-\d.]+)\s*\|\s*\$?([\d.eE+-]+)",
            line.strip(),
        )
        if m:
            symbol = m.group(2)
            price_str = m.group(3)
            change_str = m.group(4)
            vol_str = m.group(5)
            if not symbol.endswith("USDT"):
                continue
            try:
                price = float(price_str)
            except ValueError:
                price = 0.0
            try:
                change = float(change_str)
            except ValueError:
                change = 0.0
            try:
                volume = float(vol_str)
            except ValueError:
                volume = 0.0
            coins.append({
                "symbol": symbol,
                "price": price,
                "change_24h": change,
                "volume_usd": volume,
            })
    return coins


def parse_scan_hot_movers(text: str) -> list[dict]:
    """Parse scan_hot_movers output → list of coin data."""
    coins = []
    for line in text.split("\n"):
        m = re.search(
            r"\*\*(\w+USDT)\*\*\s*\$?([\d.eE+-]+)\s*\|\s*Score:\s*(\d+)\s*\|\s*(\w+)\s*H=([\d.]+)\s*\|\s*([\w-]+)",
            line,
        )
        if m:
            coins.append({
                "symbol": m.group(1),
                "price": float(m.group(2)),
                "scan_score": int(m.group(3)),
                "volatility": m.group(4),
                "hurst": float(m.group(5)),
                "strategy_hint": m.group(6),
                "source": "hot_movers",
            })
    return coins


def parse_indicators(text: str) -> IndicatorData:
    """Parse analyze_indicators output."""
    d = IndicatorData()

    def _find(pattern: str, group: int = 1) -> Optional[str]:
        m = re.search(pattern, text)
        return m.group(group) if m else None

    def _float(pattern: str) -> float:
        val = _find(pattern)
        if val:
            try:
                return float(val.replace(",", ""))
            except ValueError:
                return 0.0
        return 0.0

    # Traditional
    d.sma20 = _float(r"SMA\(20\).*?:\s*\$?([\d.]+)")
    d.ema12 = _float(r"EMA\(12\).*?:\s*\$?([\d.]+)")
    d.ema26 = _float(r"EMA\(26\).*?:\s*\$?([\d.]+)")
    d.rsi = _float(r"RSI\(14\).*?:\s*([\d.]+)")
    d.macd_hist = _float(r"hist=([-\d.]+)")
    d.bb_upper = _float(r"upper=\$?([\d.]+)")
    d.bb_mid = _float(r"mid=\$?([\d.]+)")
    d.bb_lower = _float(r"lower=\$?([\d.]+)")
    d.bb_pct_b = _float(r"%B=([\d.]+)")

    # Financial-Hacker
    d.alma10 = _float(r"ALMA\(10\).*?=\s*\$?([\d.]+)")
    d.alma30 = _float(r"ALMA\(30\).*?=\s*\$?([\d.]+)")
    d.alma_cross_pct = _float(r"(?:Bullish|Bearish)\s*\(([+-]?[\d.]+)%\)")
    d.supersmoother_slope = _float(r"slope:\s*([+-]?[\d.]+)%")

    d.hurst_val = _float(r"Hurst\(100\).*?:\s*([\d.]+)")
    d.hurst_short = _float(r"Hurst\(50\).*?:\s*([\d.]+)")
    d.cmo = _float(r"CMO\(14\).*?:\s*([+-]?[\d.]+)")

    # LaguerreRSI Dual-Gamma (#8) — format: "Fast (γ=0.3)=0.xxx ... | Slow (γ=0.6)=0.xxx ..."
    lrsi_fast_match = re.search(r"Fast \(γ=0\.3\)=([\d.]+)", text)
    lrsi_slow_match = re.search(r"Slow \(γ=0\.6\)=([\d.]+)", text)
    if lrsi_fast_match:
        d.laguerre_fast = float(lrsi_fast_match.group(1))
    if lrsi_slow_match:
        d.laguerre_slow = float(lrsi_slow_match.group(1))
    d.laguerre_divergence = d.laguerre_fast - d.laguerre_slow  # #8

    # ATR
    d.atr = _float(r"ATR\(14\)=([\d.]+)")
    d.atr_sl_long = _float(r"LONG\s*→\s*SL:\s*\$?([\d.]+)")
    d.atr_sl_short = _float(r"SHORT\s*→\s*SL:\s*\$?([\d.]+)")

    # Hurst info (#2, #7)
    d.hurst = HurstInfo(
        hurst_short=d.hurst_short if d.hurst_short > 0 else d.hurst_val,
        hurst_long=d.hurst_val,
    )
    d.hurst.compute_confidence()

    return d


def parse_signals(text: str) -> SignalResult:
    """Parse get_trading_signals output."""
    s = SignalResult()
    for line in text.split("\n"):
        if "🟢 **Buy**" in line:
            s.buy_count += 1
            m = re.search(r"\((\d+)%\)", line)
            if m:
                s.weighted_buy_score += int(m.group(1))
            # Extract indicator name
            m2 = re.search(r"\[(\w[\w()]+)\]", line)
            detail = {"side": "BUY", "indicator": m2.group(1) if m2 else "unknown"}
            m3 = re.search(r"\((\d+)%\)", line)
            detail["weight"] = int(m3.group(1)) if m3 else 0
            s.signals_detail.append(detail)
        elif "🔴 **Sell**" in line:
            s.sell_count += 1
            m = re.search(r"\((\d+)%\)", line)
            if m:
                s.weighted_sell_score += int(m.group(1))
            m2 = re.search(r"\[(\w[\w()]+)\]", line)
            detail = {"side": "SELL", "indicator": m2.group(1) if m2 else "unknown"}
            m3 = re.search(r"\((\d+)%\)", line)
            detail["weight"] = int(m3.group(1)) if m3 else 0
            s.signals_detail.append(detail)
        elif "⚪ **Neutral**" in line:
            s.neutral_count += 1
    return s


def parse_regime(text: str) -> str:
    """Parse detect_market_regime → simplified regime string."""
    if "Trending Up" in text or "TrendingUp" in text:
        return "TrendingUp"
    if "Trending Down" in text or "TrendingDown" in text:
        return "TrendingDown"
    if "Ranging" in text:
        return "Ranging"
    if "Volatile" in text:
        return "Volatile"
    if "Quiet" in text:
        return "Quiet"
    if "Trending" in text:
        return "Trending"
    return "Unknown"


def parse_support_resistance(text: str) -> SupportResistance:
    """Parse get_support_resistance output."""
    sr = SupportResistance()
    current_section = None
    for line in text.split("\n"):
        if "Resistance" in line:
            current_section = "resistance"
            continue
        if "Support" in line:
            current_section = "support"
            continue
        m = re.search(r"\$?([\d.eE+-]+)\s*\(([+-]?[\d.]+)%\)", line)
        if m:
            level = {"price": float(m.group(1)), "pct": float(m.group(2))}
            if current_section == "resistance":
                sr.resistances.append(level)
            elif current_section == "support":
                sr.supports.append(level)
    return sr


def parse_fear_greed(text: str) -> tuple[int, str]:
    """Parse Fear & Greed result → (value, label)."""
    m = re.search(r"(\d+)\s*[-–]\s*(\w+)", text)
    if m:
        return int(m.group(1)), m.group(2)
    m = re.search(r"(\d+)", text)
    if m:
        return int(m.group(1)), ""
    return 0, "Unknown"


def parse_sentiment(text: str) -> tuple[float, str]:
    """Parse composite sentiment → (score, label)."""
    m = re.search(r"Score:\s*([+-]?[\d.]+)\s*\((\w+)\)", text)
    if m:
        return float(m.group(1)), m.group(2)
    return 0.0, "Unknown"


# ─── Scoring Engine ────────────────────────────────────────────────────────────

class ScoringEngine:
    """
    #3: Signal Aggregation — weighted confluence scoring.
    Implements scoring from trading-process-improvement.md improvements.
    """

    # Weight allocation per signal source (sum = 100)
    WEIGHTS = {
        "Hurst(100)": 15,       # #2: regime detection
        "HurstDivergence": 10,  # #7: divergence
        "ALMA(10,30)": 15,      # trend direction
        "SuperSmoother(20)": 10, # trend slope
        "CMO(14)": 8,           # momentum
        "MACD(12,26,9)": 12,    # momentum confirmation
        "EMA Cross": 8,         # trend confirmation
        "RSI(14)": 7,           # overbought/oversold
        "BB(20,2)": 5,          # price position
        "LaguerreRSI": 10,      # #8: smooth momentum
    }

    @staticmethod
    def compute_composite_score(
        signals_1h: SignalResult,
        signals_15m: SignalResult,
        hurst: HurstInfo,
        indicators_1h: IndicatorData,
        indicators_15m: IndicatorData,
        market_ctx: MarketContext,
    ) -> tuple[float, str, float]:
        """
        Compute composite score 0-100 → (score, action, confidence).

        Returns: (composite_score, action, confidence)
        """
        # ── Step 1: Weighted signal score per timeframe ──
        def _tf_signal_score(signals: SignalResult) -> float:
            total_weight = signals.weighted_buy_score + signals.weighted_sell_score
            if total_weight == 0:
                return 50.0  # neutral
            # Map to 0-100: 0 = all sell, 100 = all buy, 50 = neutral
            buy_ratio = signals.weighted_buy_score / total_weight
            return buy_ratio * 100.0

        score_1h = _tf_signal_score(signals_1h) if signals_1h else 50.0
        score_15m = _tf_signal_score(signals_15m) if signals_15m else 50.0

        # ── Step 2: Multi-TF Confluence (#1) ──
        # If both TFs agree → boost. If disagree → reduce.
        if score_1h > 60 and score_15m > 60:
            confluence_bonus = 10.0
        elif score_1h < 40 and score_15m < 40:
            confluence_bonus = -10.0
        elif abs(score_1h - score_15m) > 30:
            confluence_bonus = -5.0  # disagree → uncertain
        else:
            confluence_bonus = 0.0

        # Weight: 1H = 60%, 15M = 40%
        raw_score = score_1h * 0.6 + score_15m * 0.4 + confluence_bonus

        # ── Step 3: Hurst Confidence Adjustment (#2, #7) ──
        hurst_conf = hurst.confidence_factor
        raw_score *= hurst_conf

        # Hurst regime boost
        if hurst.regime in ("Trending", "TrendingUp"):
            if raw_score > 55:
                raw_score += 5  # trending + bullish → stronger
            elif raw_score < 45:
                raw_score -= 5  # trending + bearish → stronger sell
        elif hurst.regime in ("Mean-Reverting", "Ranging"):
            # Mean-revert: extreme scores are good
            if raw_score > 70:
                raw_score -= 5  # overbought → sell signal
            elif raw_score < 30:
                raw_score += 5  # oversold → buy signal

        # ── Step 4: LaguerreRSI Divergence (#8) ──
        if indicators_15m:
            lrsi_div = indicators_15m.laguerre_divergence
            if lrsi_div < -0.3:
                # Momentum decelerating → reduce bullish
                raw_score -= 5
            elif lrsi_div > 0.3:
                # Momentum accelerating → boost
                raw_score += 5

        # ── Step 5: Sentiment Adjustment ──
        if market_ctx.is_fear and raw_score > 65:
            # Fear + bullish technical → contrarian caution
            raw_score -= 3
        elif market_ctx.is_greed and raw_score < 35:
            # Greed + bearish technical → contrarian boost
            raw_score += 3

        # ── Step 6: Clamp and determine action ──
        final_score = max(0.0, min(100.0, raw_score))

        if final_score >= 75:
            action = "STRONG_BUY"
        elif final_score >= 60:
            action = "BUY"
        elif final_score >= 45:
            action = "HOLD"
        elif final_score >= 30:
            action = "SELL"
        else:
            action = "STRONG_SELL"

        # Confidence: how far from neutral (50)
        confidence = abs(final_score - 50) / 50.0  # 0.0 - 1.0
        # Reduce confidence if Hurst divergent
        confidence *= hurst_conf

        return round(final_score, 1), action, round(confidence, 2)

    @staticmethod
    def recommend_strategy(
        hurst: HurstInfo,
        action: str,
        indicators_1h: IndicatorData,
        indicators_15m: IndicatorData,
    ) -> tuple[str, str]:
        """
        #10: Regime-appropriate strategy recommendation.
        Returns: (strategy_name, entry_type)
        """
        h = hurst.hurst_long

        if h > 0.55:
            # Trending regime
            if action in ("STRONG_BUY", "BUY"):
                if indicators_15m and indicators_15m.laguerre_fast < 0.5:
                    return "Trend-Follow LONG (ALMA+SuperSmoother)", "PULLBACK"
                return "Trend-Follow LONG (ALMA crossover)", "MOMENTUM"
            elif action in ("STRONG_SELL", "SELL"):
                if indicators_15m and indicators_15m.laguerre_fast > 0.5:
                    return "Trend-Follow SHORT (ALMA+SuperSmoother)", "PULLBACK"
                return "Trend-Follow SHORT (ALMA crossover)", "MOMENTUM"
            return "Trend-Follow (wait for signal)", "WAIT"

        elif h < 0.45:
            # Mean-Reverting regime
            if indicators_1h and indicators_1h.rsi > 70:
                return "Mean-Revert SHORT (RSI overbought)", "MOMENTUM"
            elif indicators_1h and indicators_1h.rsi < 30:
                return "Mean-Revert LONG (RSI oversold)", "MOMENTUM"
            elif indicators_1h and indicators_1h.bb_pct_b > 0.95:
                return "BB Bounce SHORT (upper band)", "MOMENTUM"
            elif indicators_1h and indicators_1h.bb_pct_b < 0.05:
                return "BB Bounce LONG (lower band)", "MOMENTUM"
            return "Mean-Revert (wait for extreme)", "WAIT"

        else:
            # Random Walk / uncertain
            return "CAUTION — Reduce size, wide stops", "WAIT"

    @staticmethod
    def compute_risk_params(
        coin: CoinAnalysis,
        hurst: HurstInfo,
        indicators_1h: IndicatorData,
        price: float,
    ) -> tuple[float, float, float, float]:
        """
        #5 + #6: Smart position sizing + ATR-based SL/TP.
        Returns: (sl, tp, rr_ratio, size_multiplier)
        """
        if not indicators_1h or price <= 0:
            return 0.0, 0.0, 0.0, 1.0

        atr = indicators_1h.atr
        if atr <= 0:
            return 0.0, 0.0, 0.0, 1.0

        # #6: ATR-based SL with regime multiplier
        h = hurst.hurst_long
        if h > 0.55:
            atr_mult = 2.0   # Trending: wider stops
        elif h < 0.45:
            atr_mult = 1.5   # Mean-reverting: tighter
        else:
            atr_mult = 2.5   # Random walk: widest

        # Hurst divergence → widen stops (#7)
        if hurst.hurst_divergence > 0.15:
            atr_mult *= 1.5

        if coin.action in ("STRONG_BUY", "BUY"):
            sl = price - (atr * atr_mult)
            tp_distance = atr * atr_mult * 1.5  # target 1.5:1 RR minimum
            tp = price + tp_distance
        elif coin.action in ("STRONG_SELL", "SELL"):
            sl = price + (atr * atr_mult)
            tp_distance = atr * atr_mult * 1.5
            tp = price - tp_distance
        else:
            sl = price - (atr * atr_mult)
            tp = price + (atr * atr_mult)
            return sl, tp, 0.0, 0.5  # HOLD → half size

        # Risk:Reward ratio
        risk = abs(price - sl)
        reward = abs(tp - price)
        rr_ratio = reward / risk if risk > 0 else 0.0

        # #5: Regime-conditional size multiplier
        if h > 0.55:
            size_mult = 1.0  # Trending: full size
        elif h < 0.45:
            size_mult = 0.5  # Mean-reverting: half size
        else:
            size_mult = 0.25  # Random walk: quarter size

        # Confidence adjustment
        size_mult *= coin.confidence

        return round(sl, 8), round(tp, 8), round(rr_ratio, 2), round(size_mult, 2)


# ─── Analysis Pipeline ─────────────────────────────────────────────────────────

def _fmt_price(price: float) -> str:
    """Format price with appropriate decimal places."""
    if price <= 0:
        return "0"
    if price >= 1000:
        return f"{price:,.2f}"
    if price >= 1:
        return f"{price:.4f}"
    if price >= 0.01:
        return f"{price:.5f}"
    if price >= 0.0001:
        return f"{price:.7f}"
    if price >= 0.00000001:
        return f"{price:.10f}"
    # Extremely small: use scientific but readable
    return f"{price:.6e}"


class AnalysisPipeline:
    """5-step analysis pipeline from trading-analysis-process.md."""

    def __init__(self, mcp: MCPClient, top_n: int = DEFAULT_TOP, quick: bool = False):
        self.mcp = mcp
        self.top_n = top_n
        self.quick = quick
        self.market_ctx = MarketContext()
        self.coins: list[CoinAnalysis] = []
        self.engine = ScoringEngine()

    def run(self) -> list[CoinAnalysis]:
        """Execute full 5-step pipeline."""
        print_header("BONBO TOP-100 COIN ANALYZER")
        print(f"  Top N: {self.top_n} | Quick mode: {self.quick}")
        print(f"  Time: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}")
        print()

        # Step 1: Scan thị trường rộng
        print_step(1, "QUÉT THỊ TRƯỜNG RỘNG (Dynamic Scanning #4)")
        self._step1_scan()
        print(f"  → Tìm thấy {len(self.coins)} coins đủ điều kiện\n")

        if not self.coins:
            print("  ⚠️ Không tìm thấy coin nào. Thoát.")
            return []

        # Step 2: Sentiment thị trường
        print_step(2, "SENTIMENT THỊ TRƯỜNG")
        self._step2_sentiment()
        print(f"  → Fear&Greed: {self.market_ctx.fear_greed_value} ({self.market_ctx.fear_greed_label})")
        print(f"  → Composite: {self.market_ctx.composite_sentiment} ({self.market_ctx.composite_label})\n")

        # Step 3: Deep Analysis (top N coins)
        if self.quick:
            detail_count = min(10, len(self.coins))
        else:
            detail_count = min(DEEP_ANALYSIS_TOP_N, len(self.coins))

        print_step(3, f"DEEP ANALYSIS — Top {detail_count} coins (Multi-TF #1, Hurst #2, LaguerreRSI #8)")
        self._step3_deep_analysis(detail_count)
        print()

        # Step 4: Scoring & Risk
        print_step(4, "SCORING & RISK (Aggregation #3, ATR-SL #6, Position Sizing #5)")
        self._step4_scoring()
        print()

        # Step 5: Sort & Report
        print_step(5, "XẾP HẠNG & BÁO CÁO")
        self._step5_report()

        return self.coins

    # ── Step 1: Scan ──

    def _step1_scan(self):
        """#4: Dynamic Scanning — 3-tier merge."""
        # Tier 1: Top coins by volume
        print("  [Tier 1] Lấy top coins theo volume...")
        result = self.mcp.call_tool("get_top_crypto", {"limit": min(self.top_n, 100)})
        top_coins = parse_top_crypto(result.get("text", ""))
        print(f"           → {len(top_coins)} coins từ get_top_crypto")

        # Tier 2: Hot movers
        print("  [Tier 2] Quét hot movers...")
        result = self.mcp.call_tool("scan_hot_movers", {"top_n": 30})
        hot_coins = parse_scan_hot_movers(result.get("text", ""))
        print(f"           → {len(hot_coins)} hot movers")

        # Tier 3: Merge & deduplicate
        seen_symbols = set()
        all_coins = []

        for c in top_coins:
            sym = c["symbol"]
            if sym not in seen_symbols:
                seen_symbols.add(sym)
                coin = CoinAnalysis(
                    symbol=sym,
                    price=c.get("price", 0),
                    change_24h=c.get("change_24h", 0),
                    volume_24h=c.get("volume_usd", 0),
                )
                all_coins.append(coin)

        # Merge hot movers (add unique symbols)
        for c in hot_coins:
            sym = c["symbol"]
            if sym not in seen_symbols:
                seen_symbols.add(sym)
                coin = CoinAnalysis(
                    symbol=sym,
                    price=c.get("price", 0),
                    scan_score=c.get("scan_score", 0),
                    scan_regime=c.get("volatility", ""),
                )
                # Hot movers already have scores
                coin.composite_score = c.get("scan_score", 0)
                all_coins.append(coin)
            else:
                # Update scan score for existing coins
                for existing in all_coins:
                    if existing.symbol == sym and c.get("scan_score", 0) > existing.scan_score:
                        existing.scan_score = c["scan_score"]
                        existing.scan_regime = c.get("volatility", "")

        # Filter: minimum volume
        self.coins = [
            c for c in all_coins
            if c.volume_24h >= MIN_VOLUME_USD or c.scan_score >= 60
        ]

        # Sort by scan_score desc, then change_24h desc
        self.coins.sort(key=lambda x: (x.scan_score, x.change_24h), reverse=True)

    # ── Step 2: Sentiment ──

    def _step2_sentiment(self):
        result = self.mcp.call_tool("get_fear_greed_index", {"history": 1})
        val, label = parse_fear_greed(result.get("text", ""))
        self.market_ctx.fear_greed_value = val
        self.market_ctx.fear_greed_label = label

        result = self.mcp.call_tool("get_composite_sentiment", {"symbol": "BTCUSDT"})
        score, label = parse_sentiment(result.get("text", ""))
        self.market_ctx.composite_sentiment = score
        self.market_ctx.composite_label = label

    # ── Step 3: Deep Analysis ──

    def _step3_deep_analysis(self, count: int):
        """Multi-TF analysis for top coins (#1, #2, #8)."""
        targets = self.coins[:count]
        total = len(targets)

        for i, coin in enumerate(targets):
            pct = (i + 1) / total * 100
            sys.stdout.write(f"\r  [{i+1}/{total}] {coin.symbol}... ")
            sys.stdout.flush()

            try:
                # Multi-TF: 1H + 15M (#1)
                results = self.mcp.call_tool_batch([
                    ("analyze_indicators", {"symbol": coin.symbol, "interval": "1h"}),
                    ("get_trading_signals", {"symbol": coin.symbol, "interval": "1h"}),
                    ("detect_market_regime", {"symbol": coin.symbol, "interval": "1h"}),
                    ("get_support_resistance", {"symbol": coin.symbol, "interval": "1h"}),
                    ("analyze_indicators", {"symbol": coin.symbol, "interval": "15m"}),
                    ("get_trading_signals", {"symbol": coin.symbol, "interval": "15m"}),
                ])

                # Parse 1H
                coin.indicators_1h = parse_indicators(results[0].get("text", ""))
                coin.signals_1h = parse_signals(results[1].get("text", ""))
                coin.regime_1h = parse_regime(results[2].get("text", ""))
                coin.sr = parse_support_resistance(results[3].get("text", ""))

                # Parse 15M
                coin.indicators_15m = parse_indicators(results[4].get("text", ""))
                coin.signals_15m = parse_signals(results[5].get("text", ""))

                # Get exact price if not set
                if coin.price <= 0:
                    price_result = self.mcp.call_tool("get_crypto_price", {"symbol": coin.symbol})
                    price_data = parse_price(price_result.get("text", ""))
                    coin.price = price_data.get("price", 0)
                    coin.change_24h = price_data.get("change_24h", coin.change_24h)
                    coin.volume_24h = price_data.get("volume_usd", coin.volume_24h)

                # Warnings
                self._generate_warnings(coin)

            except Exception as e:
                print(f"\n  ⚠️ Lỗi phân tích {coin.symbol}: {e}")
                continue

        print(" ✓")

    def _generate_warnings(self, coin: CoinAnalysis):
        """Generate risk warnings."""
        if coin.indicators_1h:
            ind = coin.indicators_1h
            if ind.rsi > 75:
                coin.warnings.append("RSI quá mua (>75)")
            if ind.rsi < 25:
                coin.warnings.append("RSI quá bán (<25)")
            if ind.laguerre_slow > 0.95 and ind.laguerre_fast > 0.95:
                coin.warnings.append("LaguerreRSI kép quá mua")
            if ind.hurst.hurst_divergence > 0.15:
                coin.warnings.append(f"Hurst divergence {ind.hurst.hurst_divergence:.2f} → regime chuyển đổi")
            if ind.bb_pct_b > 0.98:
                coin.warnings.append("BB %B > 0.98 — giá chạm upper band")
            if ind.bb_pct_b < 0.02:
                coin.warnings.append("BB %B < 0.02 — giá chạm lower band")
            if ind.cmo > 50:
                coin.warnings.append(f"CMO={ind.cmo:.0f} cực kỳ quá mua")

        # Multi-TF conflict warning (#1)
        if (coin.signals_1h and coin.signals_15m and
                coin.signals_1h.weighted_buy_score > 100 and
                coin.signals_15m.weighted_sell_score > 100):
            coin.warnings.append("⚠ Xung đột 1H↑ vs 15M↓ — chờ xác nhận")

    # ── Step 4: Scoring ──

    def _step4_scoring(self):
        for coin in self.coins:
            if not coin.indicators_1h or not coin.signals_1h:
                # No deep analysis → use scan score
                if coin.scan_score > 0:
                    coin.composite_score = float(coin.scan_score)
                    coin.action = "HOLD"
                    if coin.composite_score >= 70:
                        coin.action = "BUY"
                    elif coin.composite_score < 30:
                        coin.action = "SELL"
                continue

            # #3: Composite scoring
            hurst = coin.indicators_1h.hurst
            score, action, confidence = ScoringEngine.compute_composite_score(
                signals_1h=coin.signals_1h,
                signals_15m=coin.signals_15m or SignalResult(),
                hurst=hurst,
                indicators_1h=coin.indicators_1h,
                indicators_15m=coin.indicators_15m or IndicatorData(),
                market_ctx=self.market_ctx,
            )
            coin.composite_score = score
            coin.action = action
            coin.confidence = confidence

            # #10: Strategy recommendation
            strategy, entry = ScoringEngine.recommend_strategy(
                hurst=hurst,
                action=action,
                indicators_1h=coin.indicators_1h,
                indicators_15m=coin.indicators_15m or IndicatorData(),
            )
            coin.recommended_strategy = strategy
            coin.entry_type = entry

            # #5 + #6: Risk params
            sl, tp, rr, size_mult = ScoringEngine.compute_risk_params(
                coin=coin,
                hurst=hurst,
                indicators_1h=coin.indicators_1h,
                price=coin.price,
            )
            coin.recommended_sl = sl
            coin.recommended_tp = tp
            coin.risk_reward_ratio = rr
            coin.regime_size_multiplier = size_mult

        # Sort by composite score desc
        self.coins.sort(key=lambda x: x.composite_score, reverse=True)

    # ── Step 5: Report ──

    def _step5_report(self):
        """Generate and print final report."""
        report = self._build_report()
        print(report)

        # Save to file
        timestamp = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
        filename = f"report_top100_{timestamp}.md"
        with open(filename, "w", encoding="utf-8") as f:
            f.write(report)
        print(f"\n  📁 Báo cáo đã lưu: {filename}")

    def _build_report(self) -> str:
        """Build markdown report."""
        lines = []
        ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

        lines.append("# 📊 BÁO CÁO PHÂN TÍCH TOP COIN")
        lines.append(f"\n**Thời gian:** {ts}")
        lines.append(f"**Công cụ:** BonBo Extend MCP v0.2.0")
        lines.append(f"**Phân tích:** {len(self.coins)} coins")
        lines.append(f"**Thuật toán:** 10 cải thiện từ trading-process-improvement.md")
        lines.append("")

        # Market Context
        lines.append("## 1. BỐI CẢNH THỊ TRƯỜNG")
        lines.append("")
        fg_emoji = "😱" if self.market_ctx.fear_greed_value < 25 else "😟" if self.market_ctx.fear_greed_value < 40 else "😊" if self.market_ctx.fear_greed_value < 70 else "🤑"
        lines.append(f"| Chỉ số | Giá trị | Nhận định |")
        lines.append(f"|--------|---------|-----------|")
        lines.append(f"| Fear & Greed | {self.market_ctx.fear_greed_value} | {fg_emoji} {self.market_ctx.fear_greed_label} |")
        lines.append(f"| Composite Sentiment | {self.market_ctx.composite_sentiment:.2f} | {self.market_ctx.composite_label} |")

        if self.market_ctx.is_contrarian_buy_zone:
            lines.append(f"\n> 💡 **Extreme Fear zone** — đây là vùng contrarian buy zone historically. "
                         f"Cơ hội mua nhưng cần SL chặt.")
        elif self.market_ctx.is_fear:
            lines.append(f"\n> ⚠️ **Fear zone** — thận trọng, ưu tiên SL chặt và position size nhỏ.")
        elif self.market_ctx.is_greed:
            lines.append(f"\n> 🔴 **Greed zone** — thận trọng cao, có thể counter-trend SHORT.")
        lines.append("")

        # Top Coins Table
        lines.append("## 2. BẢNG XẾP HẠNG")
        lines.append("")
        lines.append("| # | Symbol | Giá | 24h% | Score | Action | Hurst | Regime | Strategy | R:R |")
        lines.append("|---|--------|-----|------|-------|--------|-------|---------|----------|-----|")

        buy_coins = []
        sell_coins = []
        hold_coins = []

        for i, coin in enumerate(self.coins):
            hurst_str = ""
            if coin.indicators_1h and coin.indicators_1h.hurst_val > 0:
                hurst_str = f"H={coin.indicators_1h.hurst_val:.2f}"

            regime_str = coin.regime_1h or coin.scan_regime or "-"
            strategy_str = coin.recommended_strategy or "-"
            if len(strategy_str) > 30:
                strategy_str = strategy_str[:28] + ".."

            rr_str = f"{coin.risk_reward_ratio:.1f}" if coin.risk_reward_ratio > 0 else "-"

            action_emoji = {"STRONG_BUY": "🟢🟢", "BUY": "🟢", "HOLD": "⚪", "SELL": "🔴", "STRONG_SELL": "🔴🔴"}.get(coin.action, "⚪")

            lines.append(
                f"| {i+1} | **{coin.symbol}** | ${_fmt_price(coin.price)} | "
                f"{coin.change_24h:+.1f}% | {coin.composite_score:.0f} | "
                f"{action_emoji} {coin.action} | {hurst_str} | {regime_str} | "
                f"{strategy_str} | {rr_str} |"
            )

            if coin.action in ("STRONG_BUY", "BUY"):
                buy_coins.append(coin)
            elif coin.action in ("STRONG_SELL", "SELL"):
                sell_coins.append(coin)
            else:
                hold_coins.append(coin)

        lines.append("")

        # Detailed Analysis for top picks
        detailed = [c for c in self.coins if c.indicators_1h][:DEEP_ANALYSIS_TOP_N]
        if detailed:
            lines.append("## 3. PHÂN TÍCH CHI TIẾT")
            lines.append("")

            for coin in detailed[:10]:
                lines.append(f"### {'🟢' if 'BUY' in coin.action else '🔴' if 'SELL' in coin.action else '⚪'} {coin.symbol} — {coin.action} (Score: {coin.composite_score:.0f})")
                lines.append(f"**Giá:** ${_fmt_price(coin.price)} | **24h:** {coin.change_24h:+.1f}%")
                lines.append("")

                # Multi-TF Summary (#1)
                lines.append("**Multi-Timeframe (#1):**")
                if coin.signals_1h:
                    lines.append(f"- 1H: {coin.signals_1h.buy_count} Buy / {coin.signals_1h.sell_count} Sell "
                                 f"(weighted buy={coin.signals_1h.weighted_buy_score}, sell={coin.signals_1h.weighted_sell_score})")
                if coin.signals_15m:
                    lines.append(f"- 15M: {coin.signals_15m.buy_count} Buy / {coin.signals_15m.sell_count} Sell "
                                 f"(weighted buy={coin.signals_15m.weighted_buy_score}, sell={coin.signals_15m.weighted_sell_score})")
                lines.append("")

                # Hurst & Regime (#2, #7)
                if coin.indicators_1h:
                    ind = coin.indicators_1h
                    lines.append(f"**Hurst Regime (#2, #7):**")
                    lines.append(f"- H(100)={ind.hurst_val:.3f} | H(50)={ind.hurst_short:.3f} | "
                                 f"Divergence={ind.hurst.hurst_divergence:.3f} | Confidence={ind.hurst.confidence_factor:.0%}")
                    lines.append(f"- Regime: {coin.regime_1h}")
                    lines.append("")

                    # Key Indicators
                    lines.append(f"**Chỉ báo chính:**")
                    lines.append(f"- RSI={ind.rsi:.1f} | MACD hist={ind.macd_hist:.4f} | BB %B={ind.bb_pct_b:.2f}")
                    lines.append(f"- ALMA cross={ind.alma_cross_pct:+.1f}% | SuperSmoother slope={ind.supersmoother_slope:+.2f}%")
                    lines.append(f"- CMO={ind.cmo:.1f} | ATR={ind.atr:.4f}")
                    lines.append("")

                    # LaguerreRSI (#8)
                    lines.append(f"**LaguerreRSI Dual-Gamma (#8):**")
                    lines.append(f"- Fast (γ=0.3)={ind.laguerre_fast:.3f} | Slow (γ=0.6)={ind.laguerre_slow:.3f} | "
                                 f"Divergence={ind.laguerre_divergence:+.3f}")
                    if ind.laguerre_divergence < -0.3:
                        lines.append(f"- ⚠️ Momentum đang giảm tốc")
                    elif ind.laguerre_divergence > 0.3:
                        lines.append(f"- ✅ Momentum đang tăng tốc")
                    lines.append("")

                # Risk Parameters (#5, #6)
                if coin.recommended_sl > 0:
                    lines.append(f"**Quản lý rủi ro (#5, #6):**")
                    lines.append(f"- Entry: ${_fmt_price(coin.price)}")
                    lines.append(f"- Stop Loss: ${_fmt_price(coin.recommended_sl)} (ATR-based, regime-adjusted)")
                    lines.append(f"- Take Profit: ${_fmt_price(coin.recommended_tp)}")
                    lines.append(f"- R:R = 1:{coin.risk_reward_ratio:.1f}")
                    lines.append(f"- Position size multiplier: {coin.regime_size_multiplier:.0%}")
                    lines.append("")

                # Strategy (#10)
                if coin.recommended_strategy:
                    lines.append(f"**Chiến lược (#10):** {coin.recommended_strategy}")
                    lines.append(f"- Entry type: {coin.entry_type}")
                    lines.append("")

                # Warnings
                if coin.warnings:
                    lines.append(f"**⚠️ Cảnh báo:**")
                    for w in coin.warnings:
                        lines.append(f"- {w}")
                    lines.append("")

                lines.append("---")
                lines.append("")

        # Summary
        lines.append("## 4. TÓM TẮT")
        lines.append("")
        lines.append(f"| Nhóm | Số lượng | Coins |")
        lines.append(f"|------|----------|-------|")
        lines.append(f"| 🟢 BUY | {len(buy_coins)} | {', '.join(c.symbol for c in buy_coins[:10])} |")
        lines.append(f"| ⚪ HOLD | {len(hold_coins)} | {', '.join(c.symbol for c in hold_coins[:10])} |")
        lines.append(f"| 🔴 SELL | {len(sell_coins)} | {', '.join(c.symbol for c in sell_coins[:10])} |")
        lines.append("")

        # Top 3 picks
        top3 = [c for c in self.coins if c.action in ("STRONG_BUY", "BUY") and c.confidence > 0.3][:3]
        if top3:
            lines.append("### 🏆 Top 3 Khuyến nghị")
            lines.append("")
            for i, coin in enumerate(top3):
                lines.append(f"**#{i+1} {coin.symbol}** — Score: {coin.composite_score:.0f} | "
                             f"Strategy: {coin.recommended_strategy}")
                if coin.recommended_sl > 0:
                    lines.append(f"- Entry: ${_fmt_price(coin.price)} | SL: ${_fmt_price(coin.recommended_sl)} | "
                                 f"TP: ${_fmt_price(coin.recommended_tp)} | R:R = 1:{coin.risk_reward_ratio:.1f}")
                lines.append("")

        # Disclaimer
        lines.append("---")
        lines.append("")
        lines.append("*⚠️ DISCLAIMER: Phân tích mang tính tham khảo, không phải lời khuyên đầu tư. "
                      "Luôn tự nghiên cứu và quản lý rủi ro cẩn thận.*")
        lines.append("")
        lines.append(f"*Generated by BonBo Top-100 Analyzer — {ts}*")

        return "\n".join(lines)


# ─── Utilities ─────────────────────────────────────────────────────────────────

def print_header(text: str):
    width = 60
    print("═" * width)
    print(f"  {text}")
    print("═" * width)


def print_step(step: int, text: str):
    print(f"┌─ Step {step}: {text}")
    print(f"│")


# ─── Main ──────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="BonBo Top-100 Coin Analyzer")
    parser.add_argument("--top", type=int, default=DEFAULT_TOP, help="Number of top coins to analyze")
    parser.add_argument("--quick", action="store_true", help="Quick mode — skip deep analysis for most coins")
    parser.add_argument("--output", type=str, default=None, help="Output file path")
    parser.add_argument("--binary", type=str, default=MCP_BINARY, help="Path to bonbo-extend-mcp binary")
    args = parser.parse_args()

    if not os.path.exists(args.binary):
        print(f"❌ Không tìm thấy MCP binary: {args.binary}")
        print(f"   Build trước: cd ~/BonBoExtend && cargo build --release")
        sys.exit(1)

    mcp = MCPClient(args.binary)

    try:
        print("🔌 Khởi động MCP server...")
        mcp.start()
        print("   ✓ MCP server đã sẵn sàng\n")

        pipeline = AnalysisPipeline(mcp, top_n=args.top, quick=args.quick)
        pipeline.run()

    except KeyboardInterrupt:
        print("\n\n⚠️ Đã dừng bởi user.")
    except Exception as e:
        print(f"\n❌ Lỗi: {e}")
        import traceback
        traceback.print_exc()
    finally:
        print("\n🔌 Đóng MCP server...")
        mcp.stop()
        print("   ✓ Done.")


if __name__ == "__main__":
    main()
