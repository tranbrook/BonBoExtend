# 🧠 KẾ HOẠCH: BONBO AI SELF-LEARNING TRADING LOOP

## Tầm nhìn

Xây dựng **vòng lặp tự học (self-learning loop)** cho BonBo AI Agent, trong đó AI:
1. **Tự thu thập dữ liệu** thị trường định kỳ
2. **Tự phân tích** bằng các công cụ định lượng có sẵn
3. **Tự đề xuất** giao dịch dựa trên scoring
4. **Tự backtest** để kiểm chứng hypothesis
5. **Tự ghi nhận** kết quả vào Knowledge Base để học từ thành công/thất bại
6. **Tự tối ưu** scoring weights dựa trên kinh nghiệm tích lũy

---

## Kiến trúc Vòng lặp

```
┌─────────────────────────────────────────────────────────────────────┐
│                    BONBO AI SELF-LEARNING LOOP                     │
│                                                                     │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐     │
│  │  1. SCAN │───►│ 2.ANALYZE│───►│ 3.DECIDE │───►│ 4.BACK-  │     │
│  │  Market  │    │ Quant    │    │ Score &  │    │ TEST      │     │
│  │  Data    │    │ Analysis │    │ Rank     │    │ Validate  │     │
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘     │
│       ▲                                               │            │
│       │              ┌──────────┐                     │            │
│       │              │ 6.TUNE   │◄────────            │            │
│       │              │ Weights  │                     │            │
│       │              └────┬─────┘                     │            │
│       │                   ▲                           │            │
│       │                   │                           ▼            │
│       │              ┌──────────┐              ┌──────────┐        │
│       └──────────────│ 5.LEARN  │◄─────────────│  LOG     │        │
│         next cycle   │ Knowledge│  Results     │  Trade   │        │
│                      │ Base     │              │  Journal │        │
│                      └──────────┘              └──────────┘        │
│                                                                     │
│  Tools sử dụng:                                                     │
│  ──────────────                                                     │
│  1. SCAN:     get_top_crypto, get_crypto_price, WebSocket stream   │
│  2. ANALYZE:  analyze_indicators, get_trading_signals,             │
│               detect_market_regime, get_support_resistance,        │
│               get_fear_greed_index, get_composite_sentiment,       │
│               get_whale_alerts                                     │
│  3. DECIDE:   compute_risk_metrics, calculate_position_size        │
│  4. BACKTEST: run_backtest (nhiều strategies, nhiều timeframes)    │
│  5. LEARN:    knowledge_add, knowledge_search, knowledge_hybrid    │
│  6. TUNE:     Adapt weights dựa trên accuracy history               │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Giai đoạn triển khai

### Phase 1: Trade Journal & Data Logger (Nền tảng)

**Mục tiêu:** Lưu lại mọi phân tích, quyết định, và kết quả để học từ quá khứ.

#### 1.1 Tạo crate mới `bonbo-journal`

```
bonbo-journal/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── models.rs        — TradeJournalEntry, TradeOutcome
    ├── journal.rs       — JournalStore (SQLite backend)
    ├── analysis_log.rs  — Lưu snapshot phân tích tại thời điểm quyết định
    └── performance.rs   — Tracking accuracy: forecast vs reality
```

#### 1.2 Data models

```rust
/// Một entry trong trade journal — snapshot tại thời điểm ra quyết định
struct TradeJournalEntry {
    id: String,                    // UUID
    timestamp: i64,                // Unix timestamp
    
    // Market Context
    symbol: String,
    price_at_analysis: f64,
    fear_greed_index: f64,         // 0-100
    market_regime: String,         // uptrend/downtrend/ranging
    
    // Technical Snapshot
    rsi_14: f64,
    macd_signal: String,           // bullish/bearish
    bb_percent_b: f64,
    buy_signals_count: u32,
    sell_signals_count: u32,
    
    // Decision
    recommendation: String,        // STRONG_BUY/BUY/HOLD/SELL/STRONG_SELL
    quant_score: f64,              // 0-100
    entry_price: f64,
    stop_loss: f64,
    target_price: f64,
    risk_reward_ratio: f64,
    position_size_usd: f64,
    
    // Backtest Validation
    backtest_return: f64,
    backtest_sharpe: f64,
    backtest_winrate: f64,
    
    // Outcome (điền sau khi close position hoặc sau X ngày)
    outcome: Option<TradeOutcome>,
}

struct TradeOutcome {
    close_timestamp: i64,
    exit_price: f64,
    actual_return_pct: f64,
    hit_target: bool,
    hit_stoploss: bool,
    holding_period_hours: u32,
    // Accuracy
    direction_correct: bool,       // Dự đoán đúng hướng?
    score_accuracy: f64,           // |predicted_return - actual_return|
}
```

#### 1.3 MCP Tools mới

| Tool | Mô tả |
|------|-------|
| `journal_trade_entry` | Lưu phân tích + quyết định vào journal |
| `journal_trade_outcome` | Cập nhật kết quả thực tế cho trade đã ghi |
| `get_trade_journal` | Truy vấn journal history |
| `get_trading_accuracy` | Thống kê accuracy: win rate, avg return, forecast error |

---

### Phase 2: Scheduled Scanner — Vòng lặp thu thập tự động

**Mục tiêu:** AI tự động chạy phân tích định kỳ mà không cần user prompt.

#### 2.1 Tạo crate mới `bonbo-scanner`

```
bonbo-scanner/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── scheduler.rs     — Cron-like scheduling (tokio timer)
    ├── scanner.rs       — Quét top coins + phân tích hàng loạt
    ├── screener.rs      — Scoring + ranking (logic từ quant_screener.py)
    └── alert.rs         — Push notification khi phát hiện cơ hội
```

#### 2.2 Vòng lặp chính

```rust
/// Vòng lặp quét thị trường định kỳ
/// Chạy mỗi 4 giờ: quét top 20 → phân tích → chấm điểm → alert + journal
async fn scan_loop() {
    loop {
        // 1. Collect
        let coins = get_top_coins(20).await;
        let sentiment = get_composite_sentiment().await;
        
        // 2. Analyze each coin
        for coin in coins {
            let analysis = full_analysis(coin).await;  // TA + signals + regime
            let score = compute_score(&analysis, &weights);
            let backtest = run_backtest(coin, "4h").await;
            
            // 3. Journal the analysis (ghi nhận để học)
            journal_entry(analysis, score, backtest).await;
            
            // 4. Alert if high-score opportunity
            if score >= 65 {
                send_alert(coin, score, analysis).await;
            }
        }
        
        // 5. Review past predictions
        review_past_predictions().await;
        
        // 6. Tune weights based on accuracy
        weights = tune_scoring_weights().await;
        
        // Sleep 4 hours
        tokio::time::sleep(Duration::from_secs(4 * 3600)).await;
    }
}
```

#### 2.3 MCP Tools mới

| Tool | Mô tả |
|------|-------|
| `start_scanner` | Bắt đầu vòng lặp scanner (chạy ngầm) |
| `stop_scanner` | Dừng scanner |
| `get_scanner_status` | Trạng thái scanner + latest scan results |
| `get_top_opportunities` | Lấy top N coins từ lần scan gần nhất |

---

### Phase 3: Learning Engine — Học từ kinh nghiệm

**Mục tiêu:** AI tự điều chỉnh scoring weights dựa trên kết quả thực tế.

#### 3.1 Weight Adaptation

```rust
/// Scoring weights — tự điều chỉnh dựa trên accuracy history
#[derive(Serialize, Deserialize)]
struct ScoringWeights {
    // Indicator weights (tổng = 1.0)
    rsi_weight: f64,          // default: 0.15
    macd_weight: f64,         // default: 0.10
    bb_weight: f64,           // default: 0.10
    signals_weight: f64,      // default: 0.15
    regime_weight: f64,       // default: 0.08
    risk_reward_weight: f64,  // default: 0.10
    backtest_weight: f64,     // default: 0.15
    sentiment_weight: f64,    // default: 0.10
    momentum_weight: f64,     // default: 0.07
    
    // Thresholds
    buy_threshold: f64,       // default: 60
    strong_buy_threshold: f64, // default: 70
    sell_threshold: f64,      // default: 40
}
```

#### 3.2 Learning Algorithm

```
Mỗi khi có trade outcome mới:
1. So sánh prediction (quant_score, direction) vs actual outcome
2. Tính accuracy từng indicator:
   - RSI predicted correctly? → increase rsi_weight
   - RSI predicted wrongly? → decrease rsi_weight
   - Tương tự cho MACD, BB, signals, regime, sentiment...
3. Normalize weights (sum = 1.0)
4. Lưu vào Knowledge Base

Quy tắc:
- Minimum 20 trade outcomes trước khi bắt đầu tune
- Chỉ tune nếu accuracy < 60% (tránh overfitting trên few data)
- Maximum weight change per cycle: ±0.02 (tránh drastic swings)
- Luôn giữ backtest_weight ≥ 0.10 (backtest là ground truth)
```

#### 3.3 MCP Tools mới

| Tool | Mô tả |
|------|-------|
| `get_scoring_weights` | Xem weights hiện tại |
| `get_learning_stats` | Accuracy, số trades đã học, weight change history |
| `manual_weight_override` | Cho phép user override weights |
| `reset_learning` | Reset weights về defaults |

---

### Phase 4: Strategy Discovery — Khám phá chiến lược mới

**Mục tiêu:** AI tự thử nghiệm nhiều chiến lược, chọn ra chiến lược tốt nhất cho từng coin/regime.

#### 4.1 Strategy Matrix

```
┌──────────────────────────────────────────────────────┐
│  STRATEGY DISCOVERY ENGINE                           │
│                                                       │
│  8 strategies × 20 coins × 3 timeframes = 480 tests  │
│                                                       │
│  Strategies:                                          │
│  1. SMA Crossover (10/30)                            │
│  2. SMA Crossover (5/20)                             │
│  3. EMA Crossover (12/26)                            │
│  4. RSI Mean Reversion (30/70)                       │
│  5. RSI Mean Reversion (25/75)                       │
│  6. BB Bounce (lower band buy, upper band sell)      │
│  7. MACD + RSI Confluence                            │
│  8. Multi-indicator Composite (custom weights)       │
│                                                       │
│  Timeframes: 1h, 4h, 1d                              │
│  Periods: last 200 candles                            │
│                                                       │
│  Output: Best strategy per coin per regime            │
│  ────────                                            │
│  BTC  ranging → BB Bounce 4h      (Sharpe 1.2)      │
│  BTC  uptrend → SMA Cross 1d      (Sharpe 0.8)      │
│  ETH  ranging → RSI Reversion 4h  (Sharpe 1.5)      │
│  SOL  downtrend → MACD+RSI 1h     (Sharpe 0.9)      │
│  ...                                                 │
└──────────────────────────────────────────────────────┘
```

#### 4.2 MCP Tools mới

| Tool | Mô tả |
|------|-------|
| `discover_strategies` | Chạy 480 tests, trả về best strategy per coin |
| `get_best_strategy` | Truy vấn best strategy cho 1 coin + regime |
| `get_strategy_history` | Lịch sử strategy performance |
| `validate_strategy` | Forward-test 1 strategy trên dữ liệu mới nhất |

---

### Phase 5: BonBo AI Agent Integration

**Mục tiêu:** Kết nối toàn bộ vào BonBo AI Agent để AI tự chủ.

#### 5.1 System Prompt cho AI Agent

```markdown
Bạn là BonBo Quant Trading AI. Mỗi 4 giờ, bạn tự động:

1. **SCAN**: Quét top 20 crypto coins
2. **ANALYZE**: Phân tích kỹ thuật + sentiment + on-chain
3. **SCORE**: Chấm điểm mỗi coin (0-100) bằng weights đã học
4. **BACKTEST**: Validate top picks với backtest
5. **JOURNAL**: Ghi nhận mọi phân tích + quyết định vào trade journal
6. **ALERT**: Thông báo user khi phát hiện cơ hội score ≥ 65
7. **LEARN**: Khi có kết quả thực tế, so sánh với dự đoán → tune weights
8. **ADAPT**: Điều chỉnh chiến lược theo market regime

**Quy tắc BẮT BUỘC:**
- LUÔN ghi journal trước khi đưa ra recommendation
- KHÔNG BAO GIỜ recommendation mà không có stop loss
- Circuit breaker:暂停 nếu thua 3 trades liên tiếp
- Chỉ trade khi Risk/Reward ≥ 1:1.5
- Maximum 3 positions đồng thời
```

#### 5.2 Auto-prompt Loop

```
AI Agent tự gọi MCP tools theo chu trình:

[Every 4 hours]
→ start_scanner (hoặc get_scanner_status nếu đang chạy)
→ get_top_opportunities (lấy kết quả scan gần nhất)
→ analyze_indicators (phân tích sâu top 3 picks)
→ run_backtest (validate)
→ journal_trade_entry (ghi nhận)
→ notify_user (nếu có cơ hội tốt)

[Every 24 hours — Review]
→ get_trade_journal (xem past predictions)
→ journal_trade_outcome (update outcomes cho trades đã qua)
→ get_trading_accuracy (review accuracy)
→ get_learning_stats (xem weights đã thay đổi thế nào)
→ discover_strategies (chạy weekly strategy discovery)

[On-demand — User asks]
→ Phân tích theo yêu cầu + sử dụng weights đã học
→ Explain reasoning dựa trên historical accuracy
```

---

## Timeline triển khai

```
Week 1-2:  Phase 1 — Trade Journal & Data Logger
           ├── Tạo bonbo-journal crate
           ├── SQLite schema cho journal + outcomes
           ├── 4 MCP tools: journal_entry, journal_outcome, get_journal, get_accuracy
           └── Tests

Week 3-4:  Phase 2 — Scheduled Scanner
           ├── Tạo bonbo-scanner crate
           ├── Port quant_screener.py → Rust (hoặc gọi Python subprocess)
           ├── Scheduler: 4h scan cycle
           ├── 4 MCP tools: start/stop_scanner, get_status, get_opportunities
           └── Integration test: full scan cycle

Week 5-6:  Phase 3 — Learning Engine
           ├── ScoringWeights struct + persistence
           ├── Weight adaptation algorithm
           ├── Accuracy tracking per indicator
           ├── 4 MCP tools: get_weights, get_learning_stats, override, reset
           └── Test: simulate 50 trades → verify weight tuning

Week 7-8:  Phase 4 — Strategy Discovery
           ├── 8 strategy implementations
           ├── Strategy matrix runner (480 combinations)
           ├── Best strategy selection per coin/regime
           ├── 4 MCP tools: discover, get_best, get_history, validate
           └── Performance benchmark

Week 9-10: Phase 5 — AI Agent Integration
           ├── System prompt design
           ├── Auto-prompt loop configuration
           ├── End-to-end test: full learning cycle
           ├── Documentation
           └── Deploy to production
```

---

## Tổng quan MCP Tools mới

| Phase | Tool | Mô tả |
|-------|------|-------|
| **1** | `journal_trade_entry` | Lưu phân tích + quyết định |
| **1** | `journal_trade_outcome` | Cập nhật kết quả thực tế |
| **1** | `get_trade_journal` | Truy vấn journal history |
| **1** | `get_trading_accuracy` | Thống kê accuracy |
| **2** | `start_scanner` | Bắt đầu quét định kỳ |
| **2** | `stop_scanner` | Dừng scanner |
| **2** | `get_scanner_status` | Trạng thái scanner |
| **2** | `get_top_opportunities` | Top picks từ scan gần nhất |
| **3** | `get_scoring_weights` | Xem weights hiện tại |
| **3** | `get_learning_stats` | Learning stats |
| **3** | `manual_weight_override` | Override weights |
| **3** | `reset_learning` | Reset weights |
| **4** | `discover_strategies` | Khám phá best strategies |
| **4** | `get_best_strategy` | Best strategy cho coin |
| **4** | `get_strategy_history` | Strategy history |
| **4** | `validate_strategy` | Forward-test strategy |
| | **Tổng: 16 tools mới** | (hiện có 21 → tổng 37 tools) |

---

## Kiến trúc Crate mới

```
BonBoExtend/
├── Cargo.toml                    # Workspace
├── bonbo-ta/                     ✅ Phase A — DONE
├── bonbo-data/                   ✅ Phase B — DONE
├── bonbo-quant/                  ✅ Phase C — DONE
├── bonbo-sentinel/               ✅ Phase D — DONE
├── bonbo-risk/                   ✅ Phase E — DONE
├── bonbo-extend/                 ✅ Plugin system + tools
├── bonbo-extend-mcp/             ✅ MCP server
├── bonbo-journal/                📋 Phase 1 — Trade journal (NEW)
├── bonbo-scanner/                📋 Phase 2 — Scheduled scanner (NEW)
├── scripts/
│   └── quant_screener.py         ✅ Existing
└── docs/
    ├── ARCHITECTURE.md           ✅
    ├── QUANT_ARCHITECTURE.md     ✅
    ├── phantichtop20.md          ✅
    └── SELF_LEARNING_PLAN.md     📋 This document
```

---

## Ví dụ: Vòng lặp Self-Learning trong thực tế

```
═══ Cycle 1 (Day 1, 08:00) ═══
SCAN: NEAR score=71, ETC score=63, MATIC score=59
JOURNAL: "NEAR — Buy@1.40, SL=1.19, TGT=1.68, Score=71"
JOURNAL: "ETC — Buy@8.74, SL=8.08, TGT=9.08, Score=63"
ALERT: "🟢 NEAR quant score 71 — STRONG BUY"

═══ Cycle 2 (Day 1, 12:00) ═══
SCAN: NEAR score=68 (slight decrease), ETC score=65 (increase)
REVIEW: No outcomes yet — still waiting

═══ Cycle 3 (Day 2, 08:00) ═══
SCAN: NEAR price = $1.52 (hit Target 1!)
OUTCOME: NEAR — direction_correct=true, return=+8.6%, score_accuracy=good
LEARN: RSI weight stays, BB weight increases (BB%B was good predictor)
WEIGHTS: bb_weight: 0.10 → 0.12, signals_weight: 0.15 → 0.14

═══ Cycle 4 (Day 3, 08:00) ═══
SCAN: ETC price = $7.95 (hit stop loss)
OUTCOME: ETC — direction_correct=false, return=-7.6%
LEARN: MACD was bullish but wrong → decrease macd_weight
WEIGHTS: macd_weight: 0.10 → 0.08, backtest_weight stays (backtest was flat, correctly cautious)

═══ ... after 50 trades ... ═══
ACCURACY: Direction correct: 62% (improving from initial 50%)
BEST INDICATOR: BB%B (accuracy 68%) → weight increased to 0.18
WORST INDICATOR: MACD (accuracy 45%) → weight decreased to 0.06
BEST STRATEGY per COIN:
  BTC ranging → BB Bounce (Sharpe 1.4)
  ETH uptrend → SMA Cross 1d (Sharpe 1.1)
  NEAR ranging → MACD+RSI Confluence (Sharpe 1.8)
```

---

## Rủi ro & Giải pháp

| Rủi ro | Giải pháp |
|--------|-----------|
| Overfitting weights trên ít data | Minimum 20 trades trước khi tune, max ±0.02 change/cycle |
| Market regime thay đổi nhanh | Detect regime change → reset weights về defaults |
| API rate limits (Binance) | Cache aggressive, stagger requests, batch processing |
| AI hallucination trong analysis | Mọi decision phải có quant_score + backtest validation |
| Loss streak | Circuit breaker: pause sau 3 consecutive losses |
| Data staleness | Timestamp mọi entry, alert nếu data > 1h cũ |

---

> **Kế hoạch này biến BonBo từ "công cụ phân tích bị động" thành "AI trader tự học chủ động".**
> Mỗi cycle, AI giỏi hơn một chút — giống cách một trader thực sự tích lũy kinh nghiệm.
