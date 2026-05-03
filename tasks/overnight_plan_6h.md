# BonBoExtend — 6h Overnight AI Coding Plan (2026-04-28 → 04-29)

## Context
- User đi ngủ 6h, BonBo AI tự coding
- Workspace: 18 crates, 281 tests ALL PASSING, 45K LOC
- best_trade.rs v2 vừa rewrite xong (10 improvements from trading-process-improvement.md)
- Python scanner scripts cần sync logic với Rust

---

## Phase A: BEST_TRADE.RS V2 HARDENING (1.5h)

### A1. Cross-Correlation Filter (#9) — CHƯA IMPLEMENT
- [ ] Implement cross-correlation matrix cho top candidates
- [ ] Loại bỏ coins có correlation > 0.85 với nhau (chỉ giữ score cao nhất)
- [ ] Thêm `cross_corr_filter()` function vào best_trade.rs
- [ ] Test: 2 coins correlated → chỉ giữ 1

### A2. Backtest Validation Improvement (#10)
- [ ] Backtest hiện tại: nhiều strategies return 0 trades → log warmup
- [ ] Fix: tăng warmup period hoặc dùng pre-seeded indicators
- [ ] Thêm "PASS" / "FAIL" label cho backtest (win_rate > 50% + return > 0 = PASS)
- [ ] Thêm "Backtest Confidence" score (0-100) dựa trên #trades + win_rate + sharpe

### A3. FETUSDT-specific: Entry Signal Confirmation
- [ ] Thêm "entry_confirmation_check()" function
- [ ] Check: volume spike + candle pattern + orderbook imbalance
- [ ] Return: ENTRY_NOW / WAIT / CANCEL recommendation

### A4. Output Enhancement
- [ ] Export results to JSON file for downstream consumption
- [ ] Add timestamp + F&G + regime to output
- [ ] Add "compared to last scan" delta column

---

## Phase B: PYTHON ↔ RUST SYNC (1.5h)

### B1. Unify Scoring Logic
- [ ] Python `analyze_top100.py` và Rust `best_trade.rs` cho kết quả khác nhau
- [ ] Sync: Python dùng cùng 7-family scoring như Rust
- [ ] Key: RSI < 40 = oversold BUY signal (contrarian), KHÔNG phải trend-follow SELL

### B2. Python Scanner: Add MTF + Backtest
- [ ] `orca_deep.py`: thêm 4H+1D multi-timeframe consensus
- [ ] Thêm Hurst dual-window divergence detection
- [ ] Thêm backtest validation (gọi Rust via subprocess or MCP)

### B3. Alert System
- [ ] Tạo `scripts/scan_alert.py`: chạy scanner + so sánh với lần trước
- [ ] Nếu có coin mới vào top 5 hoặc score thay đổi > 10pts → log alert
- [ ] Output: `reports/alerts_YYYYMMDD_HHMM.md`

---

## Phase C: STRATEGY OPTIMIZATION (1.5h)

### C1. Backtest Strategy Parameters
- [ ] Chạy grid search cho top strategies trên FETUSDT, BTCUSDT, SOLUSDT
- [ ] ALMA Crossover: test fast=5/8/10/12, slow=20/25/30/40
- [ ] RSI Mean Reversion: test period=7/10/14/21, oversold=20/25/30/35
- [ ] Output: optimal parameters per regime per symbol

### C2. Regime-Strategy Mapping Table
- [ ] Tạo table: Regime → Best Strategy → Optimal Params
- [ ] Store trong `config/regime_strategies.toml`
- [ ] best_trade.rs đọc table này thay vì hardcode

### C3. Walk-Forward Validation
- [ ] Implement basic walk-forward: train 70% → test 30%
- [ ] Đảm bảo backtest results không overfitted
- [ ] Report: in-sample vs out-of-sample performance

---

## Phase D: MONITORING & DOCS (1.5h)

### D1. Auto-Scan Cron Setup
- [ ] Tạo script `scripts/auto_scan.sh` chạy best_trade mỗi 4h
- [ ] Output: `reports/scan_YYYYMMDD_HHMM.md` + `reports/scan_YYYYMMDD_HHMM.json`
- [ ] Rotate: keep last 24 scans, auto-delete older

### D2. Position Monitor
- [ ] Tạo `scripts/position_monitor.py`:
  - Fetch current positions via MCP
  - Re-analyze each position with current data
  - Alert if any position trend REVERSED (4H changed direction)
  - Suggest: HOLD / CLOSE / ADJUST SL

### D3. Update Documentation
- [ ] Update `docs/PROJECT_README.md` với v2 changes
- [ ] Update `docs/activity.md` với session log
- [ ] Update `tasks/todo.md` — mark completed items

---

## Execution Priority (if time limited)
1. **A2** (Backtest fix) — critical for accuracy
2. **B1** (Python-Rust sync) — resolve conflicting analysis
3. **D2** (Position monitor) — protect user's open positions
4. **A1** (Correlation filter) — reduce redundancy
5. **C1** (Strategy optimization) — improve backtest quality
6. Everything else in order

## Verification Gates
- After each phase: `cargo check` + `cargo test` must pass
- After all phases: `cargo clippy -- -W clippy::all` must be clean
- Final: `cargo run --release --example best_trade` must produce valid output
