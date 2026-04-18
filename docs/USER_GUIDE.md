# 📖 BonBoExtend v0.2.0 — Hướng Dẫn Sử Dụng Chi Tiết

**Phiên bản:** v0.2.0  
**Ngày:** 2026-04-18  
**Tác giả:** BonBo AI Team  

---

## Mục Lục

1. [Tổng Quan](#1-tổng-quan)
2. [Cài Đặt & Build](#2-cài-đặt--build)
3. [Khởi Chạy MCP Server](#3-khởi-chạy-mcp-server)
4. [31 MCP Tools — Tham Chiếu Đầy Đủ](#4-31-mcp-tools--tham-chiếu-đầy-đủ)
5. [Self-Learning Loop — Hướng Dẫn Từng Bước](#5-self-learning-loop--hướng-dẫn-từng-bước)
6. [Ví Dụ Thực Tế — Chu Kỳ Học Hoàn Chỉnh](#6-ví-dụ-thực-tế--chu-kỳ-học-hoàn-chỉnh)
7. [Kiến Trúc Self-Learning](#7-kiến-trúc-self-learning)
8. [Troubleshooting](#8-troubleshooting)

---

## 1. Tổng Quan

BonBoExtend là **nền tảng phân tích crypto định lượng** với **khả năng tự học** (self-learning). Hệ thống ghi nhận mọi phân tích, so sánh với kết quả thực tế, và tự động cải thiện accuracy qua thời gian.

### 12 Crates

| Crate | Chức năng | Phase |
|-------|-----------|-------|
| `bonbo-ta` | 10 chỉ báo kỹ thuật (SMA, EMA, RSI, MACD, BB, ATR, ADX, Stochastic, CCI, VWAP) | A |
| `bonbo-data` | Binance REST + WebSocket, SQLite cache | B |
| `bonbo-quant` | Event-driven backtesting engine | C |
| `bonbo-sentinel` | Fear & Greed Index, whale alerts, composite sentiment | D |
| `bonbo-risk` | Position sizing (Fixed%, Kelly), CVaR/VaR, circuit breaker | E/F |
| **`bonbo-journal`** | **Trade journal, analysis snapshots, learning metrics** | **1** |
| **`bonbo-regime`** | **BOCPD regime detection, 5 market regimes** | **2** |
| **`bonbo-learning`** | **DMA Bayesian weight adaptation** | **3** |
| **`bonbo-validation`** | **CPCV, DSR, PBO, Haircut Sharpe** | **4** |
| **`bonbo-scanner`** | **Autonomous market scanner, schedules** | **5** |
| `bonbo-extend` | Plugin framework + tool registry | Core |
| `bonbo-extend-mcp` | HTTP + stdio MCP server | Core |

---

## 2. Cài Đặt & Build

### Yêu cầu
- Rust 1.75+ (`rustup update stable`)
- SQLite3 (tự động compile qua `rusqlite/bundled`)
- Internet connection (cho Binance API)

### Build

```bash
# Clone repository
cd ~/BonBoExtend

# Build toàn bộ workspace (debug)
cargo build --workspace

# Build release (khuyến nghị)
cargo build --release -p bonbo-extend-mcp

# Chạy tất cả tests
cargo test --workspace

# Kiểm tra binary
ls -lh target/release/bonbo-extend-mcp
# → ~8.5MB
```

### Verify

```bash
cargo test --workspace 2>&1 | grep "test result"
# → Kết quả: 155 passed; 0 failed
```

---

## 3. Khởi Chạy MCP Server

### HTTP Mode (khuyến nghị cho testing)

```bash
# Khởi chạy trên port 9876
./target/release/bonbo-extend-mcp --http --port 9876

# Hoặc chạy background
setsid ./target/release/bonbo-extend-mcp --http --port 9876 \
  </dev/null >/tmp/bonbo-mcp.log 2>&1 &
```

### Stdio Mode (cho AI Agent tích hợp)

```bash
# Claude Desktop / BonBo AI Agent
./target/release/bonbo-extend-mcp

# JSON-RPC qua stdin/stdout
echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}' | ./target/release/bonbo-extend-mcp
```

### Test kết nối

```bash
# Ping
curl -s -H "Content-Type: application/json" http://localhost:9876/ \
  -d '{"jsonrpc":"2.0","method":"ping","id":1}'

# Liệt kê tất cả tools
curl -s -H "Content-Type: application/json" http://localhost:9876/ \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":1}' | python3 -m json.tool
```

### Biến môi trường

| Biến | Mặc định | Mô tả |
|------|----------|-------|
| `BONBO_JOURNAL_DB` | `bonbo_journal.db` | Đường dẫn SQLite journal |
| `BONBO_CACHE_DB` | `bonbo_cache.db` | Đường dẫn SQLite cache |
| `RUST_LOG` | `info` | Log level (`debug`, `trace`) |

---

## 4. 31 MCP Tools — Tham Chiếu Đầy Đủ

### 📊 Phase A: Technical Analysis (7 tools)

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `get_crypto_price` | Giá hiện tại 1 symbol | `symbol` |
| `get_top_crypto` | Top N crypto theo volume | `limit` |
| `analyze_indicators` | Phân tích 10 chỉ báo kỹ thuật | `symbol`, `timeframe`, `limit` |
| `get_trading_signals` | Tín hiệu mua/bán tổng hợp | `symbol` |
| `detect_market_regime` | Phát hiện regime (từ bonbo-ta) | `symbol` |
| `get_support_resistance` | Mức hỗ trợ/kháng cự | `symbol`, `timeframe` |
| `system_status` | Thông tin hệ thống | — |

### 📈 Phase C: Backtesting (1 tool)

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `run_backtest` | Chạy backtest strategy | `symbol`, `strategy`, `timeframe`, `initial_capital` |

### 📰 Phase D: Sentiment (3 tools)

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `get_fear_greed_index` | Fear & Greed Index | — |
| `get_whale_alerts` | Whale transactions | `min_usd` |
| `get_composite_sentiment` | Tổng hợp sentiment | — |

### 🛡️ Phase E/F: Risk Management (2 tools)

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `calculate_position_size` | Tính vị thế | `equity`, `entry_price`, `stop_loss`, `method`, `risk_pct` |
| `compute_risk_metrics` | Tính VaR, Sharpe, Sortino | `trade_pnls`, `equity_curve` |
| `check_risk` | Kiểm tra circuit breaker | `equity`, `initial_capital`, `peak_equity`, `daily_pnl`, ... |

### 📝 **Phase 1: Trade Journal (4 tools)**

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `journal_trade_entry` | Ghi nhận phân tích + quyết định | `symbol`, `price`, `recommendation`, `quant_score`, `stop_loss`, `target_price` |
| `journal_trade_outcome` | Cập nhật kết quả thực tế | `entry_id`, `exit_price`, `direction_correct` |
| `get_trade_journal` | Truy vấn journal | `symbol`, `limit` |
| `get_learning_metrics` | Thống kê accuracy, Sharpe, PF | — |

### 🔄 **Phase 2: Regime Detection (1 tool)**

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `detect_market_regime` | BOCPD regime detection | `symbol`, `timeframe` |

### 🧠 **Phase 3: Learning Engine (2 tools)**

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `get_scoring_weights` | Xem weights hiện tại (default + per-regime) | — |
| `get_learning_stats` | DMA model stats | — |

### 🔬 **Phase 4: Validation (1 tool)**

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `validate_strategy` | CPCV + DSR + PBO validation | `returns`, `n_groups` |

### 🔍 **Phase 5: Scanner (2 tools)**

| Tool | Mô tả | Parameters |
|------|-------|------------|
| `scan_market` | Quét thị trường + chấm điểm | `min_score`, `symbols` |
| `get_scan_schedule` | Xem lịch scan | — |

---

## 5. Self-Learning Loop — Hướng Dẫn Từng Bước

### 5.1 Tổng Quan Chu Kỳ Học

```
┌──────────────────────────────────────────────────────────────────┐
│                    SELF-LEARNING LOOP                             │
│                                                                   │
│  Bước 1: QUÉT THỊ TRƯỜNG                                        │
│    scan_market → Phát hiện cơ hội                                │
│         │                                                         │
│  Bước 2: PHÂN TÍCH CHI TIẾT                                     │
│    analyze_indicators + detect_market_regime + get_trading_signals│
│         │                                                         │
│  Bước 3: GHI NHẬN VÀO JOURNAL                                   │
│    journal_trade_entry → Lưu toàn bộ snapshot phân tích           │
│         │                                                         │
│  Bước 4: ĐỢI KẾT QUẢ                                            │
│    (Thời gian thực — 4h, 8h, 24h...)                            │
│         │                                                         │
│  Bước 5: GHI NHẬN KẾT QUẢ                                       │
│    journal_trade_outcome → So sánh dự đoán vs thực tế            │
│         │                                                         │
│  Bước 6: HỌC TỪ DỮ LIỆU                                         │
│    get_learning_metrics → DMA cập nhật weights tự động           │
│         │                                                         │
│  └─────→ Lặp lại từ Bước 1 ──────────────────────────────────────│
└──────────────────────────────────────────────────────────────────┘
```

### 5.2 Bước 1: Quét Thị Trường

```bash
# Quét top crypto, chỉ hiện những coin có score ≥ 55
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "scan_market",
    "arguments": {"min_score": 55}
  },
  "id": 1
}'
```

**Kết quả:**
```
🔍 **Scan** | 5 symbols | Regime: Ranging

🟢🟢 SOLUSDT 74 (STRONG_BUY) $150 TrendingUp
🟢 BTCUSDT 68 (BUY) $77066 Ranging
🟢 ETHUSDT 62 (BUY) $2410 Ranging
```

### 5.3 Bước 2: Phân Tích Chi Tiết

```bash
# Phân tích kỹ thuật
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "analyze_indicators",
    "arguments": {"symbol": "BTCUSDT", "timeframe": "4h", "limit": 100}
  },
  "id": 1
}'

# Phát hiện regime
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "detect_market_regime",
    "arguments": {"symbol": "BTCUSDT"}
  },
  "id": 2
}'

# Tín hiệu giao dịch
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "get_trading_signals",
    "arguments": {"symbol": "BTCUSDT"}
  },
  "id": 3
}'
```

### 5.4 Bước 3: Ghi Nhận Vào Journal

```bash
# Sau khi phân tích xong, ghi nhận quyết định
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "journal_trade_entry",
    "arguments": {
      "symbol": "BTCUSDT",
      "price": 77066,
      "recommendation": "BUY",
      "quant_score": 68,
      "stop_loss": 74500,
      "target_price": 81000
    }
  },
  "id": 1
}'
```

**Kết quả:**
```
📝 **Trade Entry**
ID: `970ce9e6-07c8-45e7-8018-e6dab73af400`
BTCUSDT @ $77066.00
BUY | Score: 68
SL: $74500.00 TP: $81000.00 R:R 2.5
```

> ⚠️ **QUAN TRỌNG:** Lưu lại `ID` — cần dùng để ghi nhận kết quả sau!

### 5.5 Bước 4: Đợi Kết Quả

Đợi theo timeframe phân tích:
- **4h timeframe:** Đợi 12-24 giờ
- **1h timeframe:** Đợi 4-8 giờ
- **1d timeframe:** Đợi 3-7 ngày

### 5.6 Bước 5: Ghi Nhận Kết Quả Thực Tế

```bash
# Giả sử BTC đã lên 79500 → dự đoán BUY đúng ✅
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "journal_trade_outcome",
    "arguments": {
      "entry_id": "970ce9e6-07c8-45e7-8018-e6dab73af400",
      "exit_price": 79500,
      "direction_correct": true
    }
  },
  "id": 1
}'
```

**Kết quả:**
```
✅ **Outcome**: `970ce9e6...` exit $79500.00 ret +3.16%
```

### 5.7 Bước 6: Xem Learning Metrics

```bash
# Xem metrics sau nhiều trades
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "get_learning_metrics",
    "arguments": {}
  },
  "id": 1
}'
```

**Kết quả (sau 4 trades):**
```
📊 **Learning Metrics**
Predictions: 4 | Outcomes: 4
Direction: 75.0% | Win: 50.0% | Avg Ret: +0.88%
Sharpe: 5.35 | PF: 1.37 | Recent10: 75.0%
```

### 5.8 Xem Weights & DMA State

```bash
# Xem scoring weights cho từng regime
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {"name": "get_scoring_weights", "arguments": {}},
  "id": 1
}'

# Xem DMA model performance
curl -s -H "Content-Type: application/json" http://localhost:9876/ -d '{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {"name": "get_learning_stats", "arguments": {}},
  "id": 1
}'
```

---

## 6. Ví Dụ Thực Tế — Chu Kỳ Học Hoàn Chỉnh

### Kịch bản: Phân tích BTC hàng ngày trong 1 tuần

#### Ngày 1: Phát hiện cơ hội

```bash
# 1. Quét thị trường
scan_market(min_score=55)
# → BTCUSDT score=68 BUY

# 2. Phân tích chi tiết
analyze_indicators(symbol="BTCUSDT", timeframe="4h")
# → RSI: 45 (neutral), MACD: bullish crossover, BB: middle band

get_trading_signals(symbol="BTCUSDT")
# → 3 Buy, 1 Sell → Net: Bullish

detect_market_regime(symbol="BTCUSDT")
# → Ranging (confidence 65%)

# 3. Ghi nhận
journal_trade_entry(
  symbol="BTCUSDT", price=77000, recommendation="BUY",
  quant_score=68, stop_loss=74500, target_price=81000
)
# → ID: "abc-123"
```

#### Ngày 3: Kiểm tra kết quả

```bash
# BTC hiện tại $79500
get_crypto_price(symbol="BTCUSDT")
# → $79,500

# 4. Ghi nhận kết quả
journal_trade_outcome(
  entry_id="abc-123", exit_price=79500, direction_correct=true
)
# → ✅ Return +3.25%
```

#### Ngày 5: Thêm vài trades

```bash
# Phân tích ETH
journal_trade_entry(symbol="ETHUSDT", price=2410, recommendation="BUY",
                    quant_score=62, stop_loss=2300, target_price=2600)

# ... thời gian trôi ...

journal_trade_outcome(entry_id="def-456", exit_price=2350,
                      direction_correct=false)
# → ❌ Return -2.49%
```

#### Ngày 7: Review học

```bash
# Xem tổng kết sau 1 tuần
get_learning_metrics()
# → Direction: 75.0% | Win: 50.0% | Sharpe: 5.35 | PF: 1.37

get_scoring_weights()
# → Default weights (chưa đủ 20 trades để DMA bắt đầu tune)

# Xem tất cả entries
get_trade_journal(limit=50)
# → Danh sách tất cả trades với trạng thái ✅❌⏳
```

#### Sau 20+ trades: DMA bắt đầu học

DMA engine tự động:
1. **So sánh** mỗi indicator's prediction với actual outcome
2. **Tăng weight** cho indicators dự đoán đúng (RSI đúng → tăng rsi_weight)
3. **Giảm weight** cho indicators dự đoán sai
4. **Regime-specific:** Learning riêng cho từng regime (Ranging vs Trending)

---

## 7. Kiến Trúc Self-Learning

### 7.1 DMA — Dynamic Model Averaging

```
Input: indicator_accuracy = {"RSI": true, "MACD": false, "BB": true, ...}

Step 1: Update model posterior
  RSI: recent_accuracy = 0.99 × 0.65 + 0.01 × 1.0 = 0.6535
  MACD: recent_accuracy = 0.99 × 0.65 + 0.01 × 0.0 = 0.6435

Step 2: Compute likelihoods
  RSI likelihood = 0.6535 (higher → more weight)
  MACD likelihood = 0.6435 (lower → less weight)

Step 3: Update weights with forgetting factor λ=0.99
  new_rsi_weight = 0.99 × old_rsi × 0.6535 = ...
  new_macd_weight = 0.99 × old_macd × 0.6435 = ...

Step 4: Normalize (sum = 1.0)
  Apply min weight 3%, max change 5%

Result: RSI weight tăng, MACD weight giảm
```

### 7.2 BOCPD — Regime Detection

```
Input: closes = [50000, 50100, 50200, ..., 48000, 47500]

                    Stable phase         Regime change!
                    ──────────────── ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
Returns:           0.001 0.002 0.001    -0.04 -0.05 -0.03

BOCPD tracks:      P(run_length=0) ←──────────── Spike here!
                    ↓
                   Change point detected!
                   prev_regime → TrendingUp
                   new_regime → TrendingDown
```

### 7.3 Validation Pipeline

```
Strategy Returns ──→ CPCV (C(6,2)=15 combinations)
                      │
                      ├→ Mean OOS Sharpe: 1.8
                      ├→ Sharpe Std: 0.3
                      │
                      ├→ Deflated Sharpe Ratio: 0.93
                      │   (corrected for 5 strategies tested)
                      │
                      ├→ PBO: 0.15
                      │   (15% chance selected strategy is overfitted)
                      │
                      ├→ Haircut Sharpe: 0.9
                      │   (50% discount: original 1.8 → 0.9)
                      │
                      └→ Significant: ✅ Yes (DSR > 0.95, PBO < 0.3)
```

### 7.4 4-Layer Anti-Overfitting

```
Layer 1: Pre-Deployment
  ▸ DSR corrects for multiple testing
  ▸ PBO checks if best strategy is overfitted
  ▸ Haircut Sharpe discounts by 50%

Layer 2: Model Architecture
  ▸ Ensemble (DMA) naturally resists overfitting
  ▸ Bayesian priors with domain knowledge
  ▸ Min weight per indicator: 3%

Layer 3: Online Safeguards
  ▸ Conservative learning rates (α=0.99)
  ▸ Accuracy < 45% → revert to defaults
  ▸ Max weight change: 5% per cycle

Layer 4: Monitoring
  ▸ Rolling Sharpe of predictions
  ▸ Weight change audit log
  ▸ Minimum 20 outcomes before first tuning
```

---

## 8. Troubleshooting

### Lỗi: "Journal entry not found"

```
Nguyên nhân: entry_id không chính xác (truncated)
Giải pháp: Dùng get_trade_journal để xem full ID, hoặc
           truy vấn trực tiếp SQLite:
           sqlite3 bonbo_journal.db "SELECT id FROM journal_entries WHERE symbol='BTCUSDT';"
```

### Lỗi: "Address already in use"

```
Nguyên nhân: MCP server đã chạy trên port đó
Giải pháp:
  fuser -k 9876/tcp    # Kill process
  sleep 1
  # Restart server
```

### Lỗi: "Outcomes already exists"

```
Nguyên nhân: Đã ghi nhận kết quả cho entry này rồi
Giải pháp: Mỗi entry chỉ được ghi nhận 1 outcome.
           Tạo entry mới cho trade tiếp theo.
```

### Learning metrics không cải thiện

```
Nguyên nhân: Cần tối thiểu 20 outcomes để DMA bắt đầu tune
Giải pháp: Tiếp tục ghi nhận trades. Sau 20+ outcomes,
           DMA sẽ tự động điều chỉnh weights.
```

### Xóa database để bắt đầu lại

```bash
rm bonbo_journal.db
# MCP server sẽ tự tạo database mới khi journal_trade_entry được gọi
```

---

## 9. API Format

Tất cả tools sử dụng JSON-RPC 2.0:

```json
// Request
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "<tool_name>",
    "arguments": { ... }
  },
  "id": 1
}

// Response (success)
{
  "jsonrpc": "2.0",
  "result": {
    "content": [{ "type": "text", "text": "..." }]
  },
  "id": 1
}

// Response (error)
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Tool execution failed: ..."
  },
  "id": 1
}
```

---

## 10. Performance

| Metric | Giá trị |
|--------|---------|
| Single candle processing | **~42ns** |
| SMA 10K candles | **34µs** (292 Melem/s) |
| Full analysis 10K | **287µs** (34.8 Melem/s) |
| MCP tool response | **<50ms** (real API) |
| Binary size (release) | **8.5MB** |
| Memory | **~15MB** idle |

---

*BonBoExtend v0.2.0 — Self-Learning Crypto Trading Platform*
*12 crates • 31 MCP tools • 155 tests • 0 warnings*
