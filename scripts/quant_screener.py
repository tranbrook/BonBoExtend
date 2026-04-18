#!/usr/bin/env python3
"""
BonBoExtend — Quantitative Top-20 Crypto Screener
Phân tích toàn diện top 20 coin để tìm vị thế giao dịch tốt nhất.

Usage: python3 scripts/quant_screener.py
"""

import json
import re
import sys
import time
import urllib.request
from datetime import datetime

MCP_URL = "http://localhost:9876/mcp"

def mcp_call(tool, arguments, timeout=30):
    """Call MCP tool and return parsed result."""
    payload = json.dumps({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
        "id": 1
    }).encode()
    
    req = urllib.request.Request(MCP_URL, data=payload, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode())
            text = data.get("result", {}).get("content", [{}])[0].get("text", "")
            return {"ok": True, "text": text}
    except Exception as e:
        return {"ok": False, "text": str(e)}

def parse_number(text, prefix):
    """Extract number after prefix in text."""
    patterns = {
        "price": r'\*\*Price\*\*:\s*\$?([\d,.]+)',
        "rsi": r'RSI\(14\)\):\s*([\d.]+)',
        "macd_hist": r'hist=([-\d.]+)',
        "bb_pctb": r'%B=([\d.]+)',
        "change_24h": r'24h Change:\s*([-\d.]+)%',
        "price_inline": r'Price:\s*\$([\d,.]+)',
    }
    if prefix in patterns:
        m = re.search(patterns[prefix], text)
        if m:
            return float(m.group(1).replace(",", ""))
    return None

def get_top_coins(limit=20):
    """Get top coins by volume."""
    result = mcp_call("get_top_crypto", {"limit": limit})
    if not result["ok"]:
        print(f"❌ Cannot get top coins: {result['text']}")
        sys.exit(1)
    
    # Parse table
    lines = result["text"].split("\n")
    coins = []
    for line in lines:
        if line.startswith("|") and "BTC" in line or "ETH" in line or "USDT" in line:
            parts = [p.strip() for p in line.split("|")]
            if len(parts) >= 5 and parts[2] and "$" in (parts[3] or ""):
                try:
                    symbol = parts[2].strip()
                    price_str = parts[3].replace("$", "").replace(",", "").strip()
                    change_str = parts[4].replace("%", "").replace("+", "").replace("↓", "-").replace("↑", "+").strip()
                    vol_str = parts[5].replace("$", "").replace(",", "").strip() if len(parts) > 5 else "0"
                    
                    if "USDT" not in symbol and len(symbol) >= 3:
                        symbol = symbol + "USDT"
                    
                    coins.append({
                        "symbol": symbol,
                        "price": float(price_str) if price_str else 0,
                        "change_24h": float(change_str) if change_str else 0,
                        "volume": float(vol_str) if vol_str else 0,
                    })
                except:
                    pass
    
    # Fallback: hardcoded top 20
    if len(coins) < 10:
        coins = [
            {"symbol": "BTCUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "ETHUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "SOLUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "BNBUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "XRPUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "ADAUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "DOGEUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "AVAXUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "DOTUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "LINKUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "MATICUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "LTCUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "UNIUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "ATOMUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "ETCUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "NEARUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "FILUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "APTUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "ARBUSDT", "price": 0, "change_24h": 0, "volume": 0},
            {"symbol": "OPUSDT", "price": 0, "change_24h": 0, "volume": 0},
        ]
    
    return coins[:20]

def analyze_coin(symbol):
    """Full quantitative analysis of a single coin."""
    analysis = {"symbol": symbol, "scores": {}}
    
    # 1. Price data
    price_res = mcp_call("get_crypto_price", {"symbol": symbol}, timeout=15)
    if price_res["ok"]:
        m = re.search(r'Price:\s*\$([\d,.]+)', price_res["text"])
        if m:
            analysis["price"] = float(m.group(1).replace(",", ""))
        m = re.search(r'24h Change:\s*([-\d.]+)%', price_res["text"])
        if m:
            analysis["change_24h"] = float(m.group(1))
    
    # 2. Technical indicators (daily)
    ta_res = mcp_call("analyze_indicators", {"symbol": symbol, "interval": "1d", "limit": 50}, timeout=20)
    if ta_res["ok"]:
        text = ta_res["text"]
        
        # RSI — format: "**RSI(14)**: 68.1 ⚪ Neutral"
        m = re.search(r'RSI[^:]*:\s*([\d.]+)', text)
        if m:
            analysis["rsi"] = float(m.group(1))
        
        # MACD histogram
        m = re.search(r'hist=([-\d.]+)', text)
        if m:
            analysis["macd_hist"] = float(m.group(1))
        
        # MACD signal (check 🟢 or 🔴 on MACD line)
        macd_section = text[text.find("MACD"):] if "MACD" in text else ""
        if "🟢" in macd_section[:60]:
            analysis["macd_signal"] = "bullish"
        elif "🔴" in macd_section[:60]:
            analysis["macd_signal"] = "bearish"
            
        # Bollinger %B
        m = re.search(r'%B=([\d.]+)', text)
        if m:
            analysis["bb_pctb"] = float(m.group(1))
        
        # Price from TA
        m = re.search(r'\*\*Price\*\*:\s*\$([\d,.]+)', text)
        if m and "price" not in analysis:
            analysis["price"] = float(m.group(1).replace(",", ""))
    
    # 3. Trading signals
    sig_res = mcp_call("get_trading_signals", {"symbol": symbol, "interval": "1d"}, timeout=20)
    if sig_res["ok"]:
        text = sig_res["text"]
        buy_signals = text.count("🟢")
        sell_signals = text.count("🔴")
        analysis["buy_signals"] = buy_signals
        analysis["sell_signals"] = sell_signals
        
        # Extract signal details
        confidences = re.findall(r'\((\d+)%\)', text)
        buy_conf = []
        sell_conf = []
        sections = text.split("🟢 **Buy**")[1:] if "🟢 **Buy**" in text else []
        for s in sections:
            m = re.search(r'\((\d+)%\)', s[:50])
            if m:
                buy_conf.append(int(m.group(1)))
        
        sections = text.split("🔴 **Sell**")[1:] if "🔴 **Sell**" in text else []
        for s in sections:
            m = re.search(r'\((\d+)%\)', s[:50])
            if m:
                sell_conf.append(int(m.group(1)))
        
        analysis["buy_confidence"] = sum(buy_conf) if buy_conf else 0
        analysis["sell_confidence"] = sum(sell_conf) if sell_conf else 0
    
    # 4. Market regime
    regime_res = mcp_call("detect_market_regime", {"symbol": symbol, "interval": "1d"}, timeout=20)
    if regime_res["ok"]:
        text = regime_res["text"]
        if "Trending Up" in text:
            analysis["regime"] = "uptrend"
            analysis["regime_emoji"] = "📈"
        elif "Trending Down" in text:
            analysis["regime"] = "downtrend"
            analysis["regime_emoji"] = "📉"
        else:
            analysis["regime"] = "ranging"
            analysis["regime_emoji"] = "↔️"
    
    # 5. Support/Resistance
    sr_res = mcp_call("get_support_resistance", {"symbol": symbol, "interval": "1d"}, timeout=20)
    if sr_res["ok"]:
        text = sr_res["text"]
        # Nearest resistance — format: "R1: $price (+-X%)" or "R1: $price (+X%)"
        resistances = re.findall(r'R\d+:\s*\$([\d,.]+)\s*\(\+?(-?[\d.]+)%\)', text)
        if resistances:
            analysis["nearest_resistance_price"] = float(resistances[0][0].replace(",", ""))
            r_pct = float(resistances[0][1])
            # If resistance is below current price, the distance is negative
            analysis["nearest_resistance_pct"] = abs(r_pct)
        # Nearest support
        supports = re.findall(r'S\d+:\s*\$([\d,.]+)\s*\(-([\d.]+)%\)', text)
        if supports:
            analysis["nearest_support_price"] = float(supports[0][0].replace(",", ""))
            analysis["nearest_support_pct"] = float(supports[0][1])
    
    # 6. Backtest (quick SMA crossover)
    bt_res = mcp_call("run_backtest", {
        "symbol": symbol, "interval": "4h", "strategy": "sma_crossover", "initial_capital": 10000
    }, timeout=45)
    if bt_res["ok"]:
        text = bt_res["text"]
        m = re.search(r'Total Return\*\*:\s*([-\d.]+)%', text)
        if m:
            analysis["backtest_return"] = float(m.group(1))
        m = re.search(r'Win Rate:\s*([\d.]+)%', text)
        if m:
            analysis["backtest_winrate"] = float(m.group(1))
        m = re.search(r'Sharpe Ratio:\s*([-\d.]+)', text)
        if m:
            analysis["backtest_sharpe"] = float(m.group(1))
        m = re.search(r'Max Drawdown:\s*([\d.]+)%', text)
        if m:
            analysis["backtest_maxdd"] = float(m.group(1))
    
    return analysis

def compute_score(a):
    """Compute composite quantitative score (0-100)."""
    score = 50.0  # base
    
    # RSI score (oversold = bullish opportunity)
    rsi = a.get("rsi", 50)
    if rsi < 30:
        score += 20  # oversold = strong buy
    elif rsi < 40:
        score += 12
    elif rsi < 50:
        score += 5
    elif rsi > 80:
        score -= 15  # overbought
    elif rsi > 70:
        score -= 8
    elif rsi > 60:
        score -= 2
    
    # MACD score
    if a.get("macd_signal") == "bullish":
        score += 10
    elif a.get("macd_signal") == "bearish":
        score -= 10
    
    macd_hist = a.get("macd_hist", 0)
    if abs(macd_hist) > 0:
        score += min(8, max(-8, macd_hist * 2))
    
    # BB %B score (low %B = potential bounce)
    bb = a.get("bb_pctb", 0.5)
    if bb < 0.2:
        score += 10  # near lower band = bounce potential
    elif bb > 0.9:
        score -= 8  # near upper band = overextended
    elif 0.4 <= bb <= 0.6:
        score += 2  # middle = neutral healthy
    
    # Signal score
    buy = a.get("buy_signals", 0)
    sell = a.get("sell_signals", 0)
    buy_conf = a.get("buy_confidence", 0)
    sell_conf = a.get("sell_confidence", 0)
    net_signals = (buy * 5 + buy_conf * 0.1) - (sell * 5 + sell_conf * 0.1)
    score += net_signals
    
    # Regime score
    regime = a.get("regime", "")
    if regime == "uptrend":
        score += 8
    elif regime == "downtrend":
        score -= 8
    
    # Risk/Reward (support vs resistance)
    sup_pct = a.get("nearest_support_pct", 5)
    res_pct = a.get("nearest_resistance_pct", 5)
    if res_pct > 0 and sup_pct > 0:
        rr_ratio = res_pct / sup_pct
        if rr_ratio > 2.0:
            score += 10
        elif rr_ratio > 1.5:
            score += 5
        elif rr_ratio < 1.0:
            score -= 5
    
    # Backtest score
    bt_ret = a.get("backtest_return", 0)
    if bt_ret > 5:
        score += 10
    elif bt_ret > 0:
        score += 5
    elif bt_ret < -5:
        score -= 8
    elif bt_ret < 0:
        score -= 3
    
    bt_sharpe = a.get("backtest_sharpe", 0)
    if bt_sharpe > 1.0:
        score += 5
    elif bt_sharpe < -0.5:
        score -= 5
    
    return max(0, min(100, score))

def main():
    print("=" * 80)
    print("🔬 BonBoExtend — QUANTITATIVE TOP-20 CRYPTO SCREENER")
    print(f"   Thời gian: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("=" * 80)
    
    # Step 1: Get sentiment context
    print("\n📊 Đang lấy dữ liệu sentiment thị trường...")
    fg_res = mcp_call("get_fear_greed_index", {"history": 1})
    if fg_res["ok"]:
        print(f"   {fg_res['text'][:100]}")
    
    sentiment_res = mcp_call("get_composite_sentiment", {"symbol": "BTCUSDT"})
    if sentiment_res["ok"]:
        print(f"   {sentiment_res['text'][:120]}")
    
    # Step 2: Get top coins
    print("\n🏆 Đang lấy top 20 coins...")
    coins = get_top_coins(20)
    print(f"   Tìm thấy {len(coins)} coins")
    
    # Step 3: Analyze each coin
    results = []
    total = len(coins)
    
    for i, coin in enumerate(coins):
        symbol = coin["symbol"]
        print(f"\n   [{i+1}/{total}] 🔍 Đang phân tích {symbol}...", end="", flush=True)
        
        try:
            analysis = analyze_coin(symbol)
            analysis["score"] = compute_score(analysis)
            results.append(analysis)
            
            # Quick status
            rsi = analysis.get("rsi", "?")
            regime = analysis.get("regime_emoji", "?")
            score = analysis["score"]
            price = analysis.get("price", "?")
            
            status = "🟢" if score >= 60 else "🔴" if score < 40 else "⚪"
            print(f" {status} Score={score:.0f} | RSI={rsi} | {regime} | ${price}")
        except Exception as e:
            print(f" ❌ Error: {e}")
        
        time.sleep(0.3)  # Rate limit
    
    # Step 4: Sort by score
    results.sort(key=lambda x: x.get("score", 0), reverse=True)
    
    # Step 5: Print report
    print("\n")
    print("=" * 80)
    print("📋 BÁO CÁO XẾP HẠNG TOP 20 COIN")
    print("=" * 80)
    
    # Header
    print(f"\n{'#':<3} {'Coin':<12} {'Score':>5} {'RSI':>5} {'MACD':>8} {'BB%B':>5} {'Regime':>10} {'Signal':>8} {'BT Ret':>7} {'Sharpe':>7} {'Đề xuất'}")
    print("─" * 95)
    
    for i, a in enumerate(results):
        symbol = a["symbol"].replace("USDT", "")
        score = a.get("score", 0)
        rsi = a.get("rsi", 0)
        rsi_str = f"{rsi:.1f}" if rsi else "?"
        
        macd = a.get("macd_signal", "?")
        if macd == "bullish":
            macd_str = "🟢 Bull"
        elif macd == "bearish":
            macd_str = "🔴 Bear"
        else:
            macd_str = "⚪ ?"
        
        bb = a.get("bb_pctb", 0)
        bb_str = f"{bb:.2f}" if bb else "?"
        
        regime = a.get("regime_emoji", "?") + " " + a.get("regime", "?")[:6]
        
        buy_s = a.get("buy_signals", 0)
        sell_s = a.get("sell_signals", 0)
        signal_str = f"B:{buy_s}/S:{sell_s}"
        
        bt_ret = a.get("backtest_return", 0)
        bt_str = f"{bt_ret:+.1f}%" if bt_ret is not None else "?"
        
        sharpe = a.get("backtest_sharpe", 0)
        sharpe_str = f"{sharpe:.2f}" if sharpe is not None else "?"
        
        # Recommendation
        if score >= 70:
            rec = "🟢🟢 MUA MẠNH"
        elif score >= 60:
            rec = "🟢 MUA"
        elif score >= 50:
            rec = "⚪ GIỮ"
        elif score >= 40:
            rec = "🟠 BÁN NHẸ"
        else:
            rec = "🔴 BÁN"
        
        print(f"{i+1:<3} {symbol:<12} {score:>5.0f} {rsi_str:>5} {macd_str:>8} {bb_str:>5} {regime:>10} {signal_str:>8} {bt_str:>7} {sharpe_str:>7} {rec}")
    
    # Step 6: Top 3 picks detail
    print("\n")
    print("=" * 80)
    print("🏆 TOP 3 VỊ THẾ GIAO DỊCH TỐT NHẤT")
    print("=" * 80)
    
    for rank, a in enumerate(results[:3]):
        symbol = a["symbol"]
        price = a.get("price", 0)
        score = a.get("score", 0)
        
        emoji = "🥇" if rank == 0 else "🥈" if rank == 1 else "🥉"
        
        print(f"\n{emoji} #{rank+1}: {symbol} — Quant Score: {score:.0f}/100")
        print(f"   Giá: ${price}")
        print(f"   RSI: {a.get('rsi', '?')} | MACD: {a.get('macd_signal', '?')} | BB%B: {a.get('bb_pctb', '?')}")
        print(f"   Regime: {a.get('regime_emoji', '?')} {a.get('regime', '?')}")
        print(f"   Signals: {a.get('buy_signals', 0)} Buy / {a.get('sell_signals', 0)} Sell")
        print(f"   Nearest Resistance: +{a.get('nearest_resistance_pct', '?')}%")
        print(f"   Nearest Support: -{a.get('nearest_support_pct', '?')}%")
        print(f"   Backtest: {a.get('backtest_return', 0):+.1f}% return | Sharpe: {a.get('backtest_sharpe', 0):.2f}")
        
        # Position sizing recommendation
        if price > 0:
            sup_pct = a.get("nearest_support_pct", 5)
            res_pct = a.get("nearest_resistance_pct", 10)
            stop_price = price * (1 - sup_pct / 100)
            target_price = price * (1 + res_pct / 100)
            
            rr = res_pct / sup_pct if sup_pct > 0 and res_pct > 0 else 0
            
            print(f"   📐 Entry: ${price:.2f} | Stop: ${stop_price:.2f} (-{sup_pct:.1f}%) | Target: ${target_price:.2f} (+{res_pct:.1f}%)")
            print(f"   📐 Risk/Reward: 1:{rr:.1f}" if rr > 0 else "   📐 Risk/Reward: N/A")
            
            # Position size with Fixed 2%
            risk_amount = 200  # 2% of $10,000
            risk_per_unit = price - stop_price
            if risk_per_unit > 0:
                position_size = risk_amount / risk_per_unit
                print(f"   💰 Position (2% risk): {position_size:.2f} {symbol.replace('USDT','')} (${position_size * price:.0f})")
    
    print("\n" + "=" * 80)
    print("⚠️ DISCLAIMER: Phân tích mang tính chất tham khảo, không phải lời khuyên đầu tư.")
    print("   Luôn sử dụng risk management và không đầu tư quá khả năng chịu đựng.")
    print("=" * 80)

if __name__ == "__main__":
    main()
