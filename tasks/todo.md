# BonBoExtend Code Fix — Code Graph Review

## Fix 1: Extract duplicated helpers → utils.rs
- [x] Create bonbo-executor/src/utils.rs with decimal_to_f64 + compute_jitter
- [x] Replace 7 copies of decimal_to_f64 across executor files
- [x] Replace 5 copies of compute_jitter across algo files
- [x] Update mod.rs to export utils

## Fix 2: Add tests for circuit_breaker.rs (risk-critical, 0 tests)
- [x] Write unit tests for circuit_breaker (21 fns)

## Fix 3: Add tests for trading.rs::smart_execute (money-critical, 0 tests)
- [x] Write unit tests for smart_execute pipeline

## Fix 4: Refactor generate_signals (391 → sub-functions)
- [x] Split generate_signals into 3-4 focused functions

## Fix 5: Remove dead code in rate_limiter.rs + account.rs
- [x] Remove or annotate unused functions

## Fix 6: Extract to_ohlcv from examples → use bonbo-data
- [x] Update examples to use shared to_ohlcv from bonbo-data

## Fix 7: Compile + Test verification
- [x] cargo build --release clean
- [x] cargo test --workspace all pass
- [x] cargo clippy clean

---

## Quick Wins — Trading Analysis Improvements (2026-04-24)

- [x] QW1: ATR-Based Stop Loss — thêm compute_atr_stops() vào batch.rs
- [x] QW2: Hurst Divergence Handler — dual-window Hurst + divergence signals
- [x] QW3: LaguerreRSI Calibration — configurable gamma (0.5 fast + 0.8 slow)
- [x] Build + Test — 51 tests pass, 0 fail, release build OK
- [x] MCP Integration — all 3 Quick Wins verified live via MCP

*Session started: 2026-04-24 12:17*

---

## Phase 1+2+3 — Full Research Implementation (2026-04-24) ✅

### Phase 1: Critical Fixes
- [x] P1.1: Dynamic Scanning — scan_hot_movers MCP tool
- [x] P1.2: Regime-Conditional Position Sizing — Kelly + ATR + regime
- [x] P1.3: Portfolio Correlation Analysis — analyze_portfolio MCP tool
- [x] P1.4: MTF Look-Ahead Prevention — MtfGuard module (#1 research)
- [x] P1.5: Hurst+BOCPD Hybrid Classifier — R/S method in classifier (#2 research)

### Phase 2: Quick Wins
- [x] QW1: ATR-Based Stop Loss (regime-adaptive)
- [x] QW2: Hurst Divergence (50/100 dual-window)
- [x] QW3: Dual LaguerreRSI (γ=0.5 + γ=0.8)

### Phase 3: Strategy Library
- [x] P3.1: BB Bounce strategy (H < 0.45 only)
- [x] P3.2: Hurst Regime-Switching meta-strategy
- [x] P3.3: Backtest tool — 17 strategies
- [x] P3.4: Release build — 14 plugins, 48 tools, 151 tests, 0 fail
