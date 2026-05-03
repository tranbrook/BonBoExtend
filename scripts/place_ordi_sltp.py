#!/usr/bin/env python3
"""Place SL/TP for ORDIUSDT pullback order via Algo Orders."""
import subprocess, json, time, os

env_path = os.path.join(os.path.dirname(__file__), "..", ".env")
if os.path.exists(env_path):
    with open(env_path) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                os.environ[k.strip()] = v.strip()

MCP = "../target/release/bonbo-extend-mcp"

class C:
    def __init__(self):
        self.p = subprocess.Popen([MCP], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, bufsize=1)
        self.n = 0
        time.sleep(2)
    def call(self, method, params=None):
        self.n += 1
        req = {"jsonrpc": "2.0", "id": self.n, "method": method, "params": params or {}}
        self.p.stdin.write(json.dumps(req) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        return json.loads(line.strip()) if line else None
    def tool(self, name, args=None):
        r = self.call("tools/call", {"name": name, "arguments": args or {}})
        if not r: return None
        if "error" in r:
            print(f"  ❌ {r['error'].get('message','?')}")
            return None
        for c in r.get("result", {}).get("content", []):
            if c.get("type") == "text": print(c["text"])
        return r
    def close(self):
        self.p.terminate(); self.p.wait(timeout=5)

c = C()
c.call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "ordi-sl-tp", "version": "1.0"}})

print("=" * 70)
print("  ORDIUSDT — ĐẶT STOP LOSS + TAKE PROFIT (Algo Orders)")
print("=" * 70)

# Params
quantity = 215
sl = 4.18
tp1 = 5.23
tp2 = 5.35
tp1_qty = quantity // 2  # 107
tp2_qty = quantity - tp1_qty  # 108

# Check open orders
print("\n📋 Lệnh hiện tại:")
c.tool("futures_get_open_orders", {"symbol": "ORDIUSDT"})

# ── Stop Loss via conditional algo order ──
print(f"\n🛑 Đặt STOP LOSS: {quantity} ORDI @ ${sl}...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "SELL",
    "quantity": str(quantity),
    "order_type": "STOP_MARKET",
    "stop_price": str(sl),
    "reduce_only": True
})

# ── TP1 via conditional algo order (50% position) ──
print(f"\n🏆 Đặt TP1: {tp1_qty} ORDI @ ${tp1} (50% position)...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "SELL",
    "quantity": str(tp1_qty),
    "order_type": "TAKE_PROFIT_MARKET",
    "stop_price": str(tp1),
    "reduce_only": True
})

# ── TP2 via conditional algo order (remaining 50%) ──
print(f"\n🏆 Đặt TP2: {tp2_qty} ORDI @ ${tp2} (50% remaining)...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "SELL",
    "quantity": str(tp2_qty),
    "order_type": "TAKE_PROFIT_MARKET",
    "stop_price": str(tp2),
    "reduce_only": True
})

# Verify
print("\n📋 Xác nhận tất cả lệnh:")
c.tool("futures_get_open_orders", {"symbol": "ORDIUSDT"})

c.close()
