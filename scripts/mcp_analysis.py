#!/usr/bin/env python3
"""
BonBo MCP Client — Phân tích vi thế giao dịch TỐT NHẤT qua MCP Server.
Gọi 15 tools qua bonbo-extend-mcp (stdio mode) theo quy trình 5 bước.
"""
import subprocess, json, sys, time, os

MCP_BIN = os.path.join(os.path.dirname(__file__), "..", "target", "release", "bonbo-extend-mcp")

class McpClient:
    def __init__(self):
        self.proc = subprocess.Popen(
            [MCP_BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1
        )
        self.req_id = 0
        time.sleep(2)  # warm-up

    def call(self, method, params=None):
        self.req_id += 1
        req = {"jsonrpc": "2.0", "id": self.req_id, "method": method}
        if params: req["params"] = params
        self.proc.stdin.write(json.dumps(req) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        return json.loads(line.strip()) if line else None

    def tool(self, name, arguments=None):
        return self.call("tools/call", {"name": name, "arguments": arguments or {}})

    def close(self):
        self.proc.terminate()
        self.proc.wait(timeout=5)

def text(result):
    if not result: return "❌ No response"
    if "error" in result: return f"❌ {result['error'].get('message','?')}"
    parts = []
    for c in result.get("result", {}).get("content", []):
        if c.get("type") == "text": parts.append(c["text"])
    return "\n".join(parts) if parts else "❌ Empty"

def sep(t):
    print(f"\n{'═'*80}")
    print(f"  {t}")
    print(f"{'═'*80}")

def main():
    print("🚀 BonBo MCP Client — Phân tích qua bonbo-extend-mcp Server\n")
    c = McpClient()

    # Init
    init = c.call("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "bonbo-client", "version": "1.0"}
    })
    print("✅ MCP Server connected\n" if init and "result" in init else "❌ Connection failed\n")

    # List all tools
    tools_list = c.call("tools/list")
    if tools_list and "result" in tools_list:
        tools = tools_list["result"].get("tools", [])
        print(f"📋 Available tools ({len(tools)}):")
        for t in tools:
            desc = t.get("description", "")[:70].split("\n")[0]
            print(f"   🔧 {t['name']:<35} │ {desc}")

    # ═══════════════════════════════════════════════════════════════
    # ① BƯỚC 1: SCAN THỊ TRƯỜNG
    # ═══════════════════════════════════════════════════════════════
    sep("① BƯỚC 1: SCAN THỊ TRƯỜNG — scan_market tool")
    print(text(c.tool("scan_market", {"min_score": 55})))

    # ═══════════════════════════════════════════════════════════════
    # ② BƯỚC 2: SENTIMENT
    # ═══════════════════════════════════════════════════════════════
    sep("② BƯỚC 2: SENTIMENT — fear_greed + whale_alerts + composite")
    print("  📊 Fear & Greed Index:")
    print(text(c.tool("get_fear_greed_index")))
    print("\n  🐋 Whale Alerts:")
    print(text(c.tool("get_whale_alerts", {"min_usd": 1000000})))
    print("\n  🧠 Composite Sentiment:")
    print(text(c.tool("get_composite_sentiment")))

    # ═══════════════════════════════════════════════════════════════
    # ③ BƯỚC 3: DEEP ANALYSIS — Top coins
    # ═══════════════════════════════════════════════════════════════
    coins = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
    for sym in coins:
        sep(f"③ PHÂN TÍCH SÂU: {sym}")

        # Giá hiện tại
        print(f"  💰 Live Price:")
        print(text(c.tool("get_crypto_price", {"symbol": sym})))

        # TA Indicators 1D
        print(f"\n  📊 Technical Indicators (1D):")
        print(text(c.tool("analyze_indicators", {"symbol": sym, "interval": "1d"})))

        # Trading Signals
        print(f"\n  🎯 Trading Signals:")
        print(text(c.tool("get_trading_signals", {"symbol": sym, "interval": "1d"})))

        # Regime Detection
        print(f"\n  🌊 Market Regime:")
        print(text(c.tool("detect_market_regime", {"symbol": sym, "interval": "1d"})))

        # Support/Resistance
        print(f"\n  📏 Support & Resistance:")
        print(text(c.tool("get_support_resistance", {"symbol": sym, "interval": "1d"})))

        # 4H timeframe
        print(f"\n  📈 4H Timeframe:")
        print(text(c.tool("analyze_indicators", {"symbol": sym, "interval": "4h"})))

    # ═══════════════════════════════════════════════════════════════
    # ④ BƯỚC 4: RISK ASSESSMENT
    # ═══════════════════════════════════════════════════════════════
    sep("④ BƯỚC 4: RISK — Position Sizing + Risk Metrics")

    print("  📐 Position Size (BTC, $10k equity, 1% risk):")
    print(text(c.tool("calculate_position_size", {
        "equity": 10000, "risk_pct": 1.0, "entry_price": 77500, "stop_price": 76000
    })))

    print("\n  📊 Risk Metrics (portfolio):")
    print(text(c.tool("compute_risk_metrics", {
        "equity": 10000, "positions": [
            {"symbol": "BTCUSDT", "entry": 77500, "current": 77500, "qty": 0.01, "side": "LONG"},
            {"symbol": "ETHUSDT", "entry": 2300, "current": 2300, "qty": 0.1, "side": "LONG"}
        ]
    })))

    print("\n  ✅ Risk Check:")
    print(text(c.tool("check_risk", {
        "equity": 10000, "max_drawdown_pct": 10, "daily_loss_limit_pct": 3
    })))

    # ═══════════════════════════════════════════════════════════════
    # ⑤ BƯỚC 5: POSITION ANALYZER — TỔNG HỢP
    # ═══════════════════════════════════════════════════════════════
    sep("⑤ BƯỚC 5: POSITION ANALYZER — TỔNG HỢP VI THẾ TỐT NHẤT")

    for sym in coins:
        print(f"\n  📋 {sym} — Full Position Analysis:")
        print(text(c.tool("analyze_position_full", {
            "symbol": sym, "quantity": 1.0, "entry_price": 0, "side": "LONG"
        })))

    sep("✅ HOÀN TẤT — Tất cả phân tích qua MCP Tools")
    print("  Tools đã gọi: scan_market, get_fear_greed_index, get_whale_alerts,")
    print("  get_composite_sentiment, get_crypto_price, analyze_indicators,")
    print("  get_trading_signals, detect_market_regime, get_support_resistance,")
    print("  calculate_position_size, compute_risk_metrics, check_risk,")
    print("  analyze_position_full\n")

    c.close()

if __name__ == "__main__":
    main()
