#!/usr/bin/env python3
"""BonBo Quick Monitor — Lightweight, runs in background."""

import json, subprocess, time, os
from datetime import datetime

API_KEY = ""
API_SECRET = ""

# Load .env manually
with open(os.path.expanduser("~/BonBoExtend/.env")) as f:
    for line in f:
        line = line.strip()
        if '=' in line and not line.startswith('#'):
            key, val = line.split('=', 1)
            if key == 'BINANCE_API_KEY':
                API_KEY = val
            elif key == 'BINANCE_API_SECRET':
                API_SECRET = val

BINANCE_API_KEY = API_KEY
BINANCE_API_SECRET = API_SECRET

def api_call(endpoint, params=""):
    ts = int(time.time() * 1000)
    p = f"{params}&timestamp={ts}&recvWindow=60000" if params else f"timestamp={ts}&recvWindow=60000"
    sig = subprocess.run(['openssl','dgst','-sha256','-hmac', BINANCE_API_SECRET],
        input=p, capture_output=True, text=True).stdout.strip().split()[-1]
    r = subprocess.run(['curl','-s','--max-time','5','-H',f'X-MBX-APIKEY: {BINANCE_API_KEY}',
        f"https://fapi.binance.com{endpoint}?{p}&signature={sig}"],
        capture_output=True, text=True)
    return json.loads(r.stdout)

ENTRY_ID = 18928261846
SL_ID = 3000001310414864
TP_ID = 3000001310417660
ENTRY_P = 4.55
SL_P = 4.20
TP_P = 5.10

last_status = None
last_price = 0
cycle = 0

while True:
    try:
        cycle += 1
        now = datetime.now().strftime("%H:%M:%S")
        
        # Price
        pr = subprocess.run(['curl','-s','--max-time','3',
            'https://fapi.binance.com/fapi/v1/ticker/price?symbol=ORDIUSDT'],
            capture_output=True, text=True)
        price = float(json.loads(pr.stdout)['price'])
        
        # Entry order
        order = api_call("/fapi/v1/order", f"symbol=ORDIUSDT&orderId={ENTRY_ID}")
        status = order.get('status','?')
        
        if status == 'NEW':
            dist = ((price - ENTRY_P) / ENTRY_P) * 100
            emoji = "📈" if price > ENTRY_P else "📉"
            trail = "⬇️ cần drop" if price > ENTRY_P else "⬆️ gần entry!"
            print(f"  ⏳ [{now}] ORDI=${price:.4f} | {emoji} {dist:+.2f}% | {trail} → ${ENTRY_P} | SL:${SL_P} TP:${TP_P}", flush=True)
        
        elif status == 'FILLED' and last_status != 'FILLED':
            print(f"\n  ✅✅✅ [{now}] ENTRY FILLED! 122 ORDI @ ${ENTRY_P}")
            print(f"  🛡️ SL @ ${SL_P} (Algo #{SL_ID})")
            print(f"  🎯 TP @ ${TP_P} (Algo #{TP_ID})")
            print()
        
        elif status in ('CANCELED','CANCELLED','EXPIRED'):
            print(f"\n  ❌ [{now}] Entry {status}. Monitor dừng.")
            break
        
        if status == 'FILLED':
            pos = api_call("/fapi/v2/positionRisk", "symbol=ORDIUSDT")
            if isinstance(pos, list) and len(pos) > 0:
                amt = float(pos[0].get('positionAmt', 0))
                if amt != 0:
                    pnl = float(pos[0]['unRealizedProfit'])
                    liq = float(pos[0].get('liquidationPrice', 0))
                    pnl_pct = (pnl / (ENTRY_P * 122)) * 100
                    e = "🟢" if pnl >= 0 else "🔴"
                    sl_dist = ((price - SL_P)/price)*100
                    tp_dist = ((TP_P - price)/price)*100
                    print(f"  {e} [{now}] ${price:.4f} | PnL: ${pnl:+.4f} ({pnl_pct:+.1f}%) | Liq:${liq:.2f} | SL:{sl_dist:.1f}% TP:{tp_dist:.1f}%", flush=True)
                    
                    if sl_dist < 1.5:
                        print(f"  ⚠️⚠️⚠️ GẦN STOP-LOSS! {sl_dist:.1f}%!")
                    if tp_dist < 1.5:
                        print(f"  🎯🎯🎯 GẦN TAKE-PROFIT! {tp_dist:.1f}%!")
                else:
                    # Position closed
                    trades = api_call("/fapi/v1/userTrades", "symbol=ORDIUSDT&limit=10")
                    total = sum(float(t.get('realizedPnl',0)) for t in trades) if isinstance(trades,list) else 0
                    comm = sum(float(t.get('commission',0)) for t in trades) if isinstance(trades,list) else 0
                    print(f"\n  🔔 [{now}] VỊ THẾ ĐÃ ĐÓNG!")
                    print(f"  PnL: ${total:+.4f} | Commission: -${comm:.4f} | Net: ${total-comm:+.4f}")
                    
                    acc = api_call("/fapi/v2/account")
                    print(f"  Wallet: {acc.get('totalWalletBalance','?')} USDT")
                    break
        
        last_status = status
        last_price = price
        time.sleep(10)
        
    except KeyboardInterrupt:
        print("\n  ⏹️ Dừng monitor.")
        break
    except Exception as e:
        print(f"  ❌ {e}")
        time.sleep(15)
