#!/usr/bin/env python3
"""Place AXSUSDT LONG — adaptive to current price and available margin."""
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
        txt = ""
        for c in r.get("result", {}).get("content", []):
            if c.get("type") == "text": txt += c["text"] + "\n"
        print(txt.strip())
        return txt
    def close(self):
        self.p.terminate(); self.p.wait(timeout=5)

c = C()
c.call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "axs", "version": "1.0"}})

SYMBOL = "AXSUSDT"
CAPITAL = 100.0
LEVERAGE = 10

print("=" * 75)
print(f"  {SYMBOL} — ĐẶT LỆNH LONG ${CAPITAL:.0f} x{LEVERAGE}")
print("=" * 75)

# ── Step 0: Check balance ──
print("\n💰 Số dư:")
balance_txt = c.tool("futures_get_balance")

# ── Step 1: Get live price ──
print("\n📊 Giá AXS:")
price_txt = c.tool("get_crypto_price", {"symbol": SYMBOL})

# Parse actual price from output
import re
m = re.search(r'\$([0-9.]+)', price_txt)
live_price = float(m.group(1)) if m else 1.47

# Parse available balance
m2 = re.search(r'Available[^0-9]*([0-9.]+)', balance_txt)
available = float(m2.group(1)) if m2 else 71.0

print(f"\n  → Giá live: ${live_price:.4f}")
print(f"  → Available: ${available:.2f}")

# ── Step 2: Calculate safe quantity ──
# With 10x leverage: notional = margin * leverage
# margin needed = qty * price / leverage
# Available margin = $available (need to leave some buffer)
usable_margin = min(CAPITAL, available * 0.9)  # Use 90% of available, capped at $100
notional = usable_margin * LEVERAGE
quantity = int(notional / live_price)

# Ensure quantity doesn't exceed what margin allows
margin_needed = quantity * live_price / LEVERAGE
if margin_needed > available * 0.95:
    quantity = int(available * 0.9 * LEVERAGE / live_price)
    margin_needed = quantity * live_price / LEVERAGE
    usable_margin = margin_needed

print(f"\n📐 Tính toán thực tế:")
print(f"  Margin dùng:    ${usable_margin:.2f}")
print(f"  Leverage:       {LEVERAGE}x")
print(f"  Notional:       ${quantity * live_price:.2f}")
print(f"  Quantity:       {quantity} AXS")
print(f"  Entry (~):      ${live_price:.4f}")

# ── Step 3: Set leverage ──
print(f"\n⚙️  Set leverage {LEVERAGE}x:")
c.tool("futures_set_leverage", {"symbol": SYMBOL, "leverage": LEVERAGE})

# ── Step 4: Calculate SL/TP ──
# SL: 3.5% below entry (support-based)
# TP1: 5% above entry (R:R ~1.4:1)
# TP2: 10% above entry (R:R ~2.9:1)
sl = round(live_price * 0.965, 4)
tp1 = round(live_price * 1.05, 4)
tp2 = round(live_price * 1.10, 4)
tp1_qty = quantity // 2
tp2_qty = quantity - tp1_qty
risk = (live_price - sl) * quantity
reward = (tp2 - live_price) * quantity

print(f"\n  🛑 SL:   ${sl:.4f} (-3.5%)")
print(f"  🏆 TP1:  ${tp1:.4f} (+5.0%) — {tp1_qty} AXS (50%)")
print(f"  🏆 TP2:  ${tp2:.4f} (+10.0%) — {tp2_qty} AXS (50%)")
print(f"  ⚖️  R:R:  {(tp1-live_price)/(live_price-sl):.1f}:1 (TP1) / {(tp2-live_price)/(live_price-sl):.1f}:1 (TP2)")
print(f"  💰 Risk: ${risk:.2f} | Reward: ${reward:.2f}")

# ── Step 5: Place MARKET BUY ──
print(f"\n📌 Đặt MARKET BUY {quantity} AXS...")
order_result = c.tool("futures_place_order", {
    "symbol": SYMBOL,
    "side": "BUY",
    "quantity": str(quantity),
    "type": "MARKET"
})

if "❌" in str(order_result):
    print("\n⚠️  Không thể đặt lệnh. Dừng.")
    c.close()
    exit(1)

# ── Step 6: Wait for fill, then place SL/TP ──
time.sleep(2)

# ── Place STOP LOSS ──
print(f"\n🛑 Đặt Stop Loss @ ${sl}:")
c.tool("futures_place_order", {
    "symbol": SYMBOL,
    "side": "SELL",
    "quantity": str(quantity),
    "order_type": "STOP_MARKET",
    "stop_price": str(sl),
    "reduce_only": True
})

# ── Place TP1 (50%) ──
print(f"\n🏆 Đặt TP1 @ ${tp1} — {tp1_qty} AXS:")
c.tool("futures_place_order", {
    "symbol": SYMBOL,
    "side": "SELL",
    "quantity": str(tp1_qty),
    "order_type": "TAKE_PROFIT_MARKET",
    "stop_price": str(tp1),
    "reduce_only": True
})

# ── Place TP2 (50%) ──
print(f"\n🏆 Đặt TP2 @ ${tp2} — {tp2_qty} AXS:")
c.tool("futures_place_order", {
    "symbol": SYMBOL,
    "side": "SELL",
    "quantity": str(tp2_qty),
    "order_type": "TAKE_PROFIT_MARKET",
    "stop_price": str(tp2),
    "reduce_only": True
})

# ── Verify ──
print("\n📋 Vị thế hiện tại:")
c.tool("futures_get_positions")

print("\n📋 Lệnh đang mở:")
c.tool("futures_get_open_orders", {"symbol": SYMBOL})

# ── Final Summary ──
print(f"\n{'='*75}")
print(f"  ✅ HOÀN TẤT")
print(f"{'='*75}")
print(f"""
  ┌──────────────────────────────────────────────────────────┐
  │  📌 {SYMBOL} LONG — ${usable_margin:.0f} x{LEVERAGE} leverage              │
  │                                                          │
  │  Entry:      ~${live_price:.4f} (MARKET)                  │
  │  Quantity:   {quantity} AXS                                │
  │  Notional:   ${quantity*live_price:.2f}                       │
  │                                                          │
  │  🛑 SL:       ${sl:.4f} (-3.5%)                           │
  │  🏆 TP1:      ${tp1:.4f} (+5.0%)  → {tp1_qty} AXS (50%)        │
  │  🏆 TP2:      ${tp2:.4f} (+10.0%) → {tp2_qty} AXS (50%)        │
  │                                                          │
  │  ⚖️  R:R:      {(tp1-live_price)/(live_price-sl):.1f}:1 (TP1) / {(tp2-live_price)/(live_price-sl):.1f}:1 (TP2)              │
  │  💰 Max Risk:  ${risk:.2f}                                │
  │  🏆 Max Gain:  ${reward:.2f}                              │
  └──────────────────────────────────────────────────────────┘
""")

c.close()
