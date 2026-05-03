# 🎯 Kế hoạch Implement: trading-process-improvement.md

> **Created:** 2026-04-27 | **Source:** `docs/research/trading-process-improvement.md`
> **Scope:** 10 improvements, 4 phases, ~42,600 LOC existing codebase, 647 tests passing

---

## Tổng quan

```
┌───────────────────────────────────────────────────────────────┐
│  Phase 1 (Week 1): Quick Wins — ATR SL + Hurst + LaguerreRSI │
│  Phase 2 (Week 2): Critical Fixes — MTF + Hurst Hybrid       │
│  Phase 3 (Week 3-4): Enhancement — Sizing + Scanning + Signal │
│  Phase 4 (Week 5+): Long-term — Portfolio + Strategy Library   │
└───────────────────────────────────────────────────────────────┘
```

### Codebase hiện tại đã sẵn sàng
| Component | Status | Ghi chú |
|-----------|--------|---------|
| `bonbo-ta` (3,863 LOC, 15 indicators) | ✅ Ready | ATR, dual Hurst, dual LaguerreRSI, ALMA, Ehlers |
| `bonbo-regime` (1,268 LOC) | ✅ Ready | BOCPD + Hurst R/S + MtfGuard |
| `bonbo-risk` (954 LOC) | ✅ Ready | Kelly, ATR sizing, regime multiplier |
| `bonbo-quant` (4,206 LOC, 13 strategies) | ✅ Ready | Strategy trait + backtest engine |
| `bonbo-learning` (671 LOC) | ✅ Ready | DMA weight adaptation |
| `bonbo-agent` (decision loop) | ✅ Ready | Trait-based architecture |
| `bonbo-scanner` (402 LOC) | 🔨 Needs expansion | Top movers, dynamic discovery |

---

## ═══════════════════════════════════════════════════════════
## PHASE 1: QUICK WINS (Week 1 — 4-6 giờ)
## ═══════════════════════════════════════════════════════════

### 🟢 Task 1.1: ATR-Based Stop Loss (Quick Win #6)
**Mục tiêu:** Tính SL tự động dựa trên ATR × regime multiplier
**Thời gian:** 1-2 giờ
**Files cần sửa/tạo:**
- [ ] `bonbo-agent/src/decision_loop.rs` — thêm hàm `compute_atr_stop_loss()`
- [ ] `bonbo-risk/src/position_sizing.rs` — thêm `AtrStopLoss` struct

**Chi tiết implement:**
```
ATR SL Logic:
  Trending (H>0.55):    SL = Entry - 2.0 × ATR(14)  (wider, let winners run)
  Mean-Reverting (H<0.45): SL = Entry - 1.5 × ATR(14) (tighter)
  Random Walk:          SL = Entry - 2.5 × ATR(14)  (widest, avoid noise)
  
  TP = Entry + (Entry - SL) × risk_reward_ratio
  (risk_reward_ratio từ config: min_risk_reward = 1.5)
```

**Verification:**
- [ ] Unit test: ATR SL calculation cho mỗi regime
- [ ] Unit test: TP derivation từ SL
- [ ] Integration: decision_loop dùng ATR SL khi generate signals

---

### 🟢 Task 1.2: Hurst Divergence Handling (Quick Win #7)
**Mục tiêu:** Khi short-term Hurst ≠ long-term, điều chỉnh confidence + widen stops
**Thời gian:** 30 phút
**Files cần sửa:**
- [ ] `bonbo-regime/src/classifier.rs` — thêm `hurst_divergence()` method
- [ ] `bonbo-agent/src/live_mcp.rs` — integrate divergence vào indicator result

**Chi tiết implement:**
```
if |hurst_50 - hurst_100| > 0.15:
    confidence *= 0.5
    stop_multiplier *= 1.5
    if hurst_50 > hurst_100:
        hint = "Transition to trending"
    else:
        hint = "Trend fading, prepare exit"
```

**Verification:**
- [ ] Unit test: divergence detection với known values
- [ ] Unit test: confidence adjustment
- [ ] Unit test: stop multiplier adjustment

---

### 🟢 Task 1.3: LaguerreRSI Dual-Gamma Signal (Quick Win #8)
**Mục tiêu:** Dùng dual LaguerreRSI (0.5 fast + 0.8 slow) cho divergence signal
**Thời gian:** 1 giờ
**Files cần sửa:**
- [ ] `bonbo-ta/src/batch.rs` — thêm `laguerre_rsi_divergence` vào `FullAnalysis`
- [ ] `bonbo-agent/src/live_mcp.rs` — expose divergence trong `IndicatorResult`

**Chi tiết implement:**
```
laguerre_divergence = laguerre_rsi_fast(0.5) - laguerre_rsi_slow(0.8)

Signal:
  divergence > 0.3  → Strong BUY (fast > slow = momentum accelerating)
  divergence < -0.3 → Strong SELL (fast < slow = momentum decelerating)
  |divergence| ≤ 0.3 → NEUTRAL
```

**Verification:**
- [ ] Unit test: divergence calculation
- [ ] Unit test: signal generation từ divergence
- [ ] Integration: IndicatorResult có divergence field

---

### 🟢 Task 1.4: Wire Quick Wins vào Decision Loop
**Mục tiêu:** Kết nối Task 1.1 + 1.2 + 1.3 vào trading cycle
**Thời gian:** 1-2 giờ
**Files cần sửa:**
- [ ] `bonbo-agent/src/decision_loop.rs` — update `run_cycle()` flow
- [ ] `bonbo-agent/src/live_mcp.rs` — update `IndicatorResult` struct
- [ ] `bonbo-agent/src/mcp_client.rs` — thêm fields mới

**Verification:**
- [ ] `cargo check` pass
- [ ] `cargo test` all pass (647+ tests)
- [ ] `cargo clippy` 0 warnings

---

## ═══════════════════════════════════════════════════════════
## PHASE 2: CRITICAL FIXES (Week 2 — 5-7 ngày)
## ═══════════════════════════════════════════════════════════

### 🔴 Task 2.1: MTF Look-Ahead Bias Fix (Critical #1)
**Mục tiêu:** Đảm bảo chỉ dùng completed bars cho higher-timeframe indicators
**Thời gian:** 3-5 ngày
**Status:** `MtfGuard` đã implement trong `bonbo-regime/src/mtf_guard.rs`

**Files cần sửa:**
- [ ] `bonbo-agent/src/live_mcp.rs` — integrate `MtfGuard` vào `LiveMcpClient`
- [ ] `bonbo-quant/src/engine.rs` — thêm `strict_mtf: bool` vào `BacktestConfig`
- [ ] `bonbo-extend/src/tools/technical_analysis.rs` — gate MTF analysis
- [ ] `bonbo-ta/src/batch.rs` — thêm `FullAnalysis::compute_strict_mtf()`

**Chi tiết implement:**
```
LiveMcpClient changes:
1. Tạo MtfGuard cho mỗi timeframe pair (1h→4h, 4h→1d)
2. Khi analyze_indicators("BTCUSDT", "4h"):
   - Fetch 1h klines
   - Feed qua MtfGuard(H4, H1)
   - CHỈ tính 4h indicators khi bar complete
   - Forward-fill giá trị cũ cho incomplete bars
3. Khi analyze_indicators("BTCUSDT", "1d"):
   - Fetch 4h klines
   - Feed qua MtfGuard(D1, H4)
   - CHỈ tính 1d indicators khi bar complete

BacktestConfig changes:
1. Thêm strict_mtf: bool (default: true)
2. Khi true → chỉ emit signals trên completed bars
3. Khi false → old behavior (cho comparison testing)
```

**Verification:**
- [ ] Unit test: MtfGuard integration với indicator computation
- [ ] Integration test: so sánh strict_mtf=true vs false results
- [ ] Test: forward-fill behavior
- [ ] `cargo test` all pass

---

### 🔴 Task 2.2: Hurst DFA Method + Hybrid Regime (Critical #2)
**Mục tiêu:** Thêm DFA (Detrended Fluctuation Analysis) song song R/S, cross-validate
**Thời gian:** 5-7 ngày

**Files cần tạo/sửa:**
- [ ] `bonbo-ta/src/indicators/hurst.rs` — thêm `HurstDfa` struct
- [ ] `bonbo-regime/src/classifier.rs` — dùng dual Hurst (R/S + DFA)
- [ ] `bonbo-regime/src/models.rs` — thêm `HurstMethod`, `HybridHurstResult`

**Chi tiết implement:**
```
HurstDFA algorithm:
1. Compute cumulative sum of returns
2. Divide into windows of size s
3. For each window: fit linear trend, compute RMS of detrended series
4. Plot log(RMS) vs log(s)
5. Slope = Hurst exponent

Hybrid detection:
  hurst_rs  = R/S method (existing)
  hurst_dfa = DFA method (new)
  
  if both > 0.55 → Strong Trending (confidence +0.2)
  if both < 0.45 → Strong Mean-Reverting (confidence +0.2)
  if disagree   → Uncertain (confidence -0.3, reduce position)
  
  Sliding windows:
    50 bars  → responsive (catch regime changes early)
    100 bars → stable (confirmed regime)
    500 bars → long-term trend identification
```

**Verification:**
- [ ] Unit test: DFA computation vs known Hurst values
- [ ] Unit test: hybrid agreement/disagreement
- [ ] Property test: DFA results trong [0, 1] range
- [ ] Integration: RegimeClassifier dùng hybrid Hurst
- [ ] Benchmark: DFA performance (target: <1ms per 1000 bars)

---

### 🔴 Task 2.3: Signal Scoring Enhancement — Z-Score Normalization
**Mục tiêu:** Chuẩn hóa tất cả indicator outputs sang Z-score [-1, +1] trước khi aggregate
**Thời gian:** 2-3 ngày

**Files cần sửa:**
- [ ] `bonbo-ta/src/batch.rs` — thêm `z_score_normalize()` 
- [ ] `bonbo-learning/src/weights.rs` — dùng Z-score thay vì raw values
- [ ] `bonbo-agent/src/live_mcp.rs` — normalize indicator scores

**Chi tiết implement:**
```
Z-score normalization per indicator:
  rsi_z = (rsi - 50) / 25           → [-2, +2] → clamp [-1, +1]
  macd_z = macd_histogram / σ(hist)  → standardized
  hurst_z = (hurst - 0.5) / 0.15    → trending/mean-reverting scale
  laguerre_z = (lrsi - 0.5) × 2     → [-1, +1]
  bb_position = (price - bb_lower) / (bb_upper - bb_lower) → [0, 1] → [-1, +1]

Composite score = Σ(weight_i × z_score_i) / Σ|weight_i|
```

---

## ═══════════════════════════════════════════════════════════
## PHASE 3: ENHANCEMENTS (Week 3-4)
## ═══════════════════════════════════════════════════════════

### 🟡 Task 3.1: Dynamic Scanning — Expand Watchlist (Enhancement #4)
**Thời gian:** 2-3 ngày

**Files cần sửa/tạo:**
- [ ] `bonbo-scanner/src/scanner.rs` — thêm `scan_top_movers()`, `scan_top_volume()`
- [ ] `bonbo-scanner/src/models.rs` — thêm `ScanTier` enum
- [ ] `bonbo-binance-futures/src/rest/` — thêm endpoints nếu thiếu
- [ ] `bonbo-agent/src/mcp_client.rs` — thêm `scan_dynamic()` method

**Chi tiết:**
```
3-tier scanning:
  Tier 1: Top 50 by volume (always scan) — daily refresh
  Tier 2: Watchlist + holdings (always scan) — from config
  Tier 3: Hot movers — top gainers/losers 24h (min volume $1M)

  Deduplicate → score all → filter score ≥ 40 → sort → top N
  Auto-discover: coin mới vào top volume → auto-add vào watchlist
```

---

### 🟡 Task 3.2: Position Sizing Intelligence (Enhancement #5)
**Thời gian:** 3-5 ngày

**Files cần sửa:**
- [ ] `bonbo-risk/src/position_sizing.rs` — đã có Kelly + ATR + regime sizing
- [ ] `bonbo-agent/src/decision_loop.rs` — integrate multi-method sizing
- [ ] `bonbo-learning/src/dma.rs` — feed DMA weights vào sizing

**Chi tiết:**
```
Multi-method sizing (TAKE MINIMUM):
  1. Kelly Criterion: f* = (p×b - q) / b (where p=win_rate, b=win/loss, q=1-p)
  2. ATR-based: size = (equity × risk_pct) / (atr × multiplier)
  3. Regime multiplier: Trending 1.0, Ranging 0.5, RandomWalk 0.25, Volatile 0.3
  4. Portfolio constraint: max correlation exposure across positions
  5. DMA weight: adjust base_risk_pct based on signal confidence
  
  final_size = min(kelly, atr_size) × regime_mult × portfolio_check
```

---

### 🟡 Task 3.3: Signal Aggregation Enhancement (Enhancement #3 — Phase 1)
**Thời gian:** 1-2 tuần (chỉ phase 1: stacking base)

**Files cần tạo:**
- [ ] `bonbo-learning/src/ensemble.rs` — stacking meta-learner (lightweight)
- [ ] `bonbo-learning/src/features.rs` — feature engineering from indicators

**Chi tiết (Phase 1 — Simple Stacking):**
```
Layer 1: Z-score normalized indicators (from Task 2.3)
Layer 2: Base models:
  - Logistic Regression (fast, streaming-friendly)
  - Decision Stump ensemble (simple, interpretable)
Layer 3: Meta-learner: weighted average with DMA-adapted weights

(Phase 2 — ML stacking: RF + XGBoost → deferred to Phase 4)
```

---

## ═══════════════════════════════════════════════════════════
## PHASE 4: LONG-TERM (Week 5+)
## ═══════════════════════════════════════════════════════════

### 🔵 Task 4.1: Portfolio Analysis (Long-term #9)
**Thời gian:** 1-2 tuần
**Crate mới:** `bonbo-portfolio`

```
Features:
- Rolling 30-day correlation matrix
- Cointegration pairs detection (for pairs trading)
- VaR (Value at Risk) per position + portfolio level
- HHI (Herfindahl-Hirschman Index) — concentration risk
- Stress testing (scenario: all drop 10%)
```

---

### 🔵 Task 4.2: Strategy Library Expansion (Long-term #10)
**Thời gian:** 1+ tuần

**Status:** `bonbo-quant/advanced_strategies.rs` đã có 8 strategies:
- ✅ `EhlersTrendStrategy`
- ✅ `EnhancedMeanReversionStrategy`  
- ✅ `AlmaCrossoverStrategy`
- ✅ `LaguerreRsiStrategy`
- ✅ `CmoMomentumStrategy`
- ✅ `FhCompositeStrategy`
- ✅ `BbBounceStrategy`
- ✅ `HurstRegimeSwitchingStrategy`

**Cần thêm:**
- [ ] `SuperSmootherSlopeStrategy` — SuperSmoother slope-based
- [ ] `MacdHurstFilterStrategy` — MACD + Hurst filter
- [ ] `RegimeAdaptiveStrategy` — auto-select sub-strategy by regime
- [ ] `compare_strategies()` tool — auto-test all, recommend best per regime

---

### 🔵 Task 4.3: HMM Regime Detection (Long-term — from Critical #2 phase 2)
**Thời gian:** 1-2 tuần

```
Hidden Markov Model:
  5 states: Strong Up, Weak Up, Ranging, Weak Down, Strong Down
  Features: [hurst_rs, hurst_dfa, volatility, volume_ratio]
  
  Framework: linfa (Rust ML) hoặc candle-affine HMM
  Training: từ bonbo-journal historical data
```

---

## ═══════════════════════════════════════════════════════════
## PRIORITY MATRIX
## ═══════════════════════════════════════════════════════════

| Phase | Task | Improvement # | Impact | Effort | Tests Added |
|-------|------|--------------|--------|--------|-------------|
| 1 | 1.1 ATR SL | #6 | Medium | 1-2h | ~5 |
| 1 | 1.2 Hurst Div | #7 | Medium | 30m | ~3 |
| 1 | 1.3 LaguerreRSI | #8 | Medium | 1h | ~4 |
| 1 | 1.4 Wire Up | — | High | 1-2h | ~3 |
| 2 | 2.1 MTF Fix | #1 | Critical | 3-5d | ~8 |
| 2 | 2.2 Hurst DFA | #2 | Critical | 5-7d | ~10 |
| 2 | 2.3 Z-Score | #3 partial | High | 2-3d | ~5 |
| 3 | 3.1 Scanning | #4 | High | 2-3d | ~6 |
| 3 | 3.2 Sizing | #5 | High | 3-5d | ~7 |
| 3 | 3.3 Ensemble | #3 partial | High | 1-2w | ~8 |
| 4 | 4.1 Portfolio | #9 | Medium | 1-2w | ~10 |
| 4 | 4.2 Strategies | #10 | Medium | 1w | ~8 |
| 4 | 4.3 HMM | #2 phase2 | High | 1-2w | ~8 |

**Total estimated:** ~85 new tests across all phases

---

## ═══════════════════════════════════════════════════════════
## RISK MITIGATION
## ═══════════════════════════════════════════════════════════

1. **Always `cargo test` after each task** — maintain 647+ green tests
2. **Always `cargo clippy`** — 0 warnings policy
3. **Feature flags** cho mỗi phase (`mtf_strict`, `hurst_dfa`, `ensemble`)
4. **Backward compatible** — `strict_mtf=false` preserves old behavior
5. **Incremental rollout** — Phase 1 → testnet → live (never skip testnet)

---

## ═══════════════════════════════════════════════════════════
## DEPENDENCY GRAPH
## ═══════════════════════════════════════════════════════════

```
Task 1.1 (ATR SL) ─────────────────┐
Task 1.2 (Hurst Div) ──────────────┤
Task 1.3 (LaguerreRSI Dual) ───────┤→ Task 1.4 (Wire Up) → Phase 1 DONE
                                    │
Task 2.1 (MTF Fix) ────────────────┤
Task 2.2 (Hurst DFA) ─────────────┤→ Task 2.3 (Z-Score) → Phase 2 DONE
                                    │
Task 3.1 (Scanning) ──────────────┤
Task 3.2 (Sizing) ────────────────┤→ Task 3.3 (Ensemble) → Phase 3 DONE
                                    │
Task 4.1 (Portfolio) ─────────────┤
Task 4.2 (Strategies) ────────────┤→ Task 4.3 (HMM) → Phase 4 DONE
```
