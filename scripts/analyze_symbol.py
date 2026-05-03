#!/usr/bin/env python3
"""Deep analysis of a single symbol — calls all MCP tools for comprehensive view."""
import subprocess, json, sys, time

MCP = "../target/release/bonbo-extend-mcp"
SYMBOL = sys.argv[1] if len(sys.argv) > 1 else "ORDIUSDT"

class C:
    def __init__(self):
        self.p = subprocess.Popen([MCP], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, bufsize=1)
        self.n = 0
        time.sleep(2)
    def t(self, name, args=None):
        self.n += 1
        req = {"jsonrpc": "2.0", "id": self.n, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}
        self.p.stdin.write(json.dumps(req) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line: return "No response"
        r = json.loads(line.strip())
        if "error" in r: return f"Error: {r['error'].get('message','?')}"
        parts = []
        for c in r.get("result", {}).get("content", []):
            if c.get("type") == "text": parts.append(c["text"])
        return "\n".join(parts) if parts else "Empty"
    def close(self):
        self.p.terminate(); self.p.wait(timeout=5)

def sec(title):
    print(f"\n{'═'*80}")
    print(f"  {title}")
    print(f"{'═'*80}\n")

c = C()
# Init
c.n += 1
req = {"jsonrpc": "2.0", "id": c.n, "method": "initialize",
       "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "a", "version": "1"}}}
c.p.stdin.write(json.dumps(req) + "\n")
c.p.stdin.flush()
c.p.stdout.readline()

sec(f"📊 PHÂN TÍCH CHUYÊN SÂU: {SYMBOL}")

# ── 1. PRICE ──
sec("① GIÁ HIỆN TẠI")
print(c.t("get_crypto_price", {"symbol": SYMBOL}))

# ── 2. TECHNICAL ANALYSIS (4 timeframes) ──
for tf, label in [("1d", "1 NGÀY (1D)"), ("4h", "4 GIỜ (4H)"), ("1h", "1 GIỜ (1H)"), ("15m", "15 PHÚT (15M)")]:
    sec(f"② TECHNICAL INDICATORS — {label}")
    print(c.t("analyze_indicators", {"symbol": SYMBOL, "interval": tf}))

# ── 3. TRADING SIGNALS (4 timeframes) ──
for tf, label in [("1d", "1D"), ("4h", "4H"), ("1h", "1H"), ("15m", "15M")]:
    sec(f"③ TRADING SIGNALS — {label}")
    print(c.t("get_trading_signals", {"symbol": SYMBOL, "interval": tf}))

# ── 4. REGIME DETECTION ──
sec("④ MARKET REGIME DETECTION")
for tf in ["1d", "4h", "1h", "15m"]:
    print(f"\n  ── {tf.upper()} ──")
    print(c.t("detect_market_regime", {"symbol": SYMBOL, "interval": tf}))

# ── 5. SUPPORT / RESISTANCE ──
sec("⑤ SUPPORT & RESISTANCE LEVELS")
for tf in ["1d", "4h", "1h"]:
    print(f"\n  ── {tf.upper()} ──")
    print(c.t("get_support_resistance", {"symbol": SYMBOL, "interval": tf}))

# ── 6. SENTIMENT ──
sec("⑥ SENTIMENT THỊ TRƯỜNG")
print(c.t("get_fear_greed_index"))
print()
print(c.t("get_composite_sentiment"))

# ── 7. RISK MANAGEMENT ──
sec("⑦ RISK MANAGEMENT — Position Sizing")
print("  📐 Position Size ($10k equity, 2% risk):")
print(c.t("calculate_position_size", {"equity": 10000, "risk_pct": 2.0, "entry_price": 4.78, "stop_price": 4.30}))
print()
print("  🛡️ Risk Check:")
print(c.t("check_risk", {"equity": 10000, "max_drawdown_pct": 10, "daily_loss_limit_pct": 3}))

# ── 8. POSITION ANALYZER ──
sec("⑧ POSITION ANALYZER — TỔNG HỢP")
print(c.t("analyze_position_full", {"symbol": SYMBOL, "quantity": 100, "entry_price": 4.78, "side": "LONG"}))

# ── 9. SUMMARY ──
sec("⑨ TỔNG HỢP KẾT QUẢ")
print(f"  ✅ Phân tích hoàn tất cho {SYMBOL}")
print(f"  15 tools đã gọi qua MCP Server")
c.close()
