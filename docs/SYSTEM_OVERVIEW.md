# BonBoExtend — Tổng quan kiến trúc & vận hành

> Cập nhật: 2025-04-22 | Version: 0.1.0 | Status: Active Development

---

## 1. Giới thiệu

BonBoExtend là hệ thống **plugin framework** cho phép AI agent (BonBo) giao tiếp với Binance Futures qua MCP (Model Context Protocol). Hệ thống cung cấp 46 tools để phân tích thị trường, đặt lệnh, quản lý vị thế, và kiểm soát rủi ro.

---

## 2. Quy mô hệ thống

| Chỉ số | Giá trị |
|--------|---------|
| Rust crates | 17 |
| Rust source files | 134 |
| Tổng LOC (Rust) | ~28,000 |
| Python scripts | 15 |
| MCP Tools | 46 |
| Binary targets | 2 (MCP server + agent) |
| Examples | 6 |

---

## 3. Kiến trúc 4 tầng

```
┌─────────────────────────────────────────────────────────────┐
│  TẦNG 4: GIAO TIẾP (Interface Layer)                        │
│                                                              │
│  ┌─────────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ bonbo-extend-mcp │  │ Python       │  │ BonBo AI Agent │  │
│  │ (MCP STDIO)      │  │ Scripts      │  │ (LLM Chat)     │  │
│  │ 46 tools         │  │ 15 scripts   │  │                │  │
│  └────────┬─────────┘  └──────┬───────┘  └───────┬────────┘  │
└───────────┼────────────────────┼──────────────────┼──────────┘
            │                    │                  │
┌───────────┼────────────────────┼──────────────────┼──────────┐
│  TẦNG 3: PLUGIN FRAMEWORK                                  │
│           │                    │                  │          │
│  ┌────────▼────────────────────▼──────────────────▼────────┐ │
│  │ bonbo-extend — Plugin Registry + Tool Plugins            │ │
│  │                                                          │ │
│  │  📊 Thị trường: MarketData · TechAnalysis · Regime       │ │
│  │  🔍 Scan: Scanner · Sentinel · Validation                │ │
│  │  💰 Trading: TradingPlugin (11 tools)                    │ │
│  │  📈 Quant: Backtest · Risk · Learning · Journal          │ │
│  │  🔧 System: SystemMonitor · PriceAlert                   │ │
│  └───────────────────────────┬──────────────────────────────┘ │
└──────────────────────────────┼───────────────────────────────┘
                               │
┌──────────────────────────────┼───────────────────────────────┐
│  TẦNG 2: THƯ VIỆN CHUYÊN MÔN                               │
│                               │                              │
│  ┌────────────────────────────┤                              │
│  │ bonbo-ta (3633 LOC)        │  Hurst, ALMA, SuperSmoother  │
│  │ bonbo-data (1174 LOC)      │  Binance REST, candles, OB   │
│  │ bonbo-quant (3500 LOC)     │  Backtesting, strategies      │
│  │ bonbo-regime (673 LOC)     │  BOCPD + Hurst regime detect  │
│  │ bonbo-sentinel (1093 LOC)  │  Signal quality, confluence   │
│  │ bonbo-scanner (402 LOC)    │  Multi-market scanning        │
│  │ bonbo-risk (847 LOC)       │  Circuit breaker, pos sizing  │
│  │ bonbo-journal (1129 LOC)   │  Trade log, SQLite storage    │
│  │ bonbo-learning (671 LOC)   │  DMA learning, weight adapt   │
│  │ bonbo-validation (455 LOC) │  Signal verify, confluence    │
│  │ bonbo-funding (131 LOC)    │  Funding rate data            │
│  └────────────────────────────┘                              │
└──────────────────────────────────────────────────────────────┘
                               │
┌──────────────────────────────┼───────────────────────────────┐
│  TẦNG 1: EXECUTION CORE                                     │
│                               │                              │
│  ┌────────────────────────────▼────────────────────────────┐ │
│  │ bonbo-binance-futures (2105 LOC)                         │ │
│  │ REST: Account · Orders · AlgoOrders · Market              │ │
│  │ WebSocket: real-time prices + user data stream            │ │
│  │ Models: Order, Position, Balance, Leverage, Margin...     │ │
│  └──────────────┬───────────────────────┬───────────────────┘ │
│                 │                       │                     │
│  ┌──────────────▼──────────┐ ┌──────────▼──────────────────┐ │
│  │ bonbo-executor (543 LOC)│ │ bonbo-position-manager      │ │
│  │ Saga: Entry+SL+TP       │ │   (691 LOC)                 │ │
│  │ Compensate on failure   │ │ Tracker + Orphan cleanup     │ │
│  └─────────────────────────┘ │ Partial close, trailing stop │ │
│                               └─────────────────────────────┘ │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │ bonbo-agent (1263 LOC)                                    │ │
│  │ Decision loop → RiskGate → OrderExecutor → TrackPosition  │ │
│  │ MCP client → Signal analysis → Auto trade                 │ │
│  │ Kill switch · State machine · Orchestrator                 │ │
│  └──────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. Dependency Graph

```
bonbo-extend-mcp
  └── bonbo-extend
        ├── bonbo-ta
        ├── bonbo-data
        ├── bonbo-quant
        ├── bonbo-sentinel
        ├── bonbo-risk
        ├── bonbo-journal
        ├── bonbo-regime
        ├── bonbo-learning
        ├── bonbo-validation
        ├── bonbo-scanner
        └── bonbo-binance-futures  ← NEW (trading tools)

bonbo-agent
  ├── bonbo-binance-futures
  ├── bonbo-executor
  │     └── bonbo-binance-futures
  └── bonbo-position-manager
        └── bonbo-binance-futures
```

---

## 5. 46 MCP Tools — Chi tiết đầy đủ

### 5.1 Giá thị trường (4 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `get_crypto_price` | Giá crypto real-time + 24h change | `symbol` |
| `get_crypto_candles` | Biểu đồ nến OHLCV | `symbol`, `interval`, `limit` |
| `get_crypto_orderbook` | Sổ lệnh mua/bán | `symbol`, `limit` |
| `get_top_crypto` | Top crypto theo volume | `limit` |

### 5.2 Technical Analysis (4 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `analyze_indicators` | ALMA, SuperSmoother, RSI, MACD, CMO, BB, Hurst, LaguerreRSI | `symbol`, `interval` |
| `detect_market_regime` | BOCPD + Hurst → Trending/Ranging/Quiet/Volatile | `symbol`, `interval` |
| `get_trading_signals` | Composite buy/sell signals từ multi-indicator | `symbol`, `interval` |
| `get_support_resistance` | Support/resistance levels | `symbol`, `interval` |

### 5.3 Market Scanner (2 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `scan_market` | Scan multi-coin: regime + indicators + signals | `method`, `top_n` |
| `get_scan_schedule` | Lịch scan tự động | — |

### 5.4 Backtest (3 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `run_backtest` | Backtest chiến lược (alma_crossover, macd_crossover, laguerre_rsi) | `symbol`, `strategy`, `capital`, `interval`, `period` |
| `compare_strategies` | So sánh nhiều strategies | `symbol`, `capital`, `period` |
| `export_pinescript` | Xuất TradingView Pine Script | `strategy` |

### 5.5 Risk Management (3 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `check_risk` | Kiểm tra drawdown, risk level | `equity`, `initial_capital`, `peak_equity` |
| `calculate_position_size` | Tính cỡ vị thế theo ATR risk | `equity`, `risk_pct`, `atr`, `multiplier` |
| `compute_risk_metrics` | Sharpe, Sortino, Max DD, Win Rate | trades data |

### 5.6 Sentiment (4 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `get_composite_sentiment` | Composite score: FearGreed + technicals | — |
| `get_fear_greed_index` | Fear & Greed Index (0-100) | — |
| `get_whale_alerts` | Whale movement alerts | — |
| `validate_signal` | Đánh giá chất lượng signal | `symbol`, `direction`, `entry`, `stop`, `target` |

### 5.7 Learning (3 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `get_learning_stats` | DMA learning statistics | — |
| `get_learning_metrics` | Chi tiết metrics: weights, accuracy | — |
| `get_scoring_weights` | Trọng số hiện tại của scoring model | — |

### 5.8 Journal (3 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `journal_trade_entry` | Ghi nhận vào lệnh | `symbol`, `side`, `entry_price`, `quantity`, `strategy`, `reason` |
| `journal_trade_outcome` | Ghi nhận kết quả lệnh | `trade_id`, `exit_price`, `pnl`, `outcome` |
| `get_trade_journal` | Xem lịch sử giao dịch | `limit`, `symbol` |

### 5.9 🆕 Trading — Binance Futures trực tiếp (11 tools)

| Tool | Mô tả | Tham số bắt buộc |
|------|--------|-----------------|
| `futures_get_balance` | Xem số dư USDT (balance, available, PnL) | — |
| `futures_get_positions` | Danh sách vị thế đang mở | — |
| `futures_get_open_orders` | Lệnh đang mở theo symbol | `symbol` |
| `futures_place_order` | Đặt lệnh MARKET / LIMIT / STOP_MARKET / TRAILING_STOP_MARKET | `symbol`, `side`, `quantity`, `order_type` |
| `futures_cancel_orders` | Hủy tất cả lệnh (standard + SL/TP) | `symbol` |
| `futures_close_position` | Đóng vị thế (opposite market order + hủy SL/TP) | `symbol` |
| `futures_set_leverage` | Chỉnh đòn bẩy 1-125x | `symbol`, `leverage` |
| `futures_set_margin_type` | Chỉnh margin CROSSED / ISOLATED | `symbol`, `margin_type` |
| `futures_set_stop_loss` | Đặt Stop-Loss qua Algo API (full hoặc partial) | `symbol`, `trigger_price` + (`close_position` hoặc `quantity`) |
| `futures_set_take_profit` | Đặt Take-Profit qua Algo API | `symbol`, `trigger_price` |
| `futures_set_trailing_stop` | Đặt Trailing Stop qua Algo API | `symbol`, `callback_rate`, `quantity` |

### 5.10 Alerts (3 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `create_price_alert` | Tạo cảnh báo giá | `symbol`, `target_price`, `direction` |
| `list_price_alerts` | Xem tất cả alerts | — |
| `delete_price_alert` | Xóa alert | `alert_id` |

### 5.11 System (4 tools)

| Tool | Mô tả | Tham số |
|------|--------|---------|
| `system_health` | CPU, RAM, uptime, load | — |
| `check_port` | Kiểm tra port mở | `port`, `host` |
| `disk_usage` | Dung lượng ổ đĩa | — |
| `list_st...` | Liệt kê processes | — |

---

## 6. Qui trình vận hành — 3 chế độ

### 6.1 Chế độ 1: AI Agent tự động (`bonbo-agent`)

```
Scan (MCP) → Detect Signal → Risk Gate → Execute Saga → Track → Cleanup
     │              │              │            │           │          │
  Scanner      Validation     Circuit     Entry+SL+TP   PosMgr   OrphanCleaner
  Regime       Confluence     Breaker     (Saga)        Trailing  (Algo+Std)
```

- Agent chạy decision loop liên tục
- Tự động scan → phân tích → ra quyết định → đặt lệnh
- **Trạng thái: Chưa vận hành** — cần tích hợp MCP client hoàn thiện

### 6.2 Chế độ 2: MCP Tools thủ công (qua AI chat) — ĐANG VẬN HÀNH

```
Bạn: "Phân tích DOTUSDT"  →  analyze_indicators + detect_regime + get_trading_signals
Bạn: "Mua DOT x10"         →  futures_set_leverage → futures_set_margin_type → futures_place_order
Bạn: "Đặt SL/TP"           →  futures_set_stop_loss → futures_set_take_profit
Bạn: "Đóng vị thế"          →  futures_close_position
```

Cách gọi:
```bash
# Via MCP server (stdio)
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"futures_get_balance","arguments":{}},"id":1}' | \
  BONBO_EXTEND_LOG=error ./target/release/bonbo-extend-mcp
```

### 6.3 Chế độ 3: Python scripts

```
multi_tf_analysis.py      → Phân tích multi-timeframe top coins
trading_recommendation.py → Khuyến nghị giao dịch
quant_screener.py         → Sàng lọc quant
monitor_lite.py           → Theo dõi vị thế
self_learn_v2.py          → DMA learning loop
```

Gọi MCP tools qua subprocess (stdin/stdout JSON-RPC).

---

## 7. Build & Deploy

### 7.1 Build

```bash
cd ~/BonBoExtend

# Debug build (nhanh)
cargo build

# Release build (tối ưu)
cargo build --release

# Build chỉ MCP server
cargo build --release -p bonbo-extend-mcp

# Chạy tests
cargo test --workspace

# Lint
cargo clippy --workspace -- -W clippy::all
```

### 7.2 Environment Variables

```bash
# File .env tại ~/BonBoExtend/.env
BINANCE_API_KEY=your_api_key
BINANCE_API_SECRET=your_api_secret
BINANCE_TESTNET=false          # true cho testnet
BONBO_EXTEND_LOG=info          # log level
```

### 7.3 Khởi chạy

```bash
# Stdio mode (cho AI agent)
./target/release/bonbo-extend-mcp

# Với env vars
export $(grep -v '^#' ~/BonBoExtend/.env | xargs)
./target/release/bonbo-extend-mcp
```

---

## 8. Ví dụ End-to-End: Phân tích + Đặt lệnh

```bash
export $(grep -v '^#' ~/BonBoExtend/.env | xargs)
MCP=./target/release/bonbo-extend-mcp

# 1. Xem balance
echo '{"jsonrpc":"2.0","method":"initialize","id":0}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"futures_get_balance"},"id":1}' | $MCP

# 2. Phân tích kỹ thuật
echo '{"jsonrpc":"2.0","method":"initialize","id":0}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"analyze_indicators","arguments":{"symbol":"DOTUSDT"}},"id":1}' | $MCP

# 3. Set leverage
echo '{"jsonrpc":"2.0","method":"initialize","id":0}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"futures_set_leverage","arguments":{"symbol":"DOTUSDT","leverage":10}},"id":1}' | $MCP

# 4. Mua
echo '{"jsonrpc":"2.0","method":"initialize","id":0}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"futures_place_order","arguments":{"symbol":"DOTUSDT","side":"BUY","quantity":"100.0","order_type":"MARKET"}},"id":1}' | $MCP

# 5. Đặt SL
echo '{"jsonrpc":"2.0","method":"initialize","id":0}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"futures_set_stop_loss","arguments":{"symbol":"DOTUSDT","trigger_price":"1.250","side":"SELL","close_position":true}},"id":1}' | $MCP

# 6. Đặt TP
echo '{"jsonrpc":"2.0","method":"initialize","id":0}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"futures_set_take_profit","arguments":{"symbol":"DOTUSDT","trigger_price":"1.400","side":"SELL","close_position":true}},"id":1}' | $MCP
```

---

## 9. Thêm Plugin mới

### Bước 1: Tạo file plugin

`bonbo-extend/src/tools/my_plugin.rs`:

```rust
use async_trait::async_trait;
use crate::plugin::*;
use serde_json::{Value, json};

pub struct MyPlugin;

impl MyPlugin {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl ToolPlugin for MyPlugin {
    fn metadata(&self) -> &PluginMetadata {
        // Return plugin metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "my_tool".into(),
            description: "Description of my tool".into(),
            parameters: vec![/* ParameterSchema */],
        }]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &Value,
        _context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "my_tool" => { /* implement */ Ok("result".into()) }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
```

### Bước 2: Đăng ký

`bonbo-extend/src/tools/mod.rs`:
```rust
pub mod my_plugin;
pub use my_plugin::MyPlugin;
```

`bonbo-extend-mcp/src/main.rs`:
```rust
registry.register_tool_plugin(MyPlugin::new())?;
```

### Bước 3: Build & test

```bash
cargo build --release
cargo test --workspace
```

→ Tool mới tự động xuất hiện cho AI agent.

---

## 10. Lịch sử phát triển

| Phase | Nội dung | Status |
|-------|----------|--------|
| **Phase 1** | Plugin framework (bonbo-extend) + Market data tools | ✅ Hoàn thành |
| **Phase 2** | Technical analysis (bonbo-ta, bonbo-quant, bonbo-regime) | ✅ Hoàn thành |
| **Phase 3** | Scanner, Sentinel, Risk, Journal, Learning | ✅ Hoàn thành |
| **Phase 4** | Trading Plugin (11 tools: orders, positions, leverage, SL/TP) | ✅ Hoàn thành |
| **Phase 5** | Agent tự động (bonbo-agent + MCP client integration) | 🔄 Đang phát triển |

---

## 11. Bugs đã fix (Session 2025-04-22)

| Bug | File | Fix |
|-----|------|-----|
| `clientAlgoId` vượt 36 chars | `algo_orders.rs` | Rút ngắn UUID từ 32 → 8 chars |
| `cancel_all_orders` crash khi response là object | `orders.rs` | Handle cả object (`{code:200}`) và array |
| `FuturesBalance` thiếu `cross_wallet_balance` | `models.rs` | `#[serde(default)]` cho tất cả fields |
| `Leverage` thiếu `max_notional_value` | `models.rs` | `#[serde(default)]` |
| `AlgoOrderResponse` không parse Binance v2 response | `algo_orders.rs` | Thêm `algo_status`, `status` fields + `#[serde(default)]` |
| `is_success()` chỉ check code=="200" | `algo_orders.rs` | Check cả `algo_id > 0` |
| `reduceOnly` conflict với `closePosition` | `algo_orders.rs` | Tách logic: dùng `closePosition` khi không có quantity, `reduceOnly` khi có |
| Plugin chỉ hỗ trợ MARKET/LIMIT | `trading.rs` | Thêm STOP_MARKET, TRAILING_STOP_MARKET |
| `stop_loss` chỉ hỗ trợ closePosition | `trading.rs` | Thêm `quantity` param cho partial SL |
| DOT precision (step 0.1, tick 0.001) | `trading.rs` | Parse string quantities để giữ decimal format |
