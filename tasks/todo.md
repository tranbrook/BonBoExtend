# BonBoExtend — Trading Process Improvement Tasks

## Phase 1: Quick Wins (Week 1) ✅
- [x] **Task 1.1**: ATR-Based Stop Loss
- [x] **Task 1.2**: Hurst Divergence Handling
- [x] **Task 1.3**: LaguerreRSI Dual-Gamma Signal
- [x] **Task 1.4**: Wire Quick Wins into Decision Loop
- [x] **Gate 1**: All tests pass, cargo clippy clean

## Phase 2: Critical Fixes (Week 2) ✅
- [x] **Task 2.1**: MTF Look-Ahead Bias Fix
- [x] **Task 2.2**: Hurst DFA Method
- [x] **Task 2.3**: Z-Score Normalization
- [x] **Gate 2**: All tests pass

## Phase 3: Enhancements (Week 3-4) ✅
- [x] **Task 3.1**: Dynamic Scanning
- [x] **Task 3.2**: Multi-Method Position Sizing
- [x] **Task 3.3**: Signal Aggregation
- [x] **Gate 3**: All tests pass, 281 total

## Phase 4: Long-term (Week 5+) ✅
- [x] **Task 4.1**: Portfolio Analysis crate
- [x] **Task 4.2**: Strategy Library
- [x] **Gate 4**: All tests pass

---

## Session 2026-04-28 — Analysis + best_trade.rs v2 ✅
- [x] Phân tích top giao dịch tốt nhất (scanner v1)
- [x] Phân tích chuyên sâu AXSUSDT
- [x] Truy cập tài khoản Binance (positions: AXSUSDT, TRXUSDT, HBARUSDT)
- [x] So sánh Rust vs Python scoring — tìm ra khác biệt cốt lõi
- [x] **Rewrite best_trade.rs v2** — implement all 10 improvements
- [x] Phân tích điểm vào lệnh tối ưu FETUSDT

---

## Session 2026-04-28/29 — Overnight AI Coding (6h)

### Phase A: BEST_TRADE.RS V2 HARDENING
- [ ] **A1**: Cross-Correlation Filter (#9) — loại bỏ coins correlated > 0.85
- [x] **A2**: Backtest warmup fix — nhiều strategies trả về 0 trades
- [ ] **A3**: Entry Signal Confirmation — volume spike + candle + orderbook
- [x] **A4**: Export JSON output + delta so với scan trước

### Phase B: PYTHON ↔ RUST SYNC
- [x] **B1**: Unify scoring logic — Python dùng cùng 7-family scoring
- [ ] **B2**: orca_deep.py: thêm 4H+1D MTF consensus
- [ ] **B3**: scan_alert.py — auto-alert khi top thay đổi

### Phase C: STRATEGY OPTIMIZATION
- [ ] **C1**: Grid search optimal params cho top strategies
- [x] **C2**: Regime-Strategy mapping table (config file)
- [ ] **C3**: Walk-forward validation (train 70% / test 30%)

### Phase D: MONITORING & DOCS
- [x] **D1**: Auto-scan cron setup (mỗi 4h)
- [x] **D2**: Position monitor — alert khi trend đảo chiều
- [x] **D3**: Update PROJECT_README.md + activity.md

*Overnight session started: 2026-04-28 ~22:00 UTC*


---

## New Session - 2026-05-03 23:08
- [ ] Review existing todo items
- [ ] Identify new requirements
- [ ] Update task priorities
- [ ] Add session-specific tasks

*Session started: 2026-05-03 23:08*
