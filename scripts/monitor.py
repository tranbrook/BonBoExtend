#!/usr/bin/env python3
"""BonBo Position Monitor — Theo dõi vị thế ORDIUSDT real-time."""

import json
import subprocess
import sys
import os
import time
from datetime import datetime

# Config from .env
API_KEY = os.popen('source ~/BonBoExtend/.env && echo $BINANCE_API_KEY').read().strip()
API_SECRET = os.popen('source ~/BonBoExtend/.env && echo $BINANCE_API_SECRET').read().strip()

# Order IDs
ENTRY_ORDER_ID = 18928261846
SL_ALGO_ID = 3000001310414864
TP_ALGO_ID = 3000001310417660
SYMBOL = "ORDIUSDT"
ENTRY_PRICE = 4.55
SL_PRICE = 4.20
TP_PRICE = 5.10
QUANTITY = 122

def signed_get(endpoint, params_str):
    """Make a signed GET request to Binance."""
    ts = int(time.time() * 1000)
    full_params = f"{params_str}&timestamp={ts}&recvWindow=60000"
    sig_proc = subprocess.run(
        ['openssl', 'dgst', '-sha256', '-hmac', API_SECRET],
        input=full_params, capture_output=True, text=True
    )
    sig = sig_proc.stdout.strip().split()[-1]
    
    url = f"https://fapi.binance.com{endpoint}?{full_params}&signature={sig}"
    result = subprocess.run(
        ['curl', '-s', '--max-time', '8', '-H', f'X-MBX-APIKEY: {API_KEY}', url],
        capture_output=True, text=True
    )
    return json.loads(result.stdout)

def get_price():
    """Get current ORDIUSDT price."""
    proc = subprocess.run(
        ['curl', '-s', '--max-time', '5', 
         'https://fapi.binance.com/fapi/v1/ticker/price?symbol=ORDIUSDT'],
        capture_output=True, text=True
    )
    d = json.loads(proc.stdout)
    return float(d.get('price', 0))

def check_entry_order():
    """Check if entry limit order is still open."""
    try:
        d = signed_get("/fapi/v1/openOrders", f"symbol={SYMBOL}")
        if isinstance(d, list):
            for o in d:
                if o['orderId'] == ENTRY_ORDER_ID:
                    return o.get('status', 'UNKNOWN')
            # Not found in open orders = filled or cancelled
            return 'NOT_OPEN'
        return 'ERROR'
    except:
        return 'ERROR'

def check_order_status():
    """Query specific order status."""
    try:
        d = signed_get("/fapi/v1/order", f"symbol={SYMBOL}&orderId={ENTRY_ORDER_ID}")
        if isinstance(d, dict) and 'status' in d:
            return d['status'], float(d.get('executedQty', 0))
        return 'UNKNOWN', 0
    except:
        return 'ERROR', 0

def check_position():
    """Check ORDIUSDT position."""
    try:
        d = signed_get("/fapi/v2/positionRisk", f"symbol={SYMBOL}")
        if isinstance(d, list) and len(d) > 0:
            p = d[0]
            return {
                'qty': float(p.get('positionAmt', 0)),
                'entry': float(p.get('entryPrice', 0)),
                'mark': float(p.get('markPrice', 0)),
                'liq': float(p.get('liquidationPrice', 0)),
                'pnl': float(p.get('unRealizedProfit', 0)),
            }
        return None
    except:
        return None

def check_algo(algo_id):
    """Check algo order status."""
    try:
        d = signed_get("/fapi/v1/algoOrder", f"algoId={algo_id}")
        if isinstance(d, dict) and 'algoId' in d:
            return d.get('algoStatus', 'UNKNOWN')
        return 'UNKNOWN'
    except:
        return 'ERROR'

def pnl_bar(pnl_pct, width=20):
    """Create a visual PnL bar."""
    if pnl_pct >= 0:
        filled = min(int(pnl_pct / 2), width)
        return f"[{'🟢' * filled}{'⬜' * (width - filled)}] +{pnl_pct:.2f}%"
    else:
        filled = min(int(abs(pnl_pct) / 2), width)
        return f"[{'🔴' * filled}{'⬜' * (width - filled)}] {pnl_pct:.2f}%"

def main():
    print("╔══════════════════════════════════════════════════════════╗")
    print("║  🤖 BONBO POSITION MONITOR — ORDIUSDT LONG             ║")
    print("╠══════════════════════════════════════════════════════════╣")
    print(f"║  Entry:  ${ENTRY_PRICE:.2f}  |  SL: ${SL_PRICE:.2f}  |  TP: ${TP_PRICE:.2f}  ║")
    print(f"║  Qty: {QUANTITY} ORDI  |  Leverage: 3x  |  R:R = 1:1.57       ║")
    print("╚══════════════════════════════════════════════════════════╝")
    print()
    
    filled = False
    prev_status = None
    cycle = 0
    
    while True:
        cycle += 1
        now = datetime.now().strftime("%H:%M:%S")
        
        try:
            # Get current price
            price = get_price()
            
            if not filled:
                # Check entry order status
                status, exec_qty = check_order_status()
                
                if status == 'NEW':
                    # Still waiting
                    dist = ((price - ENTRY_PRICE) / ENTRY_PRICE) * 100
                    dist_emoji = "📈" if price > ENTRY_PRICE else "📉"
                    print(f"\r  ⏳ [{now}] #{cycle} ORDI=${price:.4f} | "
                          f"Entry ${ENTRY_PRICE} | {dist_emoji} {dist:+.2f}% from entry | "
                          f"SL:${SL_PRICE} TP:${TP_PRICE} | Waiting...", end='', flush=True)
                    
                elif status == 'FILLED':
                    filled = True
                    print()
                    print()
                    print("  " + "=" * 55)
                    print(f"  ✅ [{now}] ENTRY FILLED! 122 ORDI @ ${ENTRY_PRICE}")
                    print("  " + "=" * 55)
                    print()
                    print(f"  🛡️  SL Algo #{SL_ALGO_ID} @ ${SL_PRICE} — Active")
                    print(f"  🎯  TP Algo #{TP_ALGO_ID} @ ${TP_PRICE} — Active")
                    print()
                    
                elif status == 'PARTIALLY_FILLED':
                    print(f"\n  ⚡ [{now}] PARTIAL FILL: {exec_qty}/{QUANTITY} ORDI @ ${ENTRY_PRICE}")
                    
                elif status in ('CANCELED', 'CANCELLED', 'EXPIRED', 'REJECTED'):
                    print(f"\n  ❌ [{now}] Entry order {status}. Exiting monitor.")
                    break
                    
            if filled:
                # Monitor position
                pos = check_position()
                
                if pos and pos['qty'] != 0:
                    pnl = pos['pnl']
                    pnl_pct = (pnl / (ENTRY_PRICE * QUANTITY)) * 100
                    entry = pos['entry']
                    mark = pos['mark']
                    liq = pos['liq']
                    
                    # Distance to SL/TP
                    dist_sl = ((price - SL_PRICE) / price) * 100
                    dist_tp = ((TP_PRICE - price) / price) * 100
                    
                    # Check algo statuses
                    sl_status = check_algo(SL_ALGO_ID)
                    tp_status = check_algo(TP_ALGO_ID)
                    
                    pnl_emoji = "🟢" if pnl >= 0 else "🔴"
                    
                    print(f"\r  {pnl_emoji} [{now}] #{cycle} "
                          f"ORDI=${mark:.4f} | "
                          f"PnL: ${pnl:+.4f} ({pnl_pct:+.2f}%) | "
                          f"Liq: ${liq:.2f} | "
                          f"SL:{dist_sl:.1f}% TP:{dist_tp:.1f}% | "
                          f"SL:{sl_status[:3]} TP:{tp_status[:3]}  ", end='', flush=True)
                    
                    # Alert near SL or TP
                    if dist_sl < 2:
                        print(f"\n  ⚠️ [{now}] NEAR STOP-LOSS! Only {dist_sl:.1f}% away!")
                    if dist_tp < 2:
                        print(f"\n  🎯 [{now}] NEAR TAKE-PROFIT! Only {dist_tp:.1f}% away!")
                    
                else:
                    # Position closed
                    print()
                    print()
                    print("  " + "=" * 55)
                    print(f"  🔔 [{now}] POSITION CLOSED!")
                    print("  " + "=" * 55)
                    
                    # Check final PnL
                    try:
                        trades = signed_get("/fapi/v1/userTrades", f"symbol={SYMBOL}&limit=5")
                        if isinstance(trades, list):
                            total_pnl = sum(float(t.get('realizedPnl', 0)) for t in trades)
                            total_comm = sum(float(t.get('commission', 0)) for t in trades)
                            print(f"  Realized PnL: ${total_pnl:+.4f}")
                            print(f"  Commission:   -${total_comm:.4f}")
                            print(f"  Net:          ${total_pnl - total_comm:+.4f}")
                    except:
                        pass
                    
                    # Check if SL or TP triggered
                    sl_status = check_algo(SL_ALGO_ID)
                    tp_status = check_algo(TP_ALGO_ID)
                    print(f"  SL Algo: {sl_status}")
                    print(f"  TP Algo: {tp_status}")
                    print()
                    
                    # Check balance
                    try:
                        acc = signed_get("/fapi/v2/account", "")
                        if 'totalWalletBalance' in acc:
                            print(f"  Wallet: {acc['totalWalletBalance']} USDT")
                            print(f"  Available: {acc['availableBalance']} USDT")
                    except:
                        pass
                    
                    print()
                    print("  ✅ Monitor hoàn tất. Thoát...")
                    break
            
            time.sleep(5)  # Check every 5 seconds
            
        except KeyboardInterrupt:
            print("\n\n  ⏹️ Monitor dừng bởi user.")
            break
        except Exception as e:
            print(f"\n  ❌ Error: {e}")
            time.sleep(10)

if __name__ == "__main__":
    main()
