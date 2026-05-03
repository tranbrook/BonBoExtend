#!/usr/bin/env python3
"""Place ORDIUSDT pullback order via MCP Server with SL/TP."""
import subprocess, json, sys, time, os

# Load Binance API keys from .env
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
            print(f"  ❌ Error: {r['error'].get('message','?')}")
            return None
        for c in r.get("result", {}).get("content", []):
            if c.get("type") == "text":
                print(c["text"])
        return r
    def close(self):
        self.p.terminate(); self.p.wait(timeout=5)

c = C()

# Init
c.call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "ordi-trader", "version": "1.0"}})

print("=" * 80)
print("  ORDIUSDT — ĐẶT LỆNH PULLBACK LONG")
print("  Entry: $4.65 | Capital: $100 | Leverage: 10x")
print("=" * 80)

# ── Step 0: Check balance ──
print("\n💰 Bước 0: Kiểm tra số dư...")
c.tool("futures_get_balance")

# ── Step 1: Check existing positions ──
print("\n📋 Bước 1: Kiểm tra vị thế hiện tại...")
c.tool("futures_get_positions")

# ── Step 2: Set leverage 10x ──
print("\n⚙️  Bước 2: Set leverage 10x cho ORDIUSDT...")
c.tool("futures_set_leverage", {"symbol": "ORDIUSDT", "leverage": 10})

# ── Step 3: Calculate position ──
# $100 x 10x = $1000 notional
# Entry $4.65 → quantity = $1000 / $4.65 = 215.05 → round to 215 ORDI
entry = 4.65
capital = 100.0
leverage = 10
notional = capital * leverage
quantity = int(notional / entry)
sl = 4.18   # S2 support on 4H + ATR-based
tp1 = 5.23  # R1 resistance on 4H
tp2 = 5.35  # R2 resistance on 4H

print(f"\n📐 Bước 3: Tính toán vị thế")
print(f"  Capital:    ${capital:.0f}")
print(f"  Leverage:   {leverage}x")
print(f"  Notional:   ${notional:.0f}")
print(f"  Entry:      ${entry:.2f}")
print(f"  Quantity:   {quantity} ORDI")
print(f"  Stop Loss:  ${sl:.2f} ({(sl/entry-1)*100:.1f}%)")
print(f"  TP1:        ${tp1:.2f} ({(tp1/entry-1)*100:+.1f}%)")
print(f"  TP2:        ${tp2:.2f} ({(tp2/entry-1)*100:+.1f}%)")
risk = (entry - sl) * quantity
reward1 = (tp1 - entry) * quantity
print(f"  Risk:       ${risk:.2f} ({risk/capital*100:.1f}% capital)")
print(f"  Reward TP1: ${reward1:.2f}")
print(f"  R:R (TP1):  {reward1/risk:.1f}:1")

# ── Step 4: Place LIMIT BUY order at $4.65 ──
print(f"\n📌 Bước 4: Đặt lệnh LIMIT BUY {quantity} ORDI @ ${entry}...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "BUY",
    "quantity": str(quantity),
    "type": "LIMIT",
    "price": str(entry)
})

# ── Step 5: Place STOP_MARKET (Stop Loss) at $4.18 ──
print(f"\n🛑 Bước 5: Đặt Stop Loss @ ${sl}...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "SELL",
    "quantity": str(quantity),
    "type": "STOP_MARKET",
    "stop_price": str(sl),
    "reduce_only": True
})

# ── Step 6: Place TAKE_PROFIT_MARKET TP1 at $5.23 (50% position) ──
tp1_qty = quantity // 2
print(f"\n🏆 Bước 6: Đặt TP1 @ ${tp1} ({tp1_qty} ORDI = 50% position)...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "SELL",
    "quantity": str(tp1_qty),
    "type": "TAKE_PROFIT_MARKET",
    "stop_price": str(tp1),
    "reduce_only": True
})

# ── Step 7: Place TAKE_PROFIT_MARKET TP2 at $5.35 (remaining 50%) ──
tp2_qty = quantity - tp1_qty
print(f"\n🏆 Bước 7: Đặt TP2 @ ${tp2} ({tp2_qty} ORDI = 50% remaining)...")
c.tool("futures_place_order", {
    "symbol": "ORDIUSDT",
    "side": "SELL",
    "quantity": str(tp2_qty),
    "type": "TAKE_PROFIT_MARKET",
    "stop_price": str(tp2),
    "reduce_only": True
})

# ── Step 8: Verify open orders ──
print("\n📋 Bước 8: Xác nhận tất cả lệnh đã đặt...")
c.tool("futures_get_open_orders", {"symbol": "ORDIUSDT"})

print("\n" + "=" * 80)
print("  ✅ HOÀN TẤT ĐẶT LỆNH")
print("=" * 80)
print(f"""
  📊 TỔNG HỢP LỆNH ORDIUSDT:

  ┌─────────────────────────────────────────────────────────┐
  │  📌 LIMIT BUY:    {quantity} ORDI @ ${entry:.2f}           │
  │     Notional:     ${notional:.0f} ({leverage}x)                    │
  │                                                       │
  │  🛑 STOP LOSS:    {quantity} ORDI @ ${sl:.2f} (-{(entry-sl)/entry*100:.1f}%)     │
  │     Max Loss:     ${risk:.2f}                           │
  │                                                       │
  │  🏆 TP1 (50%):    {tp1_qty} ORDI @ ${tp1:.2f} (+{(tp1-entry)/entry*100:.1f}%)    │
  │  🏆 TP2 (50%):    {tp2_qty} ORDI @ ${tp2:.2f} (+{(tp2-entry)/entry*100:.1f}%)    │
  │                                                       │
  │  ⚖️  R:R:          {reward1/risk:.1f}:1 (TP1)                          │
  │  💰 Max Risk:      ${risk:.2f} ({risk/capital*100:.1f}% capital)            │
  │  🏆 Max Reward:    ${(tp2-entry)*quantity:.2f}                        │
  └─────────────────────────────────────────────────────────┘

  ⚠️  Lưu ý: Lệnh LIMIT sẽ kích hoạt khi giá chạm ${entry}
  🛑 SL sẽ tự động kích hoạt nếu giá xuống ${sl}
  🏆 TP1+TP2 sẽ tự động chốt lời khi giá đạt target
  📊 Monitor tại: https://www.binance.com/en/futures/ORDIUSDT
""")

c.close()
