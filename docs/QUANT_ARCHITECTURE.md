# BonBoExtend — Quantitative Crypto Analysis Platform

## Kiến trúc tổng thể

```
┌───────────────────────────────────────────────────────────────────────┐
│                     BonBo AI Agent (Core)                            │
│                        MCP Client                                    │
└────────────────────────────┬──────────────────────────────────────────┘
                             │ JSON-RPC / HTTP / stdio
                             ▼
┌───────────────────────────────────────────────────────────────────────┐
│                   bonbo-extend-mcp (MCP Server)                      │
│                 HTTP :9876  |  stdio transport                       │
│                                                                       │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │                    Plugin Registry                              │ │
│  │  ToolPlugin trait → register → route → execute                 │ │
│  └──────────┬──────────┬──────────┬──────────┬─────────────────────┘ │
│             │          │          │          │                        │
│  ┌──────────▼──┐ ┌─────▼─────┐ ┌─▼────────┐ ┌▼──────────────────┐  │
│  │  bonbo-ta   │ │ bonbo-    │ │ bonbo-   │ │ bonbo-            │  │
│  │  (Technical │ │ quant     │ │ sentinel │ │ risk              │  │
│  │  Analysis)  │ │ (Backtest │ │ (On-chain│ │ (Risk Manager     │  │
│  │             │ │  Engine)  │ │  +Sentim)│ │  + Circuit Break) │  │
│  │ RSI/MACD    │ │ Strategy  │ │ MVRV/SOPR│ │ Position Sizing   │  │
│  │ BB/EMA/ATR  │ │ Backtest  │ │ Fear&Greed│ │ Stop Loss         │  │
│  │ Stoch/ADX   │ │ Optimize  │ │ Whale    │ │ Drawdown Control  │  │
│  │ Ichimoku    │ │ Report    │ │ News/NLP │ │ CVaR/VaR          │  │
│  └─────────────┘ └───────────┘ └──────────┘ └───────────────────┘  │
│             │          │          │          │                        │
│  ┌──────────▼──────────▼──────────▼──────────▼────────────────────┐ │
│  │                    bonbo-data (Data Layer)                      │ │
│  │  MarketDataCache │ OHLCV Store │ WebSocket Stream │ REST Client │ │
│  └────────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────┘
```

## Module Dependency Graph

```
bonbo-extend (core)
    ├── bonbo-data        — Data fetching, caching, WebSocket streaming
    ├── bonbo-ta          — Technical analysis indicators (O(1) incremental)
    ├── bonbo-quant       — Backtesting engine, strategy framework
    ├── bonbo-sentinel    — On-chain analytics + sentiment analysis
    ├── bonbo-risk        — Risk management, circuit breakers, position sizing
    └── bonbo-extend-mcp  — MCP server (binary, depends on all above)
```

## Data Flow

```
[Binance REST API] ──→ bonbo-data ──→ MarketDataCache (SQLite/TimescaleDB)
[Binance WebSocket] ──→ bonbo-data ──→ Real-time OHLCV stream (tokio::mpsc)
                                          │
                    ┌─────────────────────┼─────────────────────┐
                    ▼                     ▼                     ▼
              bonbo-ta              bonbo-sentinel         bonbo-risk
           (indicators)           (on-chain/sentiment)   (risk checks)
                    │                     │                     │
                    └──────────┬──────────┘                     │
                               ▼                                │
                        bonbo-quant ◄───────────────────────────┘
                    (backtest / signals)     (pre-trade checks)
                               │
                               ▼
                    MCP tools/call → BonBo AI Agent
```

## Key Design Decisions

### 1. Incremental O(1) Indicators (from ta-rs pattern)
```rust
trait IncrementalIndicator: Send + Sync {
    type Input;
    type Output;
    
    fn next(&mut self, input: Self::Input) -> Self::Output;
    fn reset(&mut self);
}
```
- Critical for real-time: NO reprocessing full history on each candle
- Wilder's EMA (alpha=1/period) vs Standard EMA (alpha=2/(period+1))

### 2. Dual API: Batch + Streaming
- **Batch**: Historical analysis (1000 candles → indicators array)
- **Streaming**: Real-time (1 new candle → update indicator state)

### 3. Event-Driven Backtesting (NautilusTrader pattern)
- Barter-rs Strategy/RiskManager traits
- Deterministic: same data → same results
- Configurable fill models: instant, spread-based, order-book-walking

### 4. Multi-Signal Aggregation
- Fixed-weight, confidence-weighted, Bayesian, regime-adaptive
- KL/Jensen-Shannon divergence for conflict detection
- Normalize all signals to [-1, +1] scale before aggregation

### 5. Multi-Layer Risk Management
```
Signal → [Position Size Check] → [Daily Loss Check] → [Drawdown Check] → [Circuit Breaker] → Execute
         max 2% per trade        soft stop at 2%       hard stop at 5%    consecutive losses
```

## Crate Sizes (Estimated)

| Crate | Purpose | LOC (est.) | Dependencies |
|-------|---------|-----------|--------------|
| bonbo-data | Market data + cache | ~800 | reqwest, tokio, serde |
| bonbo-ta | 20+ TA indicators | ~2000 | ta-rs patterns (no ext dep) |
| bonbo-quant | Backtesting engine | ~1500 | bonbo-ta, bonbo-data |
| bonbo-sentinel | On-chain + sentiment | ~1000 | reqwest, serde |
| bonbo-risk | Risk management | ~600 | rust_decimal |
| bonbo-extend-mcp | MCP server | ~500 | all above + axum |
| **Total** | | **~6400** | |

## Phased Roadmap

### Phase A: bonbo-ta (Technical Analysis Engine) — Week 1-2
- Core trait: IncrementalIndicator
- 15 indicators: SMA, EMA, RSI, MACD, Bollinger Bands, ATR, ADX, Stochastic, CCI, VWAP, OBV, Ichimoku, Fibonacci, Pivot Points, Volume Profile
- Dual API: batch + streaming
- 6 MCP tools: analyze_indicator, get_signal, compute_indicators, get_support_resistance, detect_patterns, get_market_regime

### Phase B: bonbo-data (Data Layer) — Week 2-3
- MarketDataCache with SQLite backend
- WebSocket streaming for real-time prices
- Historical data fetching (Binance klines)
- Multi-timeframe support (1m, 5m, 15m, 1h, 4h, 1d, 1w)

### Phase C: bonbo-quant (Backtesting Engine) — Week 3-5
- Strategy trait: on_bar, on_tick, on_order_fill
- BacktestEngine: event-driven simulation
- Fill models: instant, spread-based, order-book-walking
- Fee modeling: maker/taker, gas, slippage
- Report generation: PnL, Sharpe, Sortino, Max Drawdown, Win Rate

### Phase D: bonbo-sentinel (On-chain + Sentiment) — Week 4-6
- Fear & Greed Index (free API)
- Whale alerts (large transactions)
- Glassnode on-chain metrics (MVRV, SOPR, NVT) — optional paid
- News sentiment aggregation
- Signal normalization to [-1, +1]

### Phase E: bonbo-risk (Risk Management) — Week 5-6
- Position sizing: Fixed %, Kelly, Half-Kelly
- Multi-layer circuit breaker
- CVaR/VaR computation
- Pre-trade risk checks pipeline
- Daily P&L tracking

### Phase F: Integration & Polish — Week 7-8
- All crates integrated into bonbo-extend-mcp
- 30+ MCP tools exposed
- BonBo AI Agent end-to-end testing
- Performance optimization
- Documentation and examples
