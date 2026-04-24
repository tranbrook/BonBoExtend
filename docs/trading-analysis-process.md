# 🔄 BonBoExtend — Quy Trình Phân Tích Giao Dịch

> **Version:** 1.0 | **Created:** 2026-04-24 | **Author:** BonBo AI Agent
> **Status:** Active — cần cải thiện (xem research ở cuối)

---

## Tổng quan 5 bước

```
┌─────────────────────────────────────────────────────────────┐
│                 QUY TRÌNH PHÂN TÍCH 5 BƯỚC                  │
│                                                             │
│  ① SCAN ──→ ② SENTIMENT ──→ ③ DEEP ANALYSIS ──→           │
│       │            │               │                        │
│       ▼            ▼               ▼                        │
│   Lọc 15+      Fear/Greed     Indicators + Signals          │
│   coins        Whale alerts   + Regime + S/R                │
│                                                             │
│  ──→ ④ BACKTEST/RISK ──→ ⑤ RA QUYẾT ĐỊNH                  │
│           │                      │                          │
│           ▼                      ▼                          │
│     Kiểm chứng lịch sử     Entry/Exit/Size/SL/TP           │
└─────────────────────────────────────────────────────────────┘
```

---

## ① BƯỚC 1: SCAN THỊ TRƯỜNG RỘNG

**Mục tiêu:** Lọc nhanh từ hàng trăm coin xuống 10-15 coin tiềm năng

### Công cụ MCP

| Tool | Làm gì | Tại sao |
|---|---|---|
| `scan_market` | Quét 10 coins watchlist, tính Hurst score | Lọc nhanh coin nào có trend |
| `get_top_crypto` | Top 20-25 coins theo volume 24h | Phát hiện coin đang pump/dump bất thường |
| `get_crypto_price` | Giá real-time từng coin | Xác nhận giá hiện tại |

### Đầu ra

Bảng xếp hạng tất cả coins với Score, Regime, Hurst:

```
🟢 DOGEUSDT | Score: 63 | TrendingUp | H=0.64  → BUY
🟢 XRPUSDT  | Score: 62 | Ranging    | H=0.56  → BUY  
⚪ BTCUSDT  | Score: 51 | Quiet      | H=0.68  → HOLD
🔴 DOTUSDT  | Score: 40 | TrendingDown| H=0.63 → SELL
```

### Tiêu chí lọc

- Chỉ lấy coins có **Score ≥ 55** để phân tích sâu
- Hoặc coins **đang pump mạnh (+5%+)** để đánh giá FOMO vs trend thật
- Loại bỏ coins có **volume < $1M/24h** (thanh khoản kém)

---

## ② BƯỚC 2: SENTIMENT THỊ TRƯỜNG

**Mục tiêu:** Hiểu bối cảnh thị trường chung — Bull hay Bear?

### Công cụ MCP

| Tool | Dữ liệu | Ý nghĩa |
|---|---|---|
| `get_fear_greed_index` | Fear & Greed (0-100) | <30 = cơ hội mua, >70 = thận trọng |
| `get_composite_sentiment` | Composite score (-1 → +1) | Tổng hợp nhiều nguồn |
| `get_whale_alerts` | Giao dịch lớn >$1M | Whale đang mua hay bán? |

### Quyết định dựa trên sentiment

```
Fear (<40)  → Tìm cơ hội BUY (contrarian) nhưng SL chặt
Neutral     → Tuân theo technical, không bias
Greed (>70) → Thận trọng, có thể counter-trend SHORT
```

### Whale Alert Interpretation

```
Outflow (exchange → cold wallet) = HOLD signal → bullish
Inflow  (cold wallet → exchange) = SELL pressure → bearish
USDT inflow = tiền chuẩn bị mua → neutral-to-bullish
```

---

## ③ BƯỚC 3: DEEP ANALYSIS — Phân Tích Kỹ Thuật Chuyên Sâu

Đây là bước quan trọng nhất. Mỗi coin tiềm năng được phân tích bằng **4 tool song song**.

### 3A. Technical Indicators (`analyze_indicators`)

Tính toán **15+ indicators** trên 2 nhóm:

#### Nhóm Traditional

| Indicator | Mục đích | Cách đọc |
|---|---|---|
| SMA(20) | Trend baseline | Giá > SMA = bullish |
| EMA(12), EMA(26) | Trend direction | EMA12 > EMA26 = bullish |
| RSI(14) | Overbought/Oversold | >70 = overbought, <30 = oversold |
| MACD(12,26,9) | Momentum + crossover | MACD > Signal = bullish |
| Bollinger Bands(20,2) | Volatility + price position | %B > 0.8 = near top, < 0.2 = near bottom |

#### Nhóm Financial-Hacker (nâng cao)

| Indicator | Mục đích | Tại sao quan trọng |
|---|---|---|
| **ALMA(10,30)** | Smooth moving average crossover | Tín hiệu sớm hơn EMA, ít false signal |
| **SuperSmoother(20)** | Ehlers noise filter | Xác nhận trend direction qua slope |
| **Hurst Exponent(100)** | **QUAN TRỌNG NHẤT** | Phân biệt Trending vs Random Walk vs Mean-Reverting |
| CMO(14) | Chande Momentum Oscillator | Momentum thuần, không bị lag |
| LaguerreRSI(0.8) | RSI nâng cao | Phát hiện overbought/oversold tốt hơn RSI thường |

### 3B. Trading Signals (`get_trading_signals`)

Tổng hợp tất cả indicators thành **tín hiệu BUY/SELL/NEUTRAL** với confidence %:

```
🟢 BUY  [ALMA(10,30)]  70%  — ALMA10 > ALMA30 bullish cross (+3.43%)
🟢 BUY  [MACD]         60%  — MACD bullish crossover  
🔴 SELL [CMO(14)]      65%  — CMO 51.1 extremely overbought → contrarian SELL
🔴 SELL [LaguerreRSI]  70%  — Overbought >0.8
```

### 3C. Market Regime Detection (`detect_market_regime`)

**Đây là khác biệt lớn nhất** của BonBoExtend so với phân tích thông thường.

Hurst Exponent quyết định chiến lược:

```
┌──────────────────────────────────────────────────────┐
│ Hurst > 0.55  → TRENDING                            │
│   → Dùng Trend-Following (ALMA, SuperSmoother)      │
│   → SL rộng, trailing stop                          │
│                                                      │
│ Hurst ≈ 0.45-0.55 → RANDOM WALK                     │
│   → TRÁNH hoặc giảm size (không predictable)        │
│   → Không dùng trend-following                       │
│                                                      │
│ Hurst < 0.45 → MEAN-REVERTING                       │
│   → Dùng Mean-Reversion (BB bounce, RSI)            │
│   → SL chặt, target BB middle                       │
└──────────────────────────────────────────────────────┘
```

### 3D. Support/Resistance (`get_support_resistance`)

Xác định **entry, SL, TP** levels từ cấu trúc giá thực tế:

```
Resistance: R1: $356 (+4.1%)  ← Take Profit
Support:    S1: $318 (-7.0%)  ← Stop Loss
            S2: $311 (-9.1%)  ← Emergency exit
```

### Multi-Timeframe Confirmation

Mỗi coin được phân tích trên **nhiều timeframe**:

```
1D (Daily)  → Xu hướng chính (trend direction)
4H          → Entry timing (pullback identification)  
1H          → Entry chính xác
```

**Quy tắc:** Chỉ vào lệnh khi **ít nhất 2/3 timeframe đồng thuận**.

Ví dụ BTCUSDT:

| TF | Hurst | Regime | Kết luận |
|---|---|---|---|
| 1D | 0.638 | 📈 Trending UP | Xu hướng tăng |
| 4H | 0.405 | 🔄 Mean-Reverting | Đang pullback → cơ hội mua |
| 1H | 0.666 | 📈 Trending | Trend tiếp tục |

---

## ④ BƯỚC 4: BACKTEST & RISK ASSESSMENT

**Mục tiêu:** Kiểm chứng chiến lược trên dữ liệu lịch sử

### Công cụ MCP

| Tool | Làm gì |
|---|---|
| `run_backtest` | Chạy backtest SMA crossover trên dữ liệu thật |
| `compare_strategies` | So sánh nhiều chiến lược |
| `calculate_position_size` | Tính position size theo risk % |
| `check_risk` | Circuit breaker — có nên giao dịch không? |
| `validate_strategy` | CPCV cross-validation (gold standard) |

### Metrics quan trọng

```
Sharpe Ratio > 1.0   → Tốt
Profit Factor > 2.0   → Rất tốt  
Win Rate > 50%        → Acceptable
Max Drawdown < 10%    → An toàn
```

### Position Sizing Rule

```
Risk per trade = 1-2% equity
Position Size = (Equity × Risk%) / (Entry - StopLoss)
Leverage = chỉ dùng khi Hurst > 0.55 (trending)
```

---

## ⑤ BƯỚC 5: RA QUYẾT ĐỊNH

Tổng hợp tất cả dữ liệu thành quyết định cuối cùng.

### Scoring Matrix

```
Điểm tổng hợp = Weighted Score:

Factor                          Weight    Đánh giá
──────────────────────────────────────────────────
Hurst Exponent (>0.55 trending)  25%      Hurst value
Signals đồng thuận (BUY count)   20%      # BUY vs SELL
Multi-timeframe confirm          20%      2/3 TF đồng thuận
Sentiment phù hợp                15%      Fear/Greed + Whales
Backtest profitable              10%      Sharpe, PF
Risk/Reward > 1:2                10%      Entry→TP vs Entry→SL
──────────────────────────────────────────────────
TỔNG                             100%     → QUIẾT ĐỊNH
```

### 3 Kịch bản đầu ra

| Kịch bản | Điều kiện | Hành động |
|---|---|---|
| **🟢 VÀO LỆNH** | Score ≥ 65, Hurst trending, 2+ TF confirm | Entry + SL + TP |
| **🟡 CHỜ** | Score 50-65, mixed signals | Đặt alert, chờ pullback |
| **🔴 TRÁNH** | Score < 50, Random Walk, Fear + whale bán | Không giao dịch |

---

## 🛠️ Tổng kết Tools Pipeline

```
47 MCP Tools được sử dụng theo luồng:

Bước 1 — SCAN:
  scan_market ──→ Lọc 10 coins
  get_top_crypto ──→ Thêm coins pump mạnh
  get_crypto_price ──→ Xác nhận giá

Bước 2 — SENTIMENT:
  get_fear_greed_index ──→ Bối cảnh sentiment
  get_composite_sentiment ──→ Sentiment tổng hợp
  get_whale_alerts ──→ Smart money flow

Bước 3 — DEEP ANALYSIS (per coin):
  analyze_indicators ──→ 15+ indicators
  get_trading_signals ──→ BUY/SELL signals + confidence
  detect_market_regime ──→ Hurst → chiến lược phù hợp
  get_support_resistance ──→ Entry/SL/TP levels

Bước 4 — BACKTEST & RISK:
  run_backtest ──→ Kiểm chứng lịch sử
  calculate_position_size ──→ Sizing theo risk
  check_risk ──→ Circuit breaker

Bước 5 — EXECUTION:
  futures_get_balance ──→ Kiểm tra vốn
  futures_get_positions ──→ Vị thế hiện tại
  futures_smart_execute ──→ Thực thi lệnh (OFI → Flash Limit)
  futures_set_stop_loss ──→ Đặt SL (Algo API)
  futures_set_take_profit ──→ Đặt TP (Algo API)
```

---

## So sánh với phân tích thông thường

| | Phân tích thường | BonBoExtend |
|---|---|---|
| **Regime detection** | Không có | Hurst Exponent → chọn chiến lược đúng |
| **Financial-Hacker indicators** | Không có | ALMA, SuperSmoother, LaguerreRSI, CMO |
| **Multi-timeframe** | Thủ công | Tự động 1H/4H/1D |
| **Confidence scoring** | Chủ quan | Mỗi signal có % confidence |
| **Execution** | Thủ công | Smart Execute (OFI → Flash Limit → optimal price) |
| **Sentiment** | Cảm tính | Fear/Greed + Whale Alerts quantified |
| **Risk management** | Chủ quan | Circuit breaker + Position sizing + SL/TP auto |

---

## 🔬 Vấn đề đã biết & Cần cải thiện

### Vấn đề hiện tại (phát hiện qua thực tế sử dụng)

1. **Watchlist cố định 10 coins** — scan_market chỉ quét watchlist mặc định, bỏ lỡ nhiều coins pump mạnh (KAT, MOVR, SPK)
2. **Không có volume profile** — thiếu phân tích volume at price
3. **Hurst divergence không được xử lý** — khi short-term Hurst khác long-term (VD: 0.53 vs 0.36), chưa có hướng dẫn rõ
4. **Backtest chỉ có SMA crossover** — thiếu chiến lược ALMA crossover, SuperSmoother slope, RSI mean-reversion
5. **Sentiment đơn giản** — chỉ Fear/Greed + Whale (simulated), thiếu on-chain data thật
6. **Không có correlation analysis** — chưa biết coins có tương quan gì với nhau
7. **Weight trong Scoring Matrix chủ quan** — 25/20/20/15/10/10 chưa được tối ưu hóa
8. **LaguerreRSI hay bị overbought** — thường = 1.0, không phân biệt được mức độ
9. **SL/TP chỉ dựa trên S/R** — chưa tính ATR-based stops
10. **Không có portfolio-level analysis** — đánh giá từng coin riêng, không xem tương quan portfolio

### Điểm mạnh

1. ✅ Hurst-based regime detection — unique và hiệu quả
2. ✅ Multi-timeframe confirmation — giảm false signals
3. ✅ Financial-Hacker indicators — ALMA, SuperSmoother tốt hơn traditional
4. ✅ Smart Execute — tối ưu giá entry
5. ✅ End-to-end từ scan → analysis → execution

---

> **Xem research cải thiện:** `docs/research/trading-process-improvement.md`
