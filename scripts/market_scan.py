#!/usr/bin/env python3
"""BonBo Market Scanner — Quét thị trường tìm giao dịch tốt nhất qua MCP server."""

import json
import subprocess
import sys
import time

MCP_BIN = "./target/release/bonbo-extend-mcp"

def call_mcp(method, params=None):
    """Gọi MCP tool và trả về kết quả."""
    req = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
    }
    if params:
        req["params"] = params
    
    proc = subprocess.run(
        [MCP_BIN],
        input=json.dumps(req),
        capture_output=True,
        text=True,
        timeout=30,
    )
    
    for line in proc.stdout.strip().split('\n'):
        try:
            resp = json.loads(line)
            if "result" in resp:
                content = resp["result"].get("content", [])
                for c in content:
                    if c.get("type") == "text":
                        try:
                            return json.loads(c["text"])
                        except json.JSONDecodeError:
                            return c["text"]
                return resp["result"]
        except json.JSONDecodeError:
            continue
    return None

def call_tool(name, arguments=None):
    """Gọi MCP tool."""
    params = {"name": name}
    if arguments:
        params["arguments"] = arguments
    return call_mcp("tools/call", params)

def main():
    print("=" * 70)
    print("🤖 BONBO MARKET SCANNER — Quét Thị Trường Tìm Giao Dịch Tốt Nhất")
    print("=" * 70)
    print()
    
    # 1. SENTIMENT
    print("📊 Bước 1: Kiểm tra Sentiment...")
    print("-" * 50)
    
    fg = call_tool("get_fear_greed_index", {"history": 1})
    if fg:
        print(f"  Fear & Greed: {fg}")
    
    sentiment = call_tool("get_composite_sentiment", {"symbol": "BTCUSDT"})
    if sentiment:
        print(f"  Composite Sentiment: {sentiment}")
    
    whales = call_tool("get_whale_alerts", {"min_usd": 500000})
    if whales:
        print(f"  Whale Alerts: {whales}")
    
    print()
    
    # 2. TOP CRYPTO PRICES
    print("💰 Bước 2: Lấy giá Top Crypto...")
    print("-" * 50)
    
    top = call_tool("get_top_crypto", {"limit": 20})
    if top and isinstance(top, list):
        for coin in top[:10]:
            sym = coin.get("symbol", "?")
            price = coin.get("current_price", 0)
            change = coin.get("price_change_percentage_24h", 0)
            vol = coin.get("total_volume", 0)
            mcap = coin.get("market_cap", 0)
            emoji = "🟢" if change > 0 else "🔴"
            print(f"  {emoji} {sym:12s} ${price:>12,.4f} | 24h: {change:>+7.2f}% | Vol: ${vol/1e6:>8,.0f}M | MCap: ${mcap/1e9:>6,.1f}B")
    elif top:
        print(f"  Top crypto: {top}")
    
    print()
    
    # 3. SCAN MARKET
    print("🔍 Bước 3: Scan Market — Tìm cơ hội...")
    print("-" * 50)
    
    scan = call_tool("scan_market", {"market": "crypto"})
    if scan:
        if isinstance(scan, dict):
            for key, val in scan.items():
                if isinstance(val, list):
                    print(f"  {key}: {len(val)} results")
                    for item in val[:5]:
                        print(f"    → {item}")
                else:
                    print(f"  {key}: {val}")
        else:
            print(f"  Scan result: {scan}")
    
    print()
    
    # 4. ANALYZE TOP CANDIDATES
    symbols = ["BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT", 
               "DOGEUSDT", "ADAUSDT", "AVAXUSDT", "PENDLEUSDT", "LINKUSDT"]
    
    print(f"📈 Bước 4: Phân tích {len(symbols)} symbols...")
    print("-" * 50)
    
    results = []
    
    for symbol in symbols:
        print(f"\n  📊 {symbol}:")
        
        # Get price
        price_data = call_tool("get_crypto_price", {"symbol": symbol})
        if price_data:
            print(f"    Price: {price_data}")
        
        # Analyze indicators
        indicators = call_tool("analyze_indicators", {"symbol": symbol, "timeframe": "1h"})
        if indicators:
            if isinstance(indicators, dict):
                rsi = indicators.get("rsi", indicators.get("RSI", "?"))
                macd = indicators.get("macd", indicators.get("MACD", "?"))
                bb = indicators.get("bollinger_bands", indicators.get("bb", "?"))
                print(f"    RSI(14): {rsi}")
                print(f"    MACD: {macd}")
                if isinstance(indicators, dict):
                    for k, v in list(indicators.items())[:8]:
                        if k not in ("rsi", "RSI", "macd", "MACD", "bollinger_bands", "bb"):
                            print(f"    {k}: {v}")
            else:
                print(f"    Indicators: {str(indicators)[:200]}")
        
        # Get trading signals
        signals = call_tool("get_trading_signals", {"symbol": symbol, "timeframe": "1h"})
        if signals:
            print(f"    Signals: {signals}")
        
        # Detect regime
        regime = call_tool("detect_market_regime", {"symbol": symbol})
        if regime:
            print(f"    Regime: {regime}")
        
        time.sleep(0.1)
    
    print()
    
    # 5. COMPARE STRATEGIES
    print("⚡ Bước 5: So sánh chiến lược...")
    print("-" * 50)
    
    strategies = call_tool("list_strategies")
    if strategies:
        print(f"  Available strategies: {strategies}")
    
    for symbol in ["BTCUSDT", "ETHUSDT", "SOLUSDT"]:
        compare = call_tool("compare_strategies", {"symbol": symbol, "timeframe": "1h"})
        if compare:
            print(f"\n  {symbol} strategy comparison:")
            if isinstance(compare, dict):
                for strat, result in compare.items():
                    print(f"    {strat}: {result}")
            else:
                print(f"    {str(compare)[:300]}")
    
    print()
    print("=" * 70)
    print("✅ QUÉT HOÀN TẤT!")
    print("=" * 70)


if __name__ == "__main__":
    main()
