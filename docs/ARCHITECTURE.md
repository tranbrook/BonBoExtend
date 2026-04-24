# BonBoExtend — Technical Architecture (Rust Focus)

> **Version**: 0.1.0 | **Edition**: Rust 2024 | **Last Updated**: 2026-04-23
> **33,056 LOC** | **372 tests** | **17 crates** | **6.7 MB release binary**

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Workspace Architecture](#2-workspace-architecture)
3. [Layer Model](#3-layer-model)
4. [Crate Catalog](#4-crate-catalog)
5. [Dependency Graph](#5-dependency-graph)
6. [Key Traits & Interfaces](#6-key-traits--interfaces)
7. [Execution Engine Deep Dive](#7-execution-engine-deep-dive)
8. [Data Flow](#8-data-flow)
9. [Error Handling Strategy](#9-error-handling-strategy)
10. [Concurrency Model](#10-concurrency-model)
11. [Testing Architecture](#11-testing-architecture)
12. [Build & Release](#12-build--release)
13. [Rust Quality Metrics](#13-rust-quality-metrics)

---

## 1. System Overview

BonBoExtend is a production-grade algorithmic trading platform for crypto futures (Binance),
built entirely in Rust with an MCP (Model Context Protocol) server interface for AI agent integration.

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         AI Agent (LLM)                                  │
│                    (BonBo Core / Claude / GPT)                          │
└───────────────────────────┬──────────────────────────────────────────────┘
                            │ JSON-RPC / MCP Protocol
                            ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                     bonbo-extend-mcp (6.7MB)                            │
│                   MCP Server Binary (stdio transport)                   │
└───────────────────────────┬──────────────────────────────────────────────┘
                            │
┌───────────────────────────▼──────────────────────────────────────────────┐
│                       bonbo-extend (Plugin Framework)                    │
│  ┌─────────────┐ ┌──────────────┐ ┌─────────────┐ ┌─────────────────┐  │
│  │ ToolPlugin  │ │ServicePlugin │ │ McpClient    │ │ Strategy        │  │
│  │ trait       │ │ trait        │ │ trait        │ │ trait           │  │
│  └─────────────┘ └──────────────┘ └─────────────┘ └─────────────────┘  │
└───────────────────────────┬──────────────────────────────────────────────┘
                            │
        ┌───────────────────┼───────────────────────────┐
        ▼                   ▼                           ▼
┌───────────────┐ ┌──────────────────┐ ┌─────────────────────────┐
│  bonbo-ta     │ │ bonbo-quant      │ │ bonbo-executor          │
│  Indicators   │ │ Statistics       │ │ TWAP/VWAP/POV/IS/OFI   │
│  RSI/MACD/... │ │ Hurst/MC/WF      │ │ Market Impact / Risk    │
└───────────────┘ └──────────────────┘ └─────────────────────────┘
        │                   │                           │
        ▼                   ▼                           ▼
┌───────────────────────────────────────────────────────────────────────┐
│                   bonbo-binance-futures (Transport)                   │
│                   REST API + WebSocket + Authentication               │
└───────────────────────────────────────────────────────────────────────┘
                            │
                            ▼
                   Binance Futures API
```

---

## 2. Workspace Architecture

**`Cargo.toml`** — virtual workspace with 17 crates:

```toml
[workspace]
members = [
    "bonbo-extend",            # Plugin framework + MCP tools
    "bonbo-extend-mcp",        # MCP server binary
    "bonbo-ta",                # Technical analysis indicators
    "bonbo-data",              # Market data structures
    "bonbo-quant",             # Quantitative analysis
    "bonbo-sentinel",          # Anomaly detection
    "bonbo-risk",              # Risk management
    "bonbo-journal",           # Trade journaling
    "bonbo-regime",            # Market regime detection
    "bonbo-learning",          # Adaptive learning
    "bonbo-validation",        # Strategy validation
    "bonbo-scanner",           # Market scanning
    "bonbo-binance-futures",   # Binance API client
    "bonbo-executor",          # Execution algorithms
    "bonbo-position-manager",  # Position management
    "bonbo-agent",             # Agent orchestration
    "bonbo-funding",           # Funding rate analysis
]
resolver = "2"
```

**Workspace Dependencies** (shared versions):

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | 1 (full) | Async runtime |
| `serde` / `serde_json` | 1 | Serialization |
| `anyhow` / `thiserror` | 1 / 2 | Error handling |
| `tracing` | 0.1 | Structured logging |
| `chrono` | 0.4 (serde) | Timestamps |
| `rust_decimal` | 1 (serde) | Financial math |
| `reqwest` | 0.12 (json) | HTTP client |
| `tokio-tungstenite` | 0.26 (native-tls) | WebSocket |
| `async-trait` | 0.1 | Async traits |
| `uuid` | 1 (v4) | Unique IDs |

---

## 3. Layer Model

The architecture follows a strict 5-layer dependency hierarchy:

```
LAYER 5: ORCHESTRATION   ← bonbo-extend-mcp, bonbo-extend, bonbo-agent
    ↑
LAYER 4: RISK & EXECUTION ← bonbo-executor, bonbo-risk, bonbo-position-manager
    ↑
LAYER 3: INTELLIGENCE     ← bonbo-sentinel, bonbo-regime, bonbo-learning, bonbo-scanner, bonbo-validation
    ↑
LAYER 2: CORE LIBRARIES   ← bonbo-ta, bonbo-data, bonbo-quant, bonbo-journal
    ↑
LAYER 1: TRANSPORT        ← bonbo-binance-futures
```

**Rule**: Lower layers NEVER depend on upper layers.

---

## 4. Crate Catalog

| Crate | LOC | Tests | Layer | Purpose |
|-------|-----|-------|-------|---------|
| `bonbo-binance-futures` | 2,105 | 2 | 1 | REST + WebSocket client |
| `bonbo-ta` | 3,700 | 51 | 2 | Technical indicators |
| `bonbo-data` | 1,174 | 28 | 2 | Market data structs |
| `bonbo-quant` | 3,806 | 55 | 2 | Hurst, Monte Carlo, statistics |
| `bonbo-regime` | 673 | 6 | 3 | Market regime detection |
| `bonbo-sentinel` | 1,093 | 35 | 3 | Anomaly detection |
| `bonbo-learning` | 671 | 13 | 3 | Adaptive learning |
| `bonbo-scanner` | 402 | 5 | 3 | Market scanning |
| `bonbo-validation` | 455 | 4 | 3 | Strategy validation |
| `bonbo-risk` | 847 | 26 | 4 | Risk management |
| `bonbo-executor` | 8,154 | 119 | 4 | Execution algorithms |
| `bonbo-position-manager` | 691 | 6 | 4 | Position tracking |
| `bonbo-journal` | 1,129 | 7 | 5 | Trade journaling |
| `bonbo-funding` | 131 | 1 | 5 | Funding rate analysis |
| `bonbo-agent` | 1,263 | 0 | 5 | Agent orchestration |
| `bonbo-extend` | 6,432 | 16 | 5 | Plugin framework |
| `bonbo-extend-mcp` | 330 | 0 | 5 | MCP server binary |
| **TOTAL** | **33,056** | **372** | | |

---

## 5. Dependency Graph

```
bonbo-extend-mcp ──→ bonbo-extend

bonbo-extend ──→ bonbo-ta
              ──→ bonbo-data
              ──→ bonbo-quant
              ──→ bonbo-sentinel
              ──→ bonbo-risk
              ──→ bonbo-journal
              ──→ bonbo-regime
              ──→ bonbo-learning
              ──→ bonbo-validation
              ──→ bonbo-scanner
              ──→ bonbo-binance-futures

bonbo-agent ──→ bonbo-binance-futures
            ──→ bonbo-executor
            ──→ bonbo-position-manager
            ──→ bonbo-risk
            ──→ bonbo-data

bonbo-executor ──→ bonbo-binance-futures
               ──→ bonbo-data

bonbo-quant ──→ bonbo-ta
            ──→ bonbo-data

bonbo-data ──→ bonbo-ta

bonbo-learning ──→ bonbo-journal
               ──→ bonbo-regime

bonbo-scanner ──→ bonbo-journal
              ──→ bonbo-regime
              ──→ bonbo-learning

bonbo-validation ──→ bonbo-journal
                ──→ bonbo-regime
                ──→ bonbo-learning

bonbo-position-manager ──→ bonbo-binance-futures
                      ──→ bonbo-risk

bonbo-funding ──→ bonbo-binance-futures
```

---

## 6. Key Traits & Interfaces

### 6.1 Plugin System

```rust
// bonbo-extend/src/plugin.rs
pub trait ToolPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn tools(&self) -> Vec<Tool>;
    async fn call(&self, tool: &str, args: Value) -> anyhow::Result<Value>;
}

pub trait ServicePlugin: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
}
```

### 6.2 Technical Analysis

```rust
// bonbo-ta/src/lib.rs
pub trait IncrementalIndicator: Send + Sync {
    fn update(&mut self, value: f64) -> Option<f64>;
    fn reset(&mut self);
}

pub trait CandleIndicator: Send + Sync {
    fn update_candle(&mut self, candle: &Candle) -> Option<f64>;
}
```

### 6.3 Strategy

```rust
// bonbo-quant/src/strategy.rs
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    fn analyze(&self, data: &[Candle]) -> Signal;
}
```

### 6.4 Execution

```rust
// bonbo-executor/src/execution_algo.rs
pub trait OrderPlacer: Send + Sync {
    async fn place_market(&self, symbol: &str, side: Side, qty: Decimal) -> Result<FillResult>;
    async fn place_limit(&self, symbol: &str, side: Side, qty: Decimal, price: Decimal) -> Result<FillResult>;
    async fn get_orderbook(&self, symbol: &str) -> Result<OrderBookSnapshot>;
}

// bonbo-executor/src/pov.rs
pub trait TradeFetcher: Send + Sync {
    async fn fetch_agg_trades(&self, symbol: &str, limit: u32) -> Result<Vec<Value>>;
}

// bonbo-executor/src/vwap.rs
pub trait KlineFetcher: Send + Sync {
    async fn fetch_klines(&self, symbol: &str, interval: &str, limit: u32) -> Result<Vec<Value>>;
}

// bonbo-executor/src/order_executor.rs
pub trait OrderExecutor: Send + Sync {
    async fn execute(&self, order: Order) -> Result<ExecutionResult>;
}
```

### 6.5 MCP Client

```rust
// bonbo-extend/src/mcp_client.rs
pub trait McpClient: Send + Sync {
    async fn call_tool(&self, tool: &str, args: Value) -> Result<Value>;
}
```

---

## 7. Execution Engine Deep Dive

The `bonbo-executor` crate is the largest and most sophisticated component (8,154 LOC, 119 tests).

### 7.1 Algorithm Suite

| Algorithm | File | LOC | Tests | Use Case |
|-----------|------|-----|-------|----------|
| **TWAP** | `twap.rs` | 948 | 17 | Small orders, equal time slicing |
| **VWAP** | `vwap.rs` | 1,162 | 14 | Medium orders, volume-weighted |
| **POV** | `pov.rs` | 1,048 | 16 | Large orders, tape-hiding |
| **IS** | `is.rs` | 950 | 14 | Optimal trajectory (Almgren-Chriss) |
| **OFI** | `ofi.rs` | 937 | 16 | Depth-imbalance sniping |
| **Adaptive Limit** | `execution_algo.rs` | 675 | 7 | Limit-first with market sweep |
| **Smart Selection** | `smart_execution.rs` | 355 | 6 | Auto-select best algo |
| **Saga** | `saga.rs` | 337 | 0 | Multi-exchange orchestration |

### 7.2 Shared Infrastructure

| Component | File | LOC | Tests | Purpose |
|-----------|------|-----|-------|---------|
| **Order Book** | `orderbook.rs` | 561 | 9 | L2 snapshot, slippage estimation |
| **Market Impact** | `market_impact.rs` | 599 | 13 | Square-root law, SAR, cascade |
| **Risk Guards** | `risk_guards.rs` | 295 | 5 | Kill switch, pre-trade, limits |
| **Order Builder** | `order_builder.rs` | 80 | 0 | Type-safe order construction |
| **Idempotency** | `idempotency.rs` | 83 | 0 | Duplicate order prevention |
| **Dry Run** | `dry_run.rs` | 33 | 0 | Simulated execution |

### 7.3 Execution Flow

```
Order Request → Pre-Trade Risk Check → Algo Selection → Execution Loop → Report
                      │                      │                │
                      ▼                      ▼                ▼
               kill_switch?          size/participation?   spread_gate?
               limit_breached?       TWAP/VWAP/POV/IS/OFI  slippage_est?
               budget_ok?                                 retry_logic?
```

### 7.4 Risk Gates (applied to ALL algorithms)

1. **Kill Switch**: Global on/off → immediate abort
2. **Pre-Trade Check**: Notional limit, concentration limit, daily loss limit
3. **Spread Gate**: Pause at 3× normal spread, abort at 5×
4. **Slippage Gate**: Estimate impact, reject if > max_slippage_bps
5. **Cumulative Risk**: Track total notional, commissions across slices

---

## 8. Data Flow

### 8.1 Market Data Pipeline

```
Binance API → bonbo-binance-futures::RestMarket
           → Raw JSON klines/trades/depth
           → bonbo-data::Candle / Trade / OrderBook
           → bonbo-ta::IncrementalIndicator.update()
           → Signal generation
```

### 8.2 Execution Pipeline

```
Decision → OrderBuilder → OrderPlacer trait
        → PreTradeCheck → Risk gates
        → Algo-specific loop (TWAP/VWAP/POV/IS/OFI)
        → OrderPlacer.place_market() / place_limit()
        → FillResult → ExecutionReport
        → Journal → Storage
```

### 8.3 Analysis Pipeline

```
Market Data → bonbo-ta (indicators)
           → bonbo-quant (Hurst, MC, stats)
           → bonbo-regime (Bull/Bear/Range/Volatile)
           → bonbo-sentinel (anomaly scores)
           → bonbo-learning (parameter adaptation)
           → bonbo-validation (backtest)
```

---

## 9. Error Handling Strategy

### 9.1 Two-Tier Model

| Layer | Error Type | Strategy |
|-------|-----------|----------|
| Library crates | Custom enums | `thiserror` derive |
| Application | Generic | `anyhow::Result<T>` |

### 9.2 Pattern

```rust
// Library: typed errors
#[derive(Debug, thiserror::Error)]
pub enum ImpactError {
    #[error("insufficient depth: {available} < {required}")]
    InsufficientDepth { available: f64, required: f64 },
    #[error("spread too wide: {spread_bps}bps > {max_bps}bps")]
    SpreadTooWide { spread_bps: f64, max_bps: f64 },
}

// Application: anyhow propagation
pub async fn execute_twap(...) -> anyhow::Result<TwapReport> {
    let book = placer.get_orderbook(symbol).await
        .context("TWAP: failed to fetch orderbook")?;
    ...
}
```

### 9.3 Metrics

- `anyhow::Result` used in **171** function signatures
- `thiserror` derive in custom error types
- **Zero** `unwrap()` in library code (tests only)
- All fallible operations use `?` or explicit match

---

## 10. Concurrency Model

### 10.1 Async Runtime

- **Runtime**: `tokio` with `full` features
- **241 async functions** across codebase
- **6 `tokio::spawn`** call sites for concurrent tasks

### 10.2 Shared State

| Pattern | Count | Use Case |
|---------|-------|----------|
| `Arc<Mutex<T>>` | 5 | Mutable shared state (risk tracker, kill switch) |
| `Arc<RwLock<T>>` | 15 | Read-heavy shared state (config, cache) |
| `Arc<T>` | 24 | Immutable shared state |

### 10.3 Thread Safety

All trait objects use `Send + Sync` bounds:
```rust
pub trait ToolPlugin: Send + Sync { ... }
pub trait OrderPlacer: Send + Sync { ... }
pub trait McpClient: Send + Sync { ... }
```

---

## 11. Testing Architecture

### 11.1 Test Distribution

| Crate | Tests | Focus |
|-------|-------|-------|
| bonbo-executor | 119 | Algorithm logic, risk gates, serialization |
| bonbo-quant | 55 | Hurst exponent, Monte Carlo, statistics |
| bonbo-ta | 51 | Indicator correctness, edge cases |
| bonbo-sentinel | 35 | Anomaly detection, threshold logic |
| bonbo-data | 28 | Candle aggregation, data parsing |
| bonbo-risk | 26 | Position sizing, risk limits |
| bonbo-extend | 16 | Plugin registration, MCP protocol |
| bonbo-regime | 6 | Regime classification |
| bonbo-position-manager | 6 | PnL calculation |
| bonbo-learning | 13 | Parameter adaptation |
| Others | 17 | Misc |
| **TOTAL** | **372** | |

### 11.2 Test Patterns

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indicator_edge_case() {
        // Unit test with known inputs/outputs
    }

    #[test]
    fn test_serialization_roundtrip() {
        // Verify JSON serialize/deserialize
        let json = serde_json::to_string(&data).unwrap();
        let back: T = serde_json::from_str(&json).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn test_mathematical_property() {
        // Verify invariants (e.g., RSI ∈ [0, 100])
        assert!(result >= 0.0 && result <= 100.0);
    }
}
```

### 11.3 Test Execution

```bash
cargo test --workspace              # All 372 tests
cargo test -p bonbo-executor        # 119 executor tests
cargo test -p bonbo-executor twap   # Just TWAP tests
cargo test -- --nocapture           # With stdout
```

---

## 12. Build & Release

### 12.1 Release Profile

```toml
[profile.release]
opt-level = 3        # Maximum optimization
lto = true           # Link-time optimization
strip = true         # Strip debug symbols
panic = "abort"      # Smaller binary, no unwind
codegen-units = 1    # Maximum optimization
```

### 12.2 Build Commands

```bash
cargo build --release -p bonbo-extend-mcp   # MCP server (6.7 MB)
cargo build --release --workspace            # All crates
cargo clippy --all-targets                   # Lint
cargo test --workspace                       # Test
```

### 12.3 Build Output

```
target/release/bonbo-extend-mcp  → 6.7 MB
Compile time (incremental): ~0.3s
Compile time (clean build): ~45s
```

---

## 13. Rust Quality Metrics

| Metric | Value | Assessment |
|--------|-------|------------|
| Total LOC | 33,056 | Mid-large project |
| Total tests | 372 | Good coverage |
| Test-to-LOC ratio | 1:89 | Healthy |
| Async functions | 241 | Heavily async |
| `anyhow::Result` | 171 | Consistent error handling |
| `Arc<Mutex>` | 5 | Minimal lock contention |
| `Arc<RwLock>` | 15 | Read-optimized sharing |
| Serialize/Deserialize | 185 | JSON-heavy |
| Assert count | 824 | Comprehensive assertions |
| Clippy warnings | 0 | Clean |
| Unsafe code | 0 | Memory safe |
| Edition | 2024 | Latest Rust |
| Binary size | 6.7 MB | Optimized (LTO + strip) |

---

## Appendix A: Execution Algorithm Decision Matrix

```
                    ┌──────────────────────────────────────────────┐
                    │ How to choose the right algorithm?           │
                    └──────────────────────────────────────────────┘
                                        │
                    ┌───────────────────┼──────────────────────┐
                    ▼                   ▼                      ▼
              Order Size?        Urgency?               Stealth Need?
                    │                   │                      │
         ┌────┬────┴────┐         ┌────┴────┐          ┌─────┴─────┐
         ▼    ▼         ▼         ▼         ▼          ▼           ▼
       Tiny  Small   Large     Patient   Urgent    Visible    Hidden
      <$500 $1-5K    >$5K                          │          │
         │    │         │         │         │       │          │
         ▼    ▼         ▼         ▼         ▼       ▼          ▼
      MARKET TWAP    IS/OFI     VWAP      IS      OFI        POV
             or                             (algo-   (wait      (follow
           OFI                           selecting) for depth) volume)
```

## Appendix B: Crate Responsibility Cheat Sheet

| I need to... | Use crate | Key type |
|-------------|-----------|----------|
| Get price data | `bonbo-binance-futures` | `RestMarket::get_price()` |
| Compute RSI/MACD | `bonbo-ta` | `Rsi::new(14).update(v)` |
| Get candle data | `bonbo-data` | `Candle { open, high, low, close, volume }` |
| Compute Hurst | `bonbo-quant` | `hurst::hurst_exponent(&prices)` |
| Detect regime | `bonbo-regime` | `RegimeDetector::detect(&candles)` |
| Detect anomalies | `bonbo-sentinel` | `Sentinel::analyze(&candles)` |
| Size a position | `bonbo-risk` | `PositionSizer::kelly(prob, rr)` |
| Execute TWAP | `bonbo-executor` | `execute_twap(placer, ...)` |
| Execute VWAP | `bonbo-executor` | `execute_vwap(placer, fetcher, ...)` |
| Execute POV | `bonbo-executor` | `execute_pov(placer, fetcher, ...)` |
| Minimize IS | `bonbo-executor` | `execute_is(placer, ...)` |
| OFI snipe | `bonbo-executor` | `execute_ofi(placer, ...)` |
| Register a tool | `bonbo-extend` | `impl ToolPlugin for MyPlugin` |
| Call from AI | `bonbo-extend-mcp` | MCP protocol (stdio) |
