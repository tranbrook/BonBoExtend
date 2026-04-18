# 🔬 KẾ HOẠCH NGHIÊN CỨU SELF-LEARNING TRADING v2.0

**Ngày tạo:** 2026-04-18  
**Nghiên cứu sâu:** 707 sources, 6 agents, 16 iterations  
**Cập nhật từ:** SELF_LEARNING_PLAN.md v1.0  

---

## 1. TỔNG QUAN — TẠI SAO CẦN KẾ HOẠCH MỚI?

### 1.1 Plan v1.0 (cũ) — Hạn chế
- Weight adaptation quá đơn giản: chỉ ±0.02 mỗi indicator → không đủ cho thị trường phi cố định
- Không có regime-aware learning: dùng cùng weights cho bull/bear/ranging
- Không có overfitting protection: có thể overfit vào noise khi sample size nhỏ
- Không có validation framework: không biết model có thực sự cải thiện hay random luck
- Không có concept drift detection: không biết khi nào cần adapt vs. khi nào noise

### 1.2 Plan v2.0 (mới) — Dựa trên Deep Research
- **Ensemble-based adaptation**: Nhiều model sets cho nhiều regimes
- **DMA (Dynamic Model Averaging)**: Bayesian weight adaptation thay vì heuristic
- **Regime-aware switching**: HMM/GMM + BOCPD cho real-time regime detection
- **CPCV validation**: Gold standard thay vì simple walk-forward
- **Deflated Sharpe Ratio**: Anti-overfitting metrics
- **BOCPD drift detection**: Tự động biết khi nào cần adapt

---

## 2. KIẾN TRÚC TỔNG THỂ

```
┌─────────────────────────────────────────────────────────────────┐
│                    BonBo SELF-LEARNING ENGINE                    │
│                                                                  │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────────┐ │
│  │ JOURNAL   │──▶│ REGIME   │──▶│ ENSEMBLE │──▶│ VALIDATION   │ │
│  │ (Phase 1) │   │ (Phase 2)│   │ (Phase 3)│   │ (Phase 4)   │ │
│  │           │   │          │   │          │   │              │ │
│  │ Record    │   │ Detect   │   │ DMA +    │   │ CPCV + DSR   │ │
│  │ Track     │   │ Classify │   │ Thompson │   │ PBO +        │ │
│  │ Review    │   │ BOCPD    │   │ Bandit   │   │ Haircut SR   │ │
│  └──────────┘   └──────────┘   └──────────┘   └──────────────┘ │
│       │                              │              │            │
│       ▼                              ▼              ▼            │
│  ┌──────────┐   ┌──────────────────────────────────────────┐   │
│  │ SCANNER  │   │          SCORING ENGINE                    │   │
│  │ (Phase 5)│   │                                          │   │
│  │          │   │  Regime-Aware Scoring:                    │   │
│  │ Auto-    │   │  ┌─────────┬──────────┬──────────────┐   │   │
│  │ scan     │   │  │ Bull    │ Bear     │ Ranging      │   │   │
│  │ Schedule │   │  │ weights │ weights  │ weights      │   │   │
│  │ Alert    │   │  │ (DMA)   │ (DMA)    │ (DMA)        │   │   │
│  └──────────┘   │  └─────────┴──────────┴──────────────┘   │   │
│                  │         ↑ Updated by Learning Loop       │   │
│                  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. PHASE CHI TIẾT

### Phase 1: bonbo-journal — Trade Journal & Data Logger (Nền tảng)

> **Mục tiêu:** Lưu lại mọi phân tích, quyết định, kết quả. Đây là "bộ nhớ" của hệ thống.

#### 1.1 Architecture

```
bonbo-journal/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── models.rs           — TradeJournalEntry, TradeOutcome, AnalysisSnapshot
    ├── journal.rs          — JournalStore (SQLite backend)
    ├── analysis_log.rs     — Lưu full snapshot phân tích tại thời điểm quyết định
    ├── performance.rs      — Accuracy tracking + indicator correlation
    └── mcp_tools.rs        — MCP tool implementations
```

#### 1.2 Data Models — Enhanced from v1.0

```rust
/// Snapshot phân tích tại thời điểm quyết định — CỐT LÕI cho learning
struct AnalysisSnapshot {
    // Market Context
    symbol: String,
    price: f64,
    timestamp: i64,
    fear_greed_index: f64,
    market_regime: MarketRegime,         // TrendingUp/Down/Ranging/Volatile/Quiet
    
    // Technical Indicators (RAW values — không chỉ signals)
    rsi_14: f64,
    macd_line: f64,
    macd_signal: f64,
    macd_histogram: f64,
    bb_percent_b: f64,
    bb_upper: f64,
    bb_lower: f64,
    atr_14: f64,
    ema_12: f64,
    ema_26: f64,
    sma_20: f64,
    
    // Signals
    buy_signals_count: u32,
    sell_signals_count: u32,
    signal_details: Vec<SignalDetail>,  // Mỗi signal: source, type, confidence
    
    // Sentiment
    composite_sentiment: f64,            // -1.0 to +1.0
    whale_alert_count_24h: u32,
    
    // Scoring
    quant_score: f64,                     // 0-100
    scoring_weights_hash: String,        // Hash of weights used → for tracking
    
    // Backtest validation
    backtest_return: f64,
    backtest_sharpe: f64,
    backtest_winrate: f64,
    backtest_max_drawdown: f64,
}

struct SignalDetail {
    source: String,       // "RSI(14)", "MACD(12,26,9)", "BB(20,2)"
    signal_type: String,  // Buy/Sell/Neutral
    confidence: f64,      // 0-1
    reason: String,
}

struct TradeJournalEntry {
    id: String,                          // UUID
    timestamp: i64,
    
    // Analysis snapshot at decision time
    snapshot: AnalysisSnapshot,
    
    // Decision
    recommendation: Recommendation,      // STRONG_BUY/BUY/HOLD/SELL/STRONG_SELL
    entry_price: f64,
    stop_loss: f64,
    target_price: f64,
    risk_reward_ratio: f64,
    position_size_usd: f64,
    
    // Outcome (điền sau)
    outcome: Option<TradeOutcome>,
}

struct TradeOutcome {
    close_timestamp: i64,
    exit_price: f64,
    actual_return_pct: f64,
    hit_target: bool,
    hit_stoploss: bool,
    holding_period_hours: u32,
    max_favorable_excursion: f64,        // MFE — max profit reached
    max_adverse_excursion: f64,          // MAE — max loss reached
    direction_correct: bool,
    score_accuracy: f64,
    // Per-indicator accuracy
    indicator_accuracy: HashMap<String, bool>,  // "RSI" → correct?, "MACD" → correct?
}

enum Recommendation {
    StrongBuy,
    Buy,
    Hold,
    Sell,
    StrongSell,
}
```

#### 1.3 Performance Metrics — MỚI so với v1.0

```rust
struct LearningMetrics {
    // Overall accuracy
    total_predictions: u32,
    direction_accuracy: f64,             // % dự đoán đúng hướng
    avg_score_error: f64,                // |predicted_return - actual_return|
    
    // Per-indicator accuracy (KEY for learning)
    rsi_accuracy: f64,                   // RSI predicted correctly → higher weight
    macd_accuracy: f64,
    bb_accuracy: f64,
    signals_accuracy: f64,
    regime_accuracy: f64,
    sentiment_accuracy: f64,
    backtest_accuracy: f64,
    
    // Per-regime accuracy
    accuracy_by_regime: HashMap<MarketRegime, RegimeAccuracy>,
    
    // Risk metrics
    sharpe_of_predictions: f64,          // Sharpe of following our recommendations
    sortino_of_predictions: f64,
    max_drawdown_of_predictions: f64,
    profit_factor: f64,
    
    // Statistical significance
    p_value: f64,                        // Is our accuracy significantly > 50%?
    confidence_interval_95: (f64, f64),  // 95% CI of our accuracy
}
```

#### 1.4 MCP Tools (4 tools)

| Tool | Mô tả |
|------|-------|
| `journal_trade_entry` | Lưu phân tích + quyết định vào journal |
| `journal_trade_outcome` | Cập nhật kết quả thực tế cho trade |
| `get_trade_journal` | Truy vấn journal history (filter by symbol, date, regime) |
| `get_learning_metrics` | Thống kê accuracy, per-indicator, per-regime |

---

### Phase 2: bonbo-regime — Regime Detection Engine (MỚI)

> **Mục tiêu:** Phát hiện market regime trong real-time, là nền tảng cho regime-aware learning.

#### 2.1 Architecture

```
bonbo-regime/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── hmm.rs             — Hidden Markov Model (2-4 states)
    ├── bocpd.rs           — Bayesian Online Change Point Detection
    ├── indicators.rs      — Regime indicators (volatility, trend, correlation)
    ├── classifier.rs      — RegimeClassifier combining HMM + BOCPD + indicators
    └── models.rs          — MarketRegime, RegimeState, ChangePoint
```

#### 2.2 Algorithms

| Algorithm | Role | Complexity | Status |
|-----------|------|-----------|--------|
| **BOCPD** (Adams & MacKay 2007) | Real-time change point detection | O(n) online | Priority 1 — implement first |
| **Volatility regime** (ATR-based) | Quick regime proxy | O(1) | Already in bonbo-ta |
| **Statistical test** (CUSUM/ADR) | Drift detection trigger | O(1) | Medium priority |
| **HMM** (3-4 states) | Offline regime classification | O(n·k²) | Phase 2+ |

#### 2.3 BOCPD — Implementation Priority

```rust
/// Bayesian Online Change Point Detection
/// Streams data points and detects regime changes in real-time
struct BocpdDetector {
    // Run-length distribution
    run_length_probs: Vec<f64>,           // P(r_t | data)
    
    // Hazard function (exponential)
    hazard_rate: f64,                     // default: 1/250 (expect change every 250 candles)
    
    // Observation model (Student-t for financial returns)
    mu0: f64,                             // Prior mean
    kappa0: f64,                          // Prior precision scaling
    alpha0: f64,                          // Prior degrees of freedom
    beta0: f64,                           // Prior scale
    
    // Sufficient statistics (incremental update)
    stats: Vec<SufficientStats>,
    
    // Detected change points
    change_points: Vec<ChangePoint>,
}

struct ChangePoint {
    timestamp: i64,
    candle_index: usize,
    confidence: f64,                      // Probability of change point
    prev_regime: MarketRegime,
    new_regime: MarketRegime,
}

impl BocpdDetector {
    /// Process one new data point — O(n) where n = current run length
    fn update(&mut self, value: f64, timestamp: i64) -> Option<ChangePoint>;
    
    /// Get current most likely regime
    fn current_regime(&self) -> MarketRegime;
    
    /// Get probability of regime change at current step
    fn change_probability(&self) -> f64;
}
```

#### 2.4 MCP Tools (3 tools)

| Tool | Mô tả |
|------|-------|
| `detect_regime` | Real-time regime detection cho 1 symbol |
| `get_change_points` | Lịch sử regime changes |
| `get_regime_probability` | Probability of being in each regime |

---

### Phase 3: bonbo-learning — Ensemble Learning Engine (NÂNG CẤP)

> **Mục tiêu:** Học từ dữ liệu quá khứ bằng DMA (Dynamic Model Averaging) + Thompson Sampling, với regime-aware weights.

#### 3.1 Architecture

```
bonbo-learning/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── dma.rs             — Dynamic Model Averaging (Bayesian weight adaptation)
    ├── thompson.rs        — Thompson Sampling for strategy selection
    ├── ensemble.rs        — EnsembleScorer combining DMA + regime + rules
    ├── weights.rs         — ScoringWeights with regime-specific sets
    ├── overfitting.rs     — DSR, PBO, Haircut Sharpe computation
    ├── drift.rs           — Concept drift detection (ADWIN-like)
    └── models.rs          — LearningConfig, LearningState, LearningReport
```

#### 3.2 Dynamic Model Averaging (DMA) — Core Learning Algorithm

Thay vì weight adaptation ±0.02 đơn thuần, DMA dùng **full Bayesian posterior**:

```rust
/// Dynamic Model Averaging — Bayesian weight adaptation
/// Reference: Raftery et al. (2010), eDMA R package
struct DynamicModelAveraging {
    // Models (one per indicator/signal source)
    models: Vec<DmaModel>,
    
    // Two forgetting factors — KEY innovation from research
    alpha: f64,   // Forgetting factor for model parameters (0.95-0.999)
    lambda: f64,  // Forgetting factor for model weights (0.9-0.99)
    
    // Current model weights (posterior probabilities)
    weights: Vec<f64>,                    // Sum to 1.0
    
    // Regime-specific weight sets
    regime_weights: HashMap<MarketRegime, Vec<f64>>,
    
    // History for analysis
    weight_history: Vec<WeightSnapshot>,
}

struct DmaModel {
    name: String,                         // "RSI", "MACD", "BB", "Sentiment", etc.
    
    // Prediction tracking
    predictions: Vec<PredictionRecord>,
    recent_accuracy: f64,                 // Rolling accuracy (last 50 predictions)
    
    // Bayesian parameters
    posterior_mean: f64,
    posterior_variance: f64,
    
    // Performance metrics
    cumulative_log_score: f64,            // Log predictive score
}

struct WeightSnapshot {
    timestamp: i64,
    regime: MarketRegime,
    weights: Vec<f64>,
    trigger: String,                      // "scheduled", "drift_detected", "manual"
}
```

#### 3.3 Learning Algorithm — Step by Step

```
Khi có trade outcome mới:

1. REGIME CHECK
   - BOCPD: Có regime change không?
   - Nếu có → Lưu change point, chuẩn bị weight set mới
   - Nếu không → Tiếp tục với current regime weights

2. INDICATOR ACCURACY UPDATE
   - Cho mỗi indicator (RSI, MACD, BB, Sentiment, etc.):
     - So sánh prediction vs actual outcome
     - Cập nhật Bayesian posterior (DMA):
       posterior_mean = α * prior_mean + (1-α) * new_evidence
       posterior_var = α² * prior_var + (1-α²) * noise_var

3. WEIGHT UPDATE (DMA)
   - Tính predictive likelihood cho mỗi model
   - Update weights với forgetting factor λ:
     new_weights[i] ∝ λ * old_weights[i] * likelihood[i]
   - Normalize: sum = 1.0
   - Clip: min 0.03 (không để weight = 0 hoàn toàn)
   - Guard: backtest_weight luôn ≥ 0.10

4. SAFETY CHECKS
   - Minimum 20 outcomes trước khi bắt đầu tune
   - Max weight change: ±0.05 per cycle (tăng từ ±0.02)
   - If accuracy < 45% → revert to default weights (overfitting signal)
   - Compute DSR after every 50 trades → check if overfitted

5. REGIME-SPECIFIC WEIGHTS
   - Nếu regime = "Ranging" → tăng BB_weight, giảm trend_weight
   - Nếu regime = "Trending" → tăng MACD/EMA_weight, giảm RSI_weight
   - Nếu regime = "Volatile" → tăng ATR_weight, giảm signal_weight
```

#### 3.4 Overfitting Prevention — 4 Layers

```
Layer 1 — Pre-Deployment:
  ▸ Deflated Sharpe Ratio (DSR): Correct for multiple testing
  ▸ Probability of Backtest Overfitting (PBO): pypbo-style calculation
  ▸ Haircut Sharpe: Discount all Sharpe by 50%
  ▸ Minimum Backtest Length: 2x number of strategies tested

Layer 2 — Model Architecture:
  ▸ Ensemble approach (DMA) — naturally resists overfitting
  ▸ Bayesian priors with financial domain knowledge
  ▸ Constraint: minimum weight per indicator (0.03)
  ▸ Constraint: backtest_weight always ≥ 0.10

Layer 3 — Online Safeguards:
  ▸ Drift-triggered updates only (ADWIN/BOCPD)
  ▸ Conservative learning rates (α ∈ [0.95, 0.999])
  ▸ Accuracy < 45% → revert to defaults
  ▸ Track 8-12% degradation benchmark

Layer 4 — Monitoring:
  ▸ Rolling Sharpe of predictions (must stay > 0)
  ▸ Weight change audit log
  ▸ Regime accuracy dashboard
  ▸ Minimum 20 outcomes before first tuning
```

#### 3.5 MCP Tools (5 tools)

| Tool | Mô tả |
|------|-------|
| `get_scoring_weights` | Xem weights hiện tại (tổng + per-regime) |
| `get_learning_stats` | DMA state, accuracy trend, weight change history |
| `manual_weight_override` | User override weights (với justification) |
| `compute_overfitting_metrics` | DSR, PBO, Haircut Sharpe cho strategies |
| `reset_learning` | Reset weights về defaults + clear history |

---

### Phase 4: bonbo-validation — Validation Framework (MỚI)

> **Mục tiêu:** Đảm bảo hệ thống thực sự cải thiện, không phải random luck.

#### 4.1 Architecture

```
bonbo-validation/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── cpcv.rs            — Combinatorial Purged Cross-Validation
    ├── walk_forward.rs    — Enhanced walk-forward with purging + embargoing
    ├── deflated_sharpe.rs — Deflated Sharpe Ratio computation
    ├── pbo.rs             — Probability of Backtest Overfitting
    └── report.rs          — ValidationReport with statistical tests
```

#### 4.2 Key Metrics

```rust
struct ValidationReport {
    // CPCV Results
    cpcv_sharpe_distribution: Vec<f64>,  // Distribution (not single point!)
    cpcv_mean_sharpe: f64,
    cpcv_sharpe_std: f64,
    
    // Deflated Sharpe Ratio
    deflated_sharpe_ratio: f64,
    dsr_p_value: f64,                    // Probability Sharpe > 0 after correction
    number_of_trials: u32,               // How many strategies tested
    
    // Probability of Backtest Overfitting
    pbo: f64,                            // P(selected strategy underperforms median)
    
    // Haircut Sharpe
    original_sharpe: f64,
    haircut_sharpe: f64,                 // ~50% of original
    haircut_pct: f64,
    
    // Walk-Forward
    wf_sharpe_train: f64,
    wf_sharpe_test: f64,
    wf_degradation: f64,                 // train - test (benchmark: 8-12%)
    
    // Statistical Significance
    is_statistically_significant: bool,  // p < 0.05 after all corrections
    minimum_track_record: u32,           // Min trades needed for confidence
}
```

#### 4.3 MCP Tools (3 tools)

| Tool | Mô tả |
|------|-------|
| `validate_strategy_cpcv` | CPCV validation cho 1 strategy |
| `compute_deflated_sharpe` | DSR cho multiple strategies |
| `get_validation_report` | Full validation report |

---

### Phase 5: bonbo-scanner — Autonomous Scanner + AI Agent Integration

> **Mục tiêu:** Tự động quét, phân tích, chấm điểm, journal, alert — với self-learning loop.

#### 5.1 Auto-prompt Loop — Enhanced

```
[Every 4 hours — Market Scan]
1. get_top_crypto(20)                    → Quét top 20
2. detect_regime(BTC, ETH, SOL)          → Xác định regime hiện tại
3. FOR each coin:
   analyze_indicators                    → Full TA
   get_trading_signals                   → Signals
   get_composite_sentiment               → Sentiment
   run_backtest                          → Validate
   compute_score(regime_weights)         → Regime-aware scoring
4. journal_trade_entry(top_picks)         → Ghi nhận mọi phân tích
5. IF score ≥ 65: alert_user()           → Thông báo cơ hội

[Every 24 hours — Learning Review]
1. get_trade_journal(last_7_days)        → Xem predictions gần đây
2. FOR each pending trade:
   check_actual_outcome()                → So sánh với reality
   journal_trade_outcome()               → Cập nhật kết quả
3. get_learning_metrics()                → Review accuracy
4. IF accuracy < 60%:
   review_indicator_accuracy()           → Indicators nào đang sai?
   update_weights_via_dma()              → DMA adapts weights
5. compute_overfitting_metrics()         → Check không overfitted
6. get_scoring_weights()                 → Verify weights hợp lý

[Every 7 days — Strategy Discovery]
1. discover_strategies(top_20_coins)     → 8 strategies × 3 timeframes
2. validate_strategy_cpcv(top_3)         → CPCV validate top findings
3. compute_deflated_sharpe(all_results)  → Adjust for multiple testing
4. Update best_strategy_per_coin_regime  → Apply findings
```

---

## 4. SO SÁNHH V1.0 vs V2.0

| Khía cạnh | V1.0 (cũ) | V2.0 (mới) |
|-----------|-----------|------------|
| **Weight adaptation** | Heuristic ±0.02 | DMA (Bayesian posterior) |
| **Regime awareness** | Không có | HMM + BOCPD + regime-specific weights |
| **Overfitting prevention** | Chỉ min 20 trades | 4 layers: DSR + PBO + Haircut + CPCV |
| **Validation** | Không có | CPCV + Walk-Forward + DSR |
| **Drift detection** | Không có | BOCPD + ADWIN-like |
| **Indicator accuracy** | Chỉ direction_correct | Per-indicator + per-regime accuracy |
| **Ensemble approach** | Không | DMA ensemble weights |
| **Historical learning** | Chỉ tune weights | Full Bayesian updating + weight history |
| **Statistical rigor** | Không | p-value, CI, significance tests |

---

## 5. IMPLEMENTATION PRIORITY

### Phase 1: bonbo-journal (2-3 ngày)
**Tại sao trước nhất:** Không thể học nếu không có dữ liệu. Phase này tạo nền tảng.
- [ ] Tạo crate bonbo-journal
- [ ] Models: TradeJournalEntry, TradeOutcome, AnalysisSnapshot
- [ ] JournalStore với SQLite backend
- [ ] Performance metrics tracking
- [ ] 4 MCP tools
- [ ] Unit tests

### Phase 2: bonbo-regime (2-3 ngày)
**Tại sao tiếp theo:** Regime-aware learning cần biết regime trước khi adapt weights.
- [ ] Tạo crate bonbo-regime
- [ ] BOCPD implementation (Adams & MacKay 2007)
- [ ] Simple volatility-based regime proxy
- [ ] RegimeClassifier
- [ ] 3 MCP tools
- [ ] Unit tests + validation against known regime periods

### Phase 3: bonbo-learning (3-4 ngày)
**Tại sao giai đoạn chính:** Đây là "bộ não" learning.
- [ ] Tạo crate bonbo-learning
- [ ] DMA implementation (Raftery et al. 2010)
- [ ] Regime-specific weight sets
- [ ] Overfitting metrics (DSR, PBO)
- [ ] Drift detection
- [ ] 5 MCP tools
- [ ] Unit tests + integration with journal + regime

### Phase 4: bonbo-validation (2-3 ngày)
**Tại sao cần:** Chứng minh hệ thống thực sự hoạt động.
- [ ] Tạo crate bonbo-validation
- [ ] CPCV implementation
- [ ] Deflated Sharpe Ratio
- [ ] PBO computation
- [ ] 3 MCP tools
- [ ] Tests with synthetic + real data

### Phase 5: bonbo-scanner + Integration (2-3 ngày)
**Tại sao cuối:** Cần tất cả components trước khi tự động hóa.
- [ ] Scanner với regime-aware scoring
- [ ] Auto-prompt loop
- [ ] AI Agent system prompt
- [ ] Full integration test
- [ ] Documentation

---

## 6. CRATE DEPENDENCY GRAPH

```
bonbo-journal ──────┐
                    │
bonbo-regime ───────┼──▶ bonbo-learning ──▶ bonbo-scanner
                    │         │
bonbo-ta ──────────┤         ▼
bonbo-data ────────┤    bonbo-validation
bonbo-quant ───────┤
bonbo-sentinel ────┤
bonbo-risk ────────┘
                    │
                    ▼
              bonbo-extend-mcp (35+ MCP tools total)
```

---

## 7. RISKS & MITIGATION

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Overfitting to small sample | HIGH | Critical | DSR + PBO + minimum 20 trades + 50% Sharpe haircut |
| Regime detection wrong | MEDIUM | High | BOCPD confidence threshold + multi-signal confirmation |
| DMA weights oscillating | MEDIUM | Medium | Forgetting factor tuning + max change per cycle |
| Missing data/outages | LOW | Medium | Fallback to last known good weights |
| Cold start (no data) | HIGH | Low | Start with expert weights + backtest validation |

---

## 8. SUCCESS METRICS

| Metric | Target | Measurement |
|--------|--------|-------------|
| Direction accuracy | > 55% (vs 50% random) | After 100 trades |
| Regime detection accuracy | > 70% | Validated against historical regime labels |
| Sharpe of recommendations | > 0.5 (annualized) | CPCV validated |
| PBO (overfitting) | < 0.3 | After strategy discovery |
| Weight stability | < 10% change per week | In stable regimes |
| System uptime | > 99% | Scanner availability |

---

## 9. REFERENCES (Key Papers)

| # | Paper | Year | Relevance |
|---|-------|------|-----------|
| 1 | DoubleAdapt (KDD) | 2023 | Meta-learning for incremental stock prediction |
| 2 | OneNet (NeurIPS) | 2023 | RL-weighted ensemble for time series |
| 3 | BOCPD (Adams & MacKay) | 2007 | Real-time change point detection |
| 4 | DMA (Raftery et al.) | 2010 | Dynamic Model Averaging for prediction |
| 5 | CPCV (López de Prado) | 2018 | Gold standard financial CV |
| 6 | Deflated Sharpe (Bailey et al.) | 2014 | Multiple testing correction |
| 7 | PBO (Bailey et al.) | 2015 | Backtest overfitting probability |
| 8 | Tsallis-INF (Zimmert & Seldin) | 2021 | Optimal bandit for stochastic + adversarial |
| 9 | AE-DIL | 2023 | Double incremental learning with ensemble |
| 10 | RD-Agent | 2025 | MAB for self-improving quant |
