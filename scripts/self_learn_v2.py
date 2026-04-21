#!/usr/bin/env python3
"""
BonBo Self-Learning Loop v2 — Autonomous AI Trading Learning Cycle

Full pipeline:
  1. SCAN:    Quét top 100 crypto → phân tích → chấm điểm
  2. ANALYZE: Phân tích kỹ thuật chi tiết + multi-timeframe
  3. BACKTEST: Validate signals với historical backtesting
  4. JOURNAL: Ghi nhận vào trade journal (SQLite)
  5. REVIEW:  Đánh giá past predictions vs actual outcomes
  6. LEARN:   Tune weights bằng DMA (Dynamic Model Averaging)

Usage:
  python3 self_learn_v2.py [--top-n 100] [--once] [--verbose]
"""

import json
import sys
import time
import argparse
import urllib.request
import sqlite3
import math
import os
from datetime import datetime
from pathlib import Path

# ── Config ──────────────────────────────────────────────────────────
DATA_DIR = Path.home() / ".bonbo" / "self_learning"
DB_PATH = DATA_DIR / "journal.db"
LOG_FILE = DATA_DIR / "learning_log.txt"
DATA_DIR.mkdir(parents=True, exist_ok=True)

# ── Weights (DMA-adapted) ──────────────────────────────────────────
# These weights get tuned by the learning loop
DEFAULT_WEIGHTS = {
    "rsi": 0.20,
    "macd": 0.15,
    "bollinger": 0.12,
    "sma_cross": 0.10,
    "volume": 0.08,
    "momentum": 0.10,
    "mean_reversion": 0.10,
    "backtest": 0.15,
}

SKIP_SYMBOLS = [
    "USDC", "FDUSD", "USD1", "RLUSD", "PAXG", "XAUT", "USDE",
    "XUSD", "EUR", "CUSD", "DAI", "TUSD", "BUSD", "USDP",
]


# ── Database Setup ─────────────────────────────────────────────────
def init_db():
    conn = sqlite3.connect(str(DB_PATH))
    conn.execute("""
        CREATE TABLE IF NOT EXISTS journal (
            id TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            symbol TEXT NOT NULL,
            score REAL NOT NULL,
            recommendation TEXT NOT NULL,
            price REAL NOT NULL,
            stop_loss REAL,
            take_profit REAL,
            rsi REAL,
            macd_signal TEXT,
            bb_pct REAL,
            volume_ratio REAL,
            regime TEXT,
            backtest_sharpe REAL,
            snapshot_json TEXT NOT NULL,
            outcome_json TEXT,
            verified INTEGER DEFAULT 0
        )
    """)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_symbol ON journal(symbol)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_timestamp ON journal(timestamp)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_rec ON journal(recommendation)")
    conn.execute("""
        CREATE TABLE IF NOT EXISTS weights_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            weights_json TEXT NOT NULL,
            trigger TEXT NOT NULL
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS learning_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            total_predictions INTEGER,
            correct_predictions INTEGER,
            accuracy REAL,
            avg_sharpe REAL,
            best_strategy TEXT,
            notes TEXT
        )
    """)
    conn.commit()
    return conn


# ── Market Data Fetching ───────────────────────────────────────────
def fetch_json(url, timeout=15):
    req = urllib.request.Request(url, headers={"User-Agent": "BonBoExtend/2.0"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def fetch_top_symbols(top_n=100):
    """Fetch top USDT pairs by quote volume, excluding stablecoins."""
    data = fetch_json("https://api.binance.com/api/v3/ticker/24hr")
    usdt = [
        d for d in data
        if d["symbol"].endswith("USDT")
        and float(d["quoteVolume"]) > 5_000_000
        and not any(d["symbol"].startswith(s) for s in SKIP_SYMBOLS)
    ]
    usdt.sort(key=lambda x: float(x["quoteVolume"]), reverse=True)
    return [d["symbol"] for d in usdt[:top_n]]


def fetch_klines(symbol, interval="1d", limit=200):
    url = f"https://api.binance.com/api/v3/klines?symbol={symbol}&interval={interval}&limit={limit}"
    return fetch_json(url)


# ── Technical Analysis Engine ──────────────────────────────────────
def calc_ema(data, period):
    if len(data) < period:
        return None
    k = 2 / (period + 1)
    e = sum(data[:period]) / period
    for p in data[period:]:
        e = p * k + e * (1 - k)
    return e


def calc_sma(data, period):
    if len(data) < period:
        return None
    return sum(data[-period:]) / period


def calc_rsi(data, period=14):
    if len(data) < period + 1:
        return None
    gains, losses = [], []
    for i in range(1, len(data)):
        d = data[i] - data[i - 1]
        gains.append(max(d, 0))
        losses.append(max(-d, 0))
    avg_gain = sum(gains[-period:]) / period
    avg_loss = sum(losses[-period:]) / period
    if avg_loss == 0:
        return 100
    rs = avg_gain / avg_loss
    return 100 - (100 / (1 + rs))


def calc_macd(closes, fast=12, slow=26, signal=9):
    ema_fast = calc_ema(closes, fast)
    ema_slow = calc_ema(closes, slow)
    if ema_fast is None or ema_slow is None:
        return None, None
    macd_line = ema_fast - ema_slow
    # Simplified signal line
    if len(closes) >= slow + signal:
        macd_values = []
        for i in range(slow, len(closes)):
            ef = calc_ema(closes[: i + 1], fast)
            es = calc_ema(closes[: i + 1], slow)
            if ef and es:
                macd_values.append(ef - es)
        if len(macd_values) >= signal:
            signal_line = sum(macd_values[-signal:]) / signal
            return macd_line, signal_line
    return macd_line, None


def calc_bollinger(closes, period=20, std_mult=2.0):
    sma = calc_sma(closes, period)
    if sma is None:
        return None, None, None, None
    std = (sum((c - sma) ** 2 for c in closes[-period:]) / period) ** 0.5
    upper = sma + std_mult * std
    lower = sma - std_mult * std
    bb_range = upper - lower
    pct = (closes[-1] - lower) / bb_range * 100 if bb_range > 0 else 50
    return upper, sma, lower, pct


def calc_atr(highs, lows, closes, period=14):
    if len(highs) < period + 1:
        return None
    trs = []
    for i in range(-period, 0):
        tr = max(
            highs[i] - lows[i],
            abs(highs[i] - closes[i - 1]),
            abs(lows[i] - closes[i - 1]),
        )
        trs.append(tr)
    return sum(trs) / period


def detect_regime(closes, sma20=None, sma50=None):
    """Simple regime detection from price action."""
    if len(closes) < 50:
        return "Unknown"
    # Volatility
    returns = [closes[i] - closes[i - 1] for i in range(-20, 0)]
    std = (sum((r - sum(returns) / len(returns)) ** 2 for r in returns) / len(returns)) ** 0.5
    avg_price = sum(closes[-20:]) / 20
    vol_pct = std / avg_price * 100 if avg_price > 0 else 0

    # Trend
    sma20_val = sma20 or calc_sma(closes, 20)
    sma50_val = sma50 or calc_sma(closes, 50)

    if sma20_val and sma50_val:
        if sma20_val > sma50_val * 1.02:
            trend = "TrendingUp"
        elif sma20_val < sma50_val * 0.98:
            trend = "TrendingDown"
        else:
            trend = "Ranging"
    else:
        trend = "Ranging"

    if vol_pct > 3.5:
        return "Volatile"
    return trend


def full_analysis(klines, symbol):
    """Complete technical analysis on kline data."""
    if len(klines) < 50:
        return None

    closes = [float(k[4]) for k in klines]
    highs = [float(k[2]) for k in klines]
    lows = [float(k[3]) for k in klines]
    volumes = [float(k[5]) for k in klines]
    n = len(closes)
    price = closes[-1]

    # Indicators
    rsi = calc_rsi(closes)
    sma20 = calc_sma(closes, 20)
    sma50 = calc_sma(closes, 50)
    macd_line, signal_line = calc_macd(closes)
    bb_upper, bb_mid, bb_lower, bb_pct = calc_bollinger(closes)
    atr = calc_atr(highs, lows, closes)

    # Volume
    vol_ma20 = sum(volumes[-20:]) / 20
    vol_ma5 = sum(volumes[-5:]) / 5
    vol_ratio = vol_ma5 / vol_ma20 if vol_ma20 > 0 else 1.0

    # Price changes
    chg_3d = (closes[-1] / closes[-4] - 1) * 100 if n >= 4 else 0
    chg_7d = (closes[-1] / closes[-8] - 1) * 100 if n >= 8 else 0
    chg_30d = (closes[-1] / closes[-31] - 1) * 100 if n >= 31 else 0

    # From high/low
    high_30d = max(highs[-30:]) if n >= 30 else max(highs)
    low_30d = min(lows[-30:]) if n >= 30 else min(lows)
    from_high = (price / high_30d - 1) * 100
    from_low = (price / low_30d - 1) * 100

    # Regime
    regime = detect_regime(closes, sma20, sma50)

    return {
        "symbol": symbol,
        "price": price,
        "rsi": rsi,
        "sma20": sma20,
        "sma50": sma50,
        "macd_line": macd_line,
        "signal_line": signal_line,
        "macd_bullish": macd_line is not None and macd_line > 0,
        "bb_upper": bb_upper,
        "bb_lower": bb_lower,
        "bb_pct": bb_pct,
        "atr": atr,
        "atr_pct": (atr / price * 100) if atr and price else 0,
        "vol_ratio": vol_ratio,
        "chg_3d": chg_3d,
        "chg_7d": chg_7d,
        "chg_30d": chg_30d,
        "from_high": from_high,
        "from_low": from_low,
        "regime": regime,
        "n_candles": n,
    }


# ── Scoring Engine ──────────────────────────────────────────────────
def score_analysis(analysis, weights=None):
    """Score an analysis result using weighted indicators."""
    if weights is None:
        weights = DEFAULT_WEIGHTS

    score = 50.0  # neutral baseline
    signals = []

    # RSI signal (weight: rsi)
    rsi = analysis["rsi"]
    if rsi is not None:
        if rsi < 25:
            score += weights["rsi"] * 100 * 1.5
            signals.append(f"RSI deeply oversold ({rsi:.0f})")
        elif rsi < 35:
            score += weights["rsi"] * 80
            signals.append(f"RSI oversold ({rsi:.0f})")
        elif rsi < 45:
            score += weights["rsi"] * 30
        elif rsi > 75:
            score -= weights["rsi"] * 100 * 1.5
            signals.append(f"RSI overbought ({rsi:.0f})")
        elif rsi > 65:
            score -= weights["rsi"] * 50

    # MACD (weight: macd)
    if analysis["macd_bullish"]:
        score += weights["macd"] * 80
        signals.append("MACD bullish")
    else:
        score -= weights["macd"] * 40

    # Bollinger Bands (weight: bollinger)
    bb_pct = analysis["bb_pct"]
    if bb_pct is not None:
        if bb_pct < 10:
            score += weights["bollinger"] * 100
            signals.append("BB lower band (bounce zone)")
        elif bb_pct < 25:
            score += weights["bollinger"] * 60
        elif bb_pct > 90:
            score -= weights["bollinger"] * 80
            signals.append("BB upper band (resistance)")
        elif bb_pct > 75:
            score -= weights["bollinger"] * 30

    # SMA crossover (weight: sma_cross)
    if analysis["sma20"] and analysis["sma50"]:
        if analysis["sma20"] > analysis["sma50"]:
            score += weights["sma_cross"] * 70
        else:
            score -= weights["sma_cross"] * 50

    # Volume (weight: volume)
    if analysis["vol_ratio"] > 2.0:
        score += weights["volume"] * 80
        signals.append(f"Volume surge ({analysis['vol_ratio']:.1f}x)")
    elif analysis["vol_ratio"] > 1.3:
        score += weights["volume"] * 40

    # Momentum (weight: momentum)
    chg_7d = analysis["chg_7d"]
    if chg_7d < -20:
        score += weights["momentum"] * 100
        signals.append(f"Oversold 7d ({chg_7d:.0f}%)")
    elif chg_7d < -10:
        score += weights["momentum"] * 60
    elif chg_7d > 30:
        score -= weights["momentum"] * 80
        signals.append(f"Overextended 7d (+{chg_7d:.0f}%)")
    elif chg_7d > 20:
        score -= weights["momentum"] * 40

    # Mean reversion (weight: mean_reversion)
    from_high = analysis["from_high"]
    if from_high < -25:
        score += weights["mean_reversion"] * 100
        signals.append(f"Deep correction ({from_high:.0f}%)")
    elif from_high < -15:
        score += weights["mean_reversion"] * 60
    elif from_high > 5:
        score -= weights["mean_reversion"] * 50

    # Clamp
    score = max(0, min(100, score))

    # Recommendation
    if score >= 70:
        rec = "STRONG_BUY"
    elif score >= 58:
        rec = "BUY"
    elif score >= 42:
        rec = "HOLD"
    elif score >= 30:
        rec = "SELL"
    else:
        rec = "STRONG_SELL"

    return score, rec, signals


# ── Simple Backtest on Recent Data ─────────────────────────────────
def quick_backtest(klines, analysis, window=30):
    """Simulate how this signal would have performed in the past window."""
    closes = [float(k[4]) for k in klines]
    if len(closes) < window + 14:
        return None

    # Use the last `window` bars as hypothetical entry points
    results = []
    for offset in range(14, min(window, len(closes) - 5)):
        entry = closes[-(offset)]
        # Check RSI at that point
        historical_closes = closes[: -(offset)] if offset > 0 else closes
        if len(historical_closes) < 15:
            continue
        rsi_then = calc_rsi(historical_closes)
        if rsi_then is None:
            continue

        # If RSI was oversold (< 35) → simulate buy
        if rsi_then < 35:
            exit_price = closes[-(offset - 5)] if offset > 5 else closes[-1]
            pnl_pct = (exit_price / entry - 1) * 100
            results.append(pnl_pct)

    if not results:
        return None

    avg_pnl = sum(results) / len(results)
    win_rate = len([r for r in results if r > 0]) / len(results)
    sharpe = 0
    if len(results) > 1:
        std = (sum((r - avg_pnl) ** 2 for r in results) / (len(results) - 1)) ** 0.5
        sharpe = (avg_pnl / std * (252 ** 0.5)) if std > 0 else 0

    return {"avg_pnl": avg_pnl, "win_rate": win_rate, "sharpe": sharpe, "trades": len(results)}


# ── DMA Weight Update ──────────────────────────────────────────────
def load_weights():
    """Load current weights from DB, or use defaults."""
    try:
        conn = sqlite3.connect(str(DB_PATH))
        row = conn.execute(
            "SELECT weights_json FROM weights_history ORDER BY timestamp DESC LIMIT 1"
        ).fetchone()
        conn.close()
        if row:
            return json.loads(row[0])
    except Exception:
        pass
    return DEFAULT_WEIGHTS.copy()


def save_weights(weights, trigger="auto"):
    """Save weights to DB."""
    conn = sqlite3.connect(str(DB_PATH))
    conn.execute(
        "INSERT INTO weights_history (timestamp, weights_json, trigger) VALUES (?, ?, ?)",
        (int(time.time()), json.dumps(weights), trigger),
    )
    conn.commit()
    conn.close()


def dma_update_weights(weights, predictions, outcomes, alpha=0.95, lambda_=0.95, max_change=0.03):
    """Dynamic Model Averaging weight update based on prediction accuracy."""
    if not predictions or not outcomes:
        return weights

    updated = {}
    for indicator, weight in weights.items():
        if indicator in predictions and indicator in outcomes:
            correct = predictions[indicator] == outcomes[indicator]
            likelihood = 0.7 if correct else 0.3
            updated_weight = lambda_ * weight * likelihood
            # Clamp change
            change = updated_weight - weight
            change = max(-max_change, min(max_change, change))
            updated[indicator] = weight + change
        else:
            updated[indicator] = weight

    # Normalize
    total = sum(updated.values())
    if total > 0:
        for k in updated:
            updated[k] = updated[k] / total

    # Guard: backtest weight always >= 0.10
    updated["backtest"] = max(updated.get("backtest", 0.15), 0.10)

    return updated


# ── Outcome Verification ───────────────────────────────────────────
def verify_past_predictions(conn):
    """Check past predictions vs actual outcomes."""
    now = int(time.time())
    window = 7 * 86400  # 7 days

    unverified = conn.execute(
        """
        SELECT id, symbol, price, recommendation, timestamp, snapshot_json
        FROM journal
        WHERE outcome_json IS NULL
        AND timestamp < ?
        AND recommendation IN ('STRONG_BUY', 'BUY', 'SELL', 'STRONG_SELL')
        ORDER BY timestamp DESC
        LIMIT 50
    """,
        (now - window,),
    ).fetchall()

    if not unverified:
        return 0, 0

    verified = 0
    correct = 0

    for entry_id, symbol, entry_price, rec, ts, snap_json in unverified:
        try:
            # Fetch current price
            url = f"https://api.binance.com/api/v3/ticker/price?symbol={symbol}"
            data = fetch_json(url)
            current_price = float(data["price"])
        except Exception:
            continue

        pnl_pct = (current_price / entry_price - 1) * 100

        # Determine if prediction was correct
        was_bullish = rec in ("STRONG_BUY", "BUY")
        actual_bullish = pnl_pct > 0

        is_correct = was_bullish == actual_bullish
        if is_correct:
            correct += 1
        verified += 1

        # Store outcome
        outcome = {
            "actual_price": current_price,
            "pnl_pct": round(pnl_pct, 2),
            "correct": is_correct,
            "verified_at": now,
        }
        conn.execute(
            "UPDATE journal SET outcome_json = ?, verified = 1 WHERE id = ?",
            (json.dumps(outcome), entry_id),
        )

        time.sleep(0.1)  # Rate limit

    conn.commit()
    return verified, correct


# ── Logging ─────────────────────────────────────────────────────────
def log(msg, verbose=False):
    ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    line = f"[{ts}] {msg}"
    with open(str(LOG_FILE), "a") as f:
        f.write(line + "\n")
    if verbose:
        print(line)


# ── Main Self-Learning Loop ────────────────────────────────────────
def run_cycle(top_n=100, verbose=False):
    """Run one complete self-learning cycle."""
    cycle_start = time.time()
    conn = init_db()
    weights = load_weights()

    log("=" * 60, verbose)
    log(f"🔄 SELF-LEARNING CYCLE START — Top {top_n} coins", verbose)
    log(f"📊 Current weights: {json.dumps({k: round(v,3) for k,v in weights.items()})}", verbose)
    log("=" * 60, verbose)

    # ── STEP 1: SCAN ───────────────────────────────────────────────
    log("\n📡 STEP 1: SCANNING top coins...", verbose)
    try:
        symbols = fetch_top_symbols(top_n)
    except Exception as e:
        log(f"❌ Failed to fetch symbols: {e}", verbose)
        return

    log(f"   Found {len(symbols)} symbols", verbose)

    # ── STEP 2: ANALYZE ────────────────────────────────────────────
    log(f"\n🔬 STEP 2: ANALYZING {len(symbols)} coins...", verbose)
    all_results = []
    backtest_count = 0

    for i, symbol in enumerate(symbols):
        try:
            klines = fetch_klines(symbol, "1d", 200)
            analysis = full_analysis(klines, symbol)
            if analysis is None:
                continue

            score, rec, signals = score_analysis(analysis, weights)

            # ── STEP 3: BACKTEST (for top candidates) ──────────────
            bt_result = None
            if score >= 55:  # Only backtest promising ones
                bt_result = quick_backtest(klines, analysis)
                backtest_count += 1

                # Adjust score based on backtest
                if bt_result:
                    bt_sharpe = bt_result["sharpe"]
                    if bt_sharpe > 1.5:
                        score = min(100, score + weights["backtest"] * 50)
                        signals.append(f"BT Sharpe {bt_sharpe:.1f} ✓")
                    elif bt_sharpe < 0:
                        score = max(0, score - weights["backtest"] * 30)
                        signals.append(f"BT Sharpe {bt_sharpe:.1f} ✗")

            all_results.append({
                "analysis": analysis,
                "score": score,
                "rec": rec,
                "signals": signals,
                "bt_result": bt_result,
            })

            if verbose and (i + 1) % 20 == 0:
                print(f"   ...analyzed {i+1}/{len(symbols)}")

            time.sleep(0.12)  # Rate limit

        except Exception as e:
            if verbose:
                print(f"   ⚠️ {symbol}: {e}")

    # Sort by score
    all_results.sort(key=lambda x: x["score"], reverse=True)

    log(f"\n   ✅ Analyzed {len(all_results)} coins, backtested {backtest_count}", verbose)

    # ── STEP 4: JOURNAL ────────────────────────────────────────────
    log(f"\n📝 STEP 4: JOURNALING top picks...", verbose)

    now = int(time.time())
    journal_count = 0

    for r in all_results:
        if r["rec"] not in ("STRONG_BUY", "BUY", "SELL", "STRONG_SELL"):
            continue

        a = r["analysis"]
        entry_id = f"{a['symbol']}_{now}"

        # Calculate SL/TP
        atr = a.get("atr") or a["price"] * 0.05
        if r["rec"] in ("STRONG_BUY", "BUY"):
            sl = a["price"] - atr * 1.5
            tp = a["price"] + atr * 3.0
        else:
            sl = a["price"] + atr * 1.5
            tp = a["price"] - atr * 3.0

        snapshot = {
            "rsi": a["rsi"],
            "macd_bullish": a["macd_bullish"],
            "bb_pct": a["bb_pct"],
            "vol_ratio": a["vol_ratio"],
            "regime": a["regime"],
            "chg_7d": a["chg_7d"],
            "from_high": a["from_high"],
            "signals": r["signals"],
            "backtest": r["bt_result"],
            "weights_used": {k: round(v, 3) for k, v in weights.items()},
        }

        try:
            conn.execute(
                """INSERT OR IGNORE INTO journal
                (id, timestamp, symbol, score, recommendation, price,
                 stop_loss, take_profit, rsi, macd_signal, bb_pct,
                 volume_ratio, regime, backtest_sharpe, snapshot_json)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    entry_id, now, a["symbol"], r["score"], r["rec"],
                    a["price"], sl, tp, a["rsi"],
                    "bullish" if a["macd_bullish"] else "bearish",
                    a["bb_pct"], a["vol_ratio"], a["regime"],
                    r["bt_result"]["sharpe"] if r["bt_result"] else None,
                    json.dumps(snapshot),
                ),
            )
            journal_count += 1
        except Exception as e:
            if verbose:
                print(f"   ⚠️ Journal error for {a['symbol']}: {e}")

    conn.commit()
    log(f"   ✅ Journalled {journal_count} trade entries", verbose)

    # ── STEP 5: REVIEW ─────────────────────────────────────────────
    log(f"\n🔍 STEP 5: REVIEWING past predictions...", verbose)
    verified, correct = verify_past_predictions(conn)

    if verified > 0:
        accuracy = correct / verified * 100
        log(f"   ✅ Verified {verified} predictions, {correct} correct ({accuracy:.1f}%)", verbose)
    else:
        accuracy = 0
        log(f"   ℹ️ No predictions to verify yet", verbose)

    # ── STEP 6: LEARN ──────────────────────────────────────────────
    log(f"\n🧠 STEP 6: LEARNING — DMA weight update...", verbose)

    # Get prediction accuracy per indicator
    indicator_accuracy = {}
    recent = conn.execute("""
        SELECT snapshot_json, outcome_json
        FROM journal
        WHERE outcome_json IS NOT NULL
        AND verified = 1
        ORDER BY timestamp DESC
        LIMIT 100
    """).fetchall()

    if recent:
        predictions = {}
        outcomes = {}

        for snap_str, outcome_str in recent:
            try:
                snap = json.loads(snap_str)
                outcome = json.loads(outcome_str)
                was_bullish_signal = snap.get("macd_bullish", False)

                # Per-indicator accuracy
                for ind in weights:
                    if ind not in predictions:
                        predictions[ind] = []
                        outcomes[ind] = []

                    if ind == "rsi":
                        rsi_val = snap.get("rsi", 50)
                        pred = "bullish" if rsi_val < 35 else "bearish" if rsi_val > 65 else "neutral"
                    elif ind == "macd":
                        pred = "bullish" if snap.get("macd_bullish") else "bearish"
                    elif ind == "bollinger":
                        bb = snap.get("bb_pct", 50)
                        pred = "bullish" if bb < 20 else "bearish" if bb > 80 else "neutral"
                    elif ind == "sma_cross":
                        pred = "bullish"  # simplified
                    elif ind == "volume":
                        pred = "bullish" if snap.get("vol_ratio", 1) > 1.3 else "neutral"
                    elif ind == "momentum":
                        pred = "bullish" if snap.get("chg_7d", 0) < -10 else "neutral"
                    elif ind == "mean_reversion":
                        pred = "bullish" if snap.get("from_high", 0) < -15 else "neutral"
                    elif ind == "backtest":
                        bt = snap.get("backtest")
                        pred = "bullish" if bt and bt.get("sharpe", 0) > 0.5 else "neutral"
                    else:
                        pred = "neutral"

                    actual = "bullish" if outcome.get("pnl_pct", 0) > 0 else "bearish"

                    predictions[ind].append(pred)
                    outcomes[ind].append(actual)

            except Exception:
                continue

        # Compute per-indicator predictions for DMA
        dma_preds = {}
        dma_outcomes = {}
        for ind in weights:
            if ind in predictions and predictions[ind]:
                # Last prediction
                dma_preds[ind] = predictions[ind][-1]
                dma_outcomes[ind] = outcomes[ind][-1]

        weights = dma_update_weights(weights, dma_preds, dma_outcomes)
        save_weights(weights, f"cycle_{now}")

        log(f"   📊 Updated weights: {json.dumps({k: round(v,3) for k,v in weights.items()})}", verbose)
    else:
        log(f"   ℹ️ Not enough data to update weights yet — will learn from next cycle", verbose)

    # ── SAVE STATS ─────────────────────────────────────────────────
    best = all_results[0] if all_results else None
    conn.execute(
        """INSERT INTO learning_stats
        (timestamp, total_predictions, correct_predictions, accuracy,
         avg_sharpe, best_strategy, notes)
        VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (
            now,
            verified,
            correct,
            accuracy,
            sum(
                r["bt_result"]["sharpe"]
                for r in all_results
                if r.get("bt_result")
            )
            / max(1, len([r for r in all_results if r.get("bt_result")])),
            f"{best['analysis']['symbol']}:{best['rec']}" if best else "N/A",
            json.dumps({
                "coins_analyzed": len(all_results),
                "journal_entries": journal_count,
                "verified": verified,
                "accuracy": round(accuracy, 1),
                "top_pick": {
                    "symbol": best["analysis"]["symbol"],
                    "score": best["score"],
                    "rec": best["rec"],
                } if best else None,
            }),
        ),
    )
    conn.commit()

    # ── FINAL REPORT ───────────────────────────────────────────────
    elapsed = time.time() - cycle_start
    log(f"\n{'━' * 60}", verbose)
    log(f"📊 CYCLE COMPLETE ({elapsed:.0f}s)", verbose)
    log(f"{'━' * 60}", verbose)
    log(f"  Coins analyzed:  {len(all_results)}", verbose)
    log(f"  Backtests run:   {backtest_count}", verbose)
    log(f"  Journal entries: {journal_count}", verbose)
    log(f"  Predictions verified: {verified} ({accuracy:.0f}% accuracy)", verbose)

    if all_results:
        log(f"\n🏆 TOP 10 PICKS:", verbose)
        log(f"  {'#':<3} {'Symbol':<14} {'Score':>6} {'Rec':<14} {'RSI':>5} {'Regime':<12} {'Signals'}", verbose)
        log(f"  {'─'*80}", verbose)
        for i, r in enumerate(all_results[:10]):
            a = r["analysis"]
            sig_str = ", ".join(r["signals"][:3]) if r["signals"] else "Neutral"
            rsi_str = f"{a['rsi']:.0f}" if a["rsi"] else "N/A"
            log(
                f"  {i+1:<3} {a['symbol']:<14} {r['score']:>5.0f} {r['rec']:<14} {rsi_str:>5} {a['regime']:<12} {sig_str}",
                verbose,
            )

    conn.close()
    return all_results


def main():
    parser = argparse.ArgumentParser(description="BonBo Self-Learning Loop v2")
    parser.add_argument("--top-n", type=int, default=100, help="Number of coins to scan")
    parser.add_argument("--once", action="store_true", help="Run one cycle then exit")
    parser.add_argument("--interval", type=int, default=300, help="Seconds between cycles")
    parser.add_argument("--verbose", "-v", action="store_true", help="Verbose output")
    args = parser.parse_args()

    if args.once:
        run_cycle(args.top_n, args.verbose)
    else:
        cycle = 0
        while True:
            cycle += 1
            print(f"\n{'='*60}")
            print(f"  CYCLE #{cycle} — {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
            print(f"{'='*60}")
            run_cycle(args.top_n, verbose=True)
            print(f"\n⏳ Next cycle in {args.interval}s... (Ctrl+C to stop)")
            try:
                time.sleep(args.interval)
            except KeyboardInterrupt:
                print("\n👋 Self-learning stopped by user.")
                break


if __name__ == "__main__":
    main()
