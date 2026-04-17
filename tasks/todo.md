# BonBoExtend — Task List

## ✅ Phase 1: Foundation (DONE)
- [x] Phân tích kiến trúc BonBo Core hiện tại
- [x] Phân tích BonBoTrade hiện tại
- [x] Thiết kế Plugin System architecture
- [x] Tạo workspace Cargo.toml
- [x] Tạo bonbo-extend crate (Plugin trait, Registry)
- [x] Tạo bonbo-extend-mcp crate (MCP Server)
- [x] Build thành công, 0 warnings, 6/6 tests pass
- [x] Tạo docs: ARCHITECTURE.md, UPGRADE_GUIDE.md
- [x] Tạo upgrade script
- [x] Git init + commit

## ✅ Phase 2a: Build & Tool Testing (DONE)
- [x] Build release binary bonbo-extend-mcp (7.2MB)
- [x] Chạy thử MCP server stdio mode — test tools/list ✅ 10 tools
- [x] Chạy thử MCP server HTTP mode — test tools/list + tools/call ✅ port 9876
- [x] Test all 10 tools successfully via HTTP MCP
- [x] Test error handling (unknown tool) ✅ isError: true

## ✅ Phase 2c: Quantitative Analysis Research (DONE)
- [x] Deep research: 6 agents, 9 iterations, 361 sources
- [x] Thiết kế kiến trúc Quantitative Crypto Analysis Platform
- [x] Tạo docs/QUANT_ARCHITECTURE.md
- [x] Identify 6 new crates: bonbo-ta, bonbo-data, bonbo-quant, bonbo-sentinel, bonbo-risk
- [x] Identify Rust TA libraries: ta-rs, nanobook, indicators-ta, quantedge-ta
- [x] Identify backtesting frameworks: NautilusTrader, Barter-rs, hftbacktest
- [x] Identify on-chain data providers: Glassnode, Nansen, Dune, Alternative.me
- [x] Risk management architecture: circuit breakers, Kelly Criterion, CVaR

## 📋 Phase A: bonbo-ta — Technical Analysis Engine
- [ ] Tạo crate bonbo-ta trong workspace
- [ ] Implement IncrementalIndicator trait (Next<T>/Reset pattern)
- [ ] Implement indicators: SMA, EMA, RSI
- [ ] Implement indicators: MACD, Bollinger Bands, ATR
- [ ] Implement indicators: ADX, Stochastic, CCI
- [ ] Implement indicators: VWAP, OBV, Volume Profile
- [ ] Dual API: batch (historical) + streaming (real-time)
- [ ] MCP tools: analyze_indicator, get_signal, compute_indicators
- [ ] MCP tools: get_support_resistance, detect_patterns, get_market_regime
- [ ] Unit tests + cross-validation against TA-Lib

## 📋 Phase B: bonbo-data — Data Layer
- [ ] Tạo crate bonbo-data trong workspace
- [ ] MarketDataCache with SQLite backend
- [ ] Historical data fetching (Binance klines, multi-timeframe)
- [ ] WebSocket streaming for real-time prices
- [ ] MCP tools: fetch_historical, stream_realtime, get_multi_timeframe

## 📋 Phase C: bonbo-quant — Backtesting Engine
- [ ] Tạo crate bonbo-quant trong workspace
- [ ] Strategy trait: on_bar, on_tick, on_order_fill
- [ ] BacktestEngine: event-driven simulation
- [ ] Fill models: instant, spread-based, order-book-walking
- [ ] Fee modeling: maker/taker, gas, slippage
- [ ] Report: PnL, Sharpe, Sortino, Max Drawdown, Win Rate
- [ ] MCP tools: run_backtest, optimize_strategy, get_backtest_report

## 📋 Phase D: bonbo-sentinel — On-chain + Sentiment
- [ ] Tạo crate bonbo-sentinel trong workspace
- [ ] Fear & Greed Index (Alternative.me free API)
- [ ] Whale alerts (large transactions)
- [ ] Glassnode on-chain metrics (MVRV, SOPR, NVT) — optional paid
- [ ] Signal normalization to [-1, +1]
- [ ] MCP tools: get_sentiment, get_onchain_metrics, get_whale_alerts

## 📋 Phase E: bonbo-risk — Risk Management
- [ ] Tạo crate bonbo-risk trong workspace
- [ ] Position sizing: Fixed %, Kelly, Half-Kelly
- [ ] Multi-layer circuit breaker
- [ ] CVaR/VaR computation
- [ ] Pre-trade risk checks pipeline
- [ ] MCP tools: calculate_position_size, check_risk, get_portfolio_metrics

## 📋 Phase F: Integration & Polish
- [ ] All crates integrated into bonbo-extend-mcp
- [ ] 30+ MCP tools exposed to BonBo AI Agent
- [ ] End-to-end testing with BonBo
- [ ] Performance benchmarks
- [ ] Documentation and examples
