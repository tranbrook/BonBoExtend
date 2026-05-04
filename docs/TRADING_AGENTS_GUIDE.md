# 🧠 TRADING AGENTS — Hướng dẫn sử dụng vào Workflow thực tế

> Hướng dẫn chi tiết cách tích hợp Debate Engine, Decision Journal, Self-Reflection và Episodic Memory vào quy trình giao dịch hàng ngày.

---

## 📋 TỔNG QUAN KIẾN TRÚC

```
┌──────────────────────────────────────────────────────────────────┐
│                    WORKFLOW GIAO DỊCH                            │
│                                                                  │
│  Bước 1: best_trade / momentum_scan  →  Tìm cơ hội              │
│  Bước 2: best_entry                   →  Phân tích entry         │
│  Bước 3: Hard Guards (Rust)           →  Kiểm tra rủi ro cứng   │
│  Bước 4: Debate Engine                →  Bull vs Bear → Risk     │
│  Bước 5: Decision Journal             →  Lưu quyết định          │
│  Bước 6: Execute                       →  Đặt lệnh                │
│  Bước 7: Update Outcome               →  Cập nhật kết quả        │
│  Bước 8: Self-Reflection              →  Rút kinh nghiệm         │
│  Bước 9: Episodic Memory              →  Lưu bài học              │
└──────────────────────────────────────────────────────────────────┘
```

---

## BƯỚC 1: QUÉT THỊ TRƯỜNG — Tìm cơ hội

### 1.1 Chạy best_trade (quét 87 coins)

```bash
BINANCE_MARKET_TYPE=futures target/release/examples/best_trade
```

**Output mẫu:**
```
🏆 TOP 5 SCORING:
1. CHZUSDT    Score: 87  | RSI: 35 (oversold) | Hurst: 0.67 (trending) | Funding: -0.02%
2. MEGAUSDT   Score: 83  | RSI: 42            | Hurst: 0.61           | Funding: 0.01%
3. PENGUUSDT  Score: 79  | RSI: 48            | Hurst: 0.55           | Funding: -0.01%
4. ORDIUSDT   Score: 76  | RSI: 52            | Hurst: 0.58           | Funding: 0.005%
5. ENSOUSDT   Score: 72  | RSI: 55            | Hurst: 0.50           | Funding: 0.01%

📊 Market Sentiment: Fear & Greed = 45 (Fear)
📈 Regime: Trending Up (Hurst > 0.55)
```

**Thông tin thu thập được để truyền vào Debate:**
- Ticker: `CHZUSDT`
- Market summary: `"RSI oversold at 35, Hurst 0.67 trending, volume surge 3x, funding negative -0.02%"`
- Analyst reports từ best_trade output

---

### 1.2 Chạy best_entry (phân tích chi tiết top pick)

```bash
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry CHZUSDT
```

**Output mẫu:**
```
═══════════════════════════════════════════════
  🎯 CHZUSDT — DEEP ANALYSIS
═══════════════════════════════════════════════

💰 Entry:  $0.0847 (BUY)
🛡️  SL:    $0.0778 (-8.1%)
🎯 TP1:   $0.1003 (+18.4%)  | R:R = 2.3:1
🎯 TP2:   $0.1120 (+32.2%)  | R:R = 4.0:1
🎯 TP3:   $0.1250 (+47.6%)  | R:R = 5.9:1

📊 Kelly Size: 8.2% equity

┌─ Multi-TF Breakdown ─────────────────────┐
│ TF    │ RSI  │ MACD    │ BB%B │ ALMA     │
│ 1D    │ 42   │ Bull ✅  │ 0.28 │ Bull ✅  │
│ 4H    │ 35   │ Bull ✅  │ 0.15 │ Bear ❌  │
│ 1H    │ 48   │ Bear ❌  │ 0.45 │ Bull ✅  │
│ 15M   │ 55   │ Bull ✅  │ 0.52 │ Bull ✅  │
└───────────────────────────────────────────┘

📈 Derivatives:
  Funding: -0.02% (shorts paying → bullish)
  OI Change 24h: +8.5%
  L/S Ratio: 0.45 (more shorts → contrarian bullish)
  Taker B/S: 1.35 (buying dominance)
```

---

## BƯỚC 2: CHẠY DEBATE ENGINE — Bull vs Bear

### 2.1 Chuẩn bị input cho Debate

Từ Bước 1, bạn có các thông tin sau:

```text
ticker = "CHZUSDT"
market_summary = "RSI oversold at 35, Hurst 0.67 trending up, volume surge 3x,
                  funding -0.02%, L/S ratio 0.45, OI +8.5%, taker B/S 1.35,
                  Fear & Greed = 45"
analyst_reports = [
  "Technical: BUY — oversold bounce with volume confirmation",
  "Sentiment: NEUTRAL — F&G = 45 (Fear zone)",
  "Derivatives: BULLISH — negative funding + low L/S + rising OI",
]
```

### 2.2 Chạy Research Debate (Bull vs Bear)

**Mục đích:** Hai bên Bull và Bear tranh luận, tìm ra hướng giao dịch tốt nhất.

```rust
use bonbo_debate::{DebateEngine, DebateConfig};

// Tạo engine với config mặc định (3 rounds research, 2 rounds risk)
let engine = DebateEngine::rule_based(DebateConfig::default());

// Hoặc dùng config nhanh nếu cần quyết định gấp
// let engine = DebateEngine::rule_based(DebateConfig::fast());

// Chạy Research Debate
let research = engine.run_research_debate(
    "CHZUSDT",
    "RSI oversold at 35, Hurst 0.67 trending up, volume surge 3x, \
     funding -0.02%, L/S ratio 0.45, OI +8.5%, taker B/S 1.35",
    &[
        "Technical: BUY signal — oversold bounce with volume".to_string(),
        "Sentiment: NEUTRAL — Fear & Greed at 45".to_string(),
        "Derivatives: BULLISH — negative funding + contrarian L/S".to_string(),
    ],
).await?;
```

**Kết quả research debate:**
```text
Consensus: Buy
Strength:  0.78
Rounds:    2 (early termination — bull thắng rõ ràng)

Bull arguments:
  • Round 1: "Bullish signal: oversold" (confidence: 0.75)
  • Round 1: "Bullish signal: trending up" (confidence: 0.75)
  • Round 1: "Bullish signal: momentum" (confidence: 0.75)
  → Bull Researcher: BUY với confidence 0.75

Bear arguments:
  • Round 1: "No strong bearish signals detected" (confidence: 0.50)
  → Bear Researcher: HOLD với confidence 0.25

→ Weighted: Bull (0.75 × 0.5) > Bear (0.25 × 0.5) → BUY
```

### 2.3 Chạy Risk Debate (Aggressive vs Neutral vs Conservative)

**Mục đích:** 3 risk debators quyết định position size và rủi ro.

```rust
// Tiếp tục từ research debate ở trên
let risk = engine.run_risk_debate(
    "CHZUSDT",
    &research,
    "Funding -0.02%, OI +8.5%, L/S ratio 0.45, F&G = 45",
).await?;
```

**Kết quả risk debate:**
```text
Risk Level:      Medium
Direction:       Buy
Size Multiplier: 0.15 (15% of Kelly)

Aggressive:   BUY  | size: 0.25 | "Follow signals with larger size"
Neutral:      BUY  | size: 0.15 | "Balanced approach"
Conservative: HOLD | size: 0.00 | "Not strong enough for my standards"

→ Majority: BUY (2/3 vote Buy)
→ Average size: (0.25 + 0.15 + 0.00) / 3 = 0.133 → clamped to 0.15
→ Risk Level: Medium (0.1 < 0.15 < 0.2)
```

### 2.4 Ý nghĩa các config

| Config | Research Rounds | Risk Rounds | Consensus Threshold | Khi nào dùng |
|--------|----------------|-------------|---------------------|--------------|
| `DebateConfig::default()` | 3 | 2 | 0.8 | **Mặc định** — cân bằng |
| `DebateConfig::research()` | 5 | 3 | 0.9 | Khi cần phân tích sâu |
| `DebateConfig::fast()` | 1 | 1 | 0.7 | Khi cần quyết định nhanh |

---

## BƯỚC 3: HARD GUARDS — Kiểm tra rủi ro cứng

Trước khi ghi quyết định, Rust hard guards kiểm tra:

```text
✅ Max position size:     8.2% ≤ 25%     PASS
✅ Max drawdown:          Portfolio OK     PASS
✅ Volatility spike:      ATR normal       PASS
✅ Max leverage:          5x ≤ 10x         PASS
✅ Min liquidity:         $50M > $10M      PASS
✅ Max daily loss:        -1.2% > -5%      PASS
```

Hard guards luôn chạy trong Rust code — **không thể bypass bởi LLM hay debate.**

---

## BƯỚC 4: GHI DECISION JOURNAL — Lưu quyết định

### 4.1 Tạo và lưu TradeDecision

```rust
use bonbo_decision_journal::{DecisionJournal, JournalConfig};
use bonbo_llm_types::journal::{TradeDecision, MarketSnapshot};
use bonbo_llm_types::{Rating, RiskLevel, TradeDirection};
use rust_decimal_macros::dec;

// Mở journal (tạo SQLite DB tự động)
let journal = DecisionJournal::open(&JournalConfig::default())?;

// Tạo trade decision từ kết quả debate + best_entry
let decision = TradeDecision {
    decision_id: format!("dec_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S")),
    ticker: "CHZUSDT".to_string(),
    action: TradeDirection::Buy,               // Từ debate consensus
    rating: Rating::Buy,                       // Từ research
    confidence: dec!(0.78),                    // Từ debate strength
    agent_votes: vec![],                       // Bull/Bear votes
    reasoning: format!(
        "Research debate: BUY (strength 0.78). Bull args: oversold, trending, momentum. \
         Risk debate: Medium risk, size 15% Kelly. Derivatives: bullish bias."
    ),
    market_context: MarketSnapshot {
        price: dec!(0.0847),
        volume_24h: dec!(150000000),
        rsi_4h: Some(dec!(35)),
        rsi_1d: Some(dec!(42)),
        hurst_4h: Some(dec!(0.67)),
        fear_greed: Some(45),
        funding_rate: Some(dec!(-0.0002)),
        regime: "Trending Up".to_string(),
    },
    entry_price: dec!(0.0847),
    stop_loss: dec!(0.0778),
    take_profits: vec![
        // TP1, TP2, TP3 từ best_entry
    ],
    position_size: dec!(0.082),             // 8.2% equity
    position_size_pct: dec!(8),
    risk_reward_ratio: dec!(2.3),
    guard_result: /* hard guard result */,
    risk_level: RiskLevel::Medium,
    debate_rounds: research.rounds_completed + risk.rounds_completed,
    regime: "Trending Up".to_string(),
    strategy: "ALMA Crossover".to_string(),
    outcome: None,                           // Chưa có kết quả
    reflection: None,                        // Chưa có reflection
    timestamp: chrono::Utc::now(),
};

// Lưu vào journal
let result = journal.store_decision(&decision)?;
println!("✅ Decision saved: {} at {}", result.decision_id, result.stored_at);
```

**Database:** `~/.bonbo/journal/decisions.db` (SQLite)

---

## BƯỚC 5: EXECUTE — Đặt lệnh giao dịch

Dựa trên kết quả debate và best_entry:

```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  📋 TRADE EXECUTION PLAN
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Symbol:    CHZUSDT
  Direction: LONG
  Entry:     $0.0847
  SL:        $0.0778 (-8.1%)
  TP1:       $0.1003 (+18.4%)  ← Đặt take profit
  TP2:       $0.1120 (+32.2%)
  TP3:       $0.1250 (+47.6%)
  Size:      8.2% equity × 0.15 (risk debate) = 1.23% equity
  R:R:       2.3:1
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

---

## BƯỚC 6: UPDATE OUTCOME — Cập nhật kết quả

Sau khi trade đóng (hit TP hoặc SL), cập nhật outcome:

### 6.1 Lưu Trade Outcome

```rust
use bonbo_llm_types::journal::{TradeOutcome, ExitReason};

let outcome = TradeOutcome {
    exit_price: dec!(0.1003),              // Hit TP1
    pnl_pct: dec!(18.4),                   // +18.4%
    pnl_absolute: dec!(226),               // $226 profit
    holding_hours: dec!(36),               // 36 hours
    mfe_pct: dec!(22.5),                   // Max favor: +22.5%
    mae_pct: dec!(4.2),                    // Max adverse: -4.2%
    exit_reason: ExitReason::TakeProfit1,  // Hit TP1
    exit_timestamp: chrono::Utc::now(),
};

journal.update_outcome("dec_20260503_083000", &outcome)?;
println!("✅ Outcome updated");
```

---

## BƯỚC 7: SELF-REFLECTION — Rút kinh nghiệm tự động

### 7.1 Chạy TradeReflector

```rust
use bonbo_debate::reflection::{TradeReflector, ReflectionInput};

let reflector = TradeReflector::default_matcher();

let reflection_output = reflector.reflect(&ReflectionInput {
    decision: decision.clone(),   // TradeDecision đã lưu
    outcome: outcome.clone(),     // TradeOutcome vừa update
});
```

### 7.2 Xem kết quả reflection

```rust
println!("═══ SELF-REFLECTION ═══");
println!("Ticker: {}", reflection_output.ticker);

println!("\n✅ What went well:");
for well in &reflection_output.what_went_well {
    println!("  • {}", well);
}
// Output:
//   • Trade profitable: 18.40%
//   • Ran significantly in our favor

println!("\n🔧 Improvements:");
for imp in &reflection_output.improvements {
    println!("  • {}", imp);
}
// Output:
//   • Left money on table: MFE=22.5% vs actual=18.4%

println!("\n📝 Lesson: {}", reflection_output.lesson);
// Output: "Winning Buy trade in Trending Up regime with ALMA Crossover strategy.
//          debated confidence was 78%."

println!("\n🔄 Would repeat: {}", reflection_output.would_repeat);
// Output: true

println!("\n📌 Patterns matched:");
for pattern in &reflection_output.analogous_situations {
    println!("  • {}", pattern);
}
// Output:
//   • oversold_bounce: Oversold RSI + support bounce
```

### 7.3 Lưu reflection vào Journal

```rust
use bonbo_debate::reflection::TradeReflector;

let reflection = TradeReflector::to_trade_reflection(&reflection_output);
journal.add_reflection("dec_20260503_083000", &reflection)?;
println!("✅ Reflection saved to journal");
```

---

## BƯỚC 8: EPISODIC MEMORY — Lưu bài học cho tương lai

### 8.1 Lưu Episode

```rust
use bonbo_memory::{MemoryStore, MemoryConfig, episode::{Episode, EpisodeOutcome}};

let store = MemoryStore::open(MemoryConfig::default())?;

let episode = Episode {
    id: "ep_20260503_chz_long".to_string(),
    ticker: "CHZUSDT".to_string(),
    regime: "Trending Up".to_string(),
    strategy: "ALMA Crossover".to_string(),
    direction: "LONG".to_string(),
    context_description: "Oversold bounce from support with volume surge 3x, \
                          negative funding shorts paying longs, low L/S ratio contrarian bullish".to_string(),
    tags: vec![
        "oversold".to_string(),
        "support".to_string(),
        "volume_surge".to_string(),
        "negative_funding".to_string(),
        "contrarian".to_string(),
    ],
    entry_price: "0.0847".to_string(),
    stop_loss: "0.0778".to_string(),
    outcome: EpisodeOutcome {
        is_win: true,
        pnl_pct: 18.4,
        exit_reason: "take_profit_1".to_string(),
        mfe_pct: 22.5,
        mae_pct: 4.2,
    },
    lesson: "Oversold bounces in trending regime with negative funding and low L/S \
             ratio are high-probability longs. Wait for RSI < 35 + volume surge.".to_string(),
    entry_confidence: 0.78,
    rsi_4h: Some(35.0),
    hurst_4h: Some(0.67),
    fear_greed: Some(45),
    pnl_pct: 18.4,
    holding_hours: 36.0,
    debate_rounds: 4,   // 2 research + 2 risk
    created_at: chrono::Utc::now(),
};

store.store(&episode)?;
println!("✅ Episode saved to memory");
```

**Database:** `~/.bonbo/memory/episodes.db` (SQLite)

---

## 🔁 VÒNG LẶP: SỬ DỤNG MEMORY CHO TRADE TIẾP THEO

### Trước khi vào trade mới → Kiểm tra Memory

```rust
use bonbo_memory::{MemoryStore, MemoryConfig, retrieval::MemoryRetrieval};

let store = MemoryStore::open(MemoryConfig::default())?;
let retrieval = MemoryRetrieval::new(&store, 0.3); // min similarity 30%
```

#### A. Tìm trade tương tự trong quá khứ

```rust
// Tìm những lần trước đã trade oversold bounce
let similar = retrieval.find_similar(
    "CHZUSDT oversold trending up volume surge",  // Mô tả tình huống hiện tại
    5,                                              // Tối đa 5 kết quả
)?;

println!("═══ SIMILAR PAST TRADES ═══");
for scored in &similar {
    println!(
        "  {} | sim: {:.0}% | outcome: {} | PnL: {:.1}% | lesson: {}",
        scored.episode.ticker,
        scored.similarity * 100.0,
        if scored.episode.outcome.is_win { "✅ WIN" } else { "❌ LOSS" },
        scored.episode.outcome.pnl_pct,
        scored.episode.lesson,
    );
}

// Output mẫu:
//   CHZUSDT  | sim: 85% | outcome: ✅ WIN  | PnL: 18.4% | lesson: Oversold bounces work...
//   ETHUSDT  | sim: 72% | outcome: ✅ WIN  | PnL: 12.3% | lesson: Trend following works...
//   BTCUSDT  | sim: 65% | outcome: ❌ LOSS | PnL: -5.2% | lesson: Regime change caught...
```

#### B. Kiểm tra win rate theo regime + strategy

```rust
let stats = retrieval.regime_strategy_win_rate("Trending Up", "ALMA Crossover")?;

println!("═══ REGIME + STRATEGY STATS ═══");
println!("  Regime:   {}", stats.regime);
println!("  Strategy: {}", stats.strategy);
println!("  Win rate: {:.1}% ({}/{})",
    stats.win_rate * 100.0, stats.wins, stats.total_trades);
println!("  Avg PnL:  {:.2}%", stats.avg_pnl_pct);

// Output mẫu:
//   Regime:   Trending Up
//   Strategy: ALMA Crossover
//   Win rate: 62.5% (10/16)
//   Avg PnL:  +3.8%
```

**→ Nếu win rate < 50% hoặc avg PnL âm → cân nhắc KHÔNG dùng strategy này trong regime này!**

#### C. Xem lịch sử trade theo ticker

```rust
let btc_trades = retrieval.find_by_ticker("CHZUSDT", 10)?;

println!("═══ CHZUSDT TRADE HISTORY ═══");
for trade in &btc_trades {
    println!(
        "  {} | {} | {} | PnL: {:.1}% | {}",
        trade.created_at.format("%Y-%m-%d"),
        trade.direction,
        trade.regime,
        trade.outcome.pnl_pct,
        if trade.outcome.is_win { "✅" } else { "❌" },
    );
}
```

---

## 📊 WORKFLOW HOÀN CHỈNH — Ví dụ từ đầu đến cuối

```
════════════════════════════════════════════════════════════════
  NGÀY 2026-05-03 — WORKFLOW GIAO DỊCH
════════════════════════════════════════════════════════════════

07:00  📡 Bước 1: Quét thị trường
       $ best_trade → TOP 5: CHZUSDT(87), MEGAUSDT(83), PENGUUSDT(79)

07:15  🔍 Bước 2: Deep analysis
       $ best_entry CHZUSDT → Entry $0.0847, SL $0.0778, TP1 $0.1003

07:20  🧠 Bước 3: Debate Engine
       Research: Bull ✅ (0.78) vs Bear (0.22) → BUY
       Risk:     Aggressive(Buy 25%), Neutral(Buy 15%), Conservative(Hold)
       → BUY với size 15% Kelly

07:22  🛡️  Bước 4: Hard Guards
       ✅ Tất cả checks PASSED

07:23  📝 Bước 5: Decision Journal
       ✅ Saved dec_20260503_072300

07:25  💰 Bước 6: Execute
       LONG CHZUSDT @ $0.0847 | SL $0.0778 | TP1 $0.1003
       Size: 1.23% equity ($123 trên $10,000 account)

────── chờ 36 giờ ──────

2026-05-04 19:25  🎯 Hit TP1 @ $0.1003 (+18.4%)

19:26  📊 Bước 7: Update Outcome
       ✅ PnL: +$226 | Holding: 36h | MFE: 22.5% | MAE: 4.2%

19:27  🪞 Bước 8: Self-Reflection
       ✅ What went well: Trade profitable, ran in favor
       🔧 Improvement: Left money on table (MFE 22.5% vs actual 18.4%)
       📝 Lesson: "Winning Buy trade in Trending Up with ALMA Crossover"
       🔄 Would repeat: YES
       📌 Pattern: oversold_bounce (historical win rate: 62%)

19:28  🧠 Bước 9: Episodic Memory
       ✅ Episode saved: "Oversold bounces in trending regime with
          negative funding are high-probability longs"

════════════════════════════════════════════════════════════════
  NGÀY 2026-05-05 — TRADE MỚI (sử dụng memory)
════════════════════════════════════════════════════════════════

07:00  📡 Quét thị trường
       $ best_trade → TOP: PENGUUSDT(85), ETHUSDT(82)

07:15  🔍 Deep analysis PENGUUSDT
       $ best_entry PENGUUSDT → RSI 32, oversold, trending, volume surge

07:20  🧠 Kiểm tra Memory TRƯỚC khi debate
       retrieval.find_similar("PENGUUSDT oversold trending volume surge", 5)
       → Tìm thấy 3 trade tương tự:
         ✅ CHZUSDT oversold: +18.4% (similarity: 72%)
         ✅ ETHUSDT oversold: +12.3% (similarity: 65%)
         ❌ BTCUSDT oversold: -5.2%  (similarity: 55%)
         → Lesson: "Regime change" — kiểm tra Hurst divergence trước!

       retrieval.regime_strategy_win_rate("Trending Up", "ALMA Crossover")
       → Win rate: 62.5% (10/16) → CẨN THẬN nhưng OK

07:25  🧠 Debate Engine (có thêm memory context)
       Research: Bull ✅ (0.82) — stronger confidence nhờ memory confirmation
       Risk:     Neutral(Buy 15%)
       → BUY với size 12% Kelly (hơi nhỏ vì BTCUSDT loss trước đó)

... tiếp tục workflow như trên ...
```

---

## 🔧 CRYPTO-SPECIFIC DEBATORS — Phân tích chuyên sâu

### Derivatives Bias

```rust
use bonbo_debate::crypto::CryptoDebateContext;

let ctx = CryptoDebateContext {
    ticker: "CHZUSDT".to_string(),
    btc_dominance: 52.0,
    funding_rate: -0.02,          // Shorts paying → BULLISH
    oi_change_24h: 8.5,           // Rising OI → interest tăng
    long_short_ratio: 0.45,       // Nhiều short hơn → contrarian BULLISH
    taker_buy_sell_ratio: 1.35,   // Buying dominance → BULLISH
    top_trader_ls_ratio: 1.2,
    liquidation_24h_usd: 50_000_000.0,
    mcap_rank: Some(85),
    defi_tvl: None,
    active_addresses_24h: Some(50000),
    whale_tx_count_24h: Some(15),
    network_vol_change_pct: Some(25.0),
    exchange_netflow: Some(-2000.0),  // Outflow = accumulation → BULLISH
    market_summary: "CHZ trending up with volume surge".to_string(),
    fear_greed_index: 45,
};

// Derivatives bias
let bias = ctx.derivatives_bias();
match &bias {
    DerivativesBias::Bullish { confidence } => {
        println!("📈 Derivatives: BULLISH (confidence: {:.0}%)", confidence * 100.0);
        // → Truyền thêm thông tin "bullish" vào debate
    }
    DerivativesBias::Bearish { confidence } => {
        println!("📉 Derivatives: BEARISH (confidence: {:.0}%)", confidence * 100.0);
    }
    DerivativesBias::Neutral => {
        println!("➡️  Derivatives: NEUTRAL");
    }
}

// On-chain health
let health = ctx.onchain_health();
println!("🔗 On-chain health: {} ({})", health.score, health.assessment);
// Output: "On-chain health: 85 (Healthy)"
```

**Bảng logic Derivatives Bias:**

| Chỉ số | Giá trị | Signal | Weight |
|--------|---------|--------|--------|
| Funding rate | < -0.01 | 🟢 Bullish (shorts paying) | +2 |
| Funding rate | -0.01 → 0 | 🟡 Mild bullish | +1 |
| Funding rate | > 0.05 | 🔴 Bearish (overleveraged longs) | +2 |
| Funding rate | 0.01 → 0.05 | 🟡 Mild bearish | +1 |
| OI change | > +5% | 🟢 Bullish (interest tăng) | +1 |
| OI change | < -5% | 🔴 Bearish (interest giảm) | +1 |
| L/S ratio | < 0.5 | 🟢 Bullish (contrarian — quá nhiều short) | +2 |
| L/S ratio | > 2.0 | 🔴 Bearish (contrarian — quá nhiều long) | +2 |
| Taker B/S | > 1.2 | 🟢 Bullish (buying dominance) | +1 |
| Taker B/S | < 0.8 | 🔴 Bearish (selling dominance) | +1 |
| Exchange netflow | < 0 | 🟢 Bullish (outflow = accumulation) | +1 |
| Exchange netflow | > 0 | 🔴 Bearish (inflow = selling) | +1 |

---

## 🪞 PATTERN MATCHER — 6 Patterns tự động nhận diện

| Pattern | Mô tả | Win Rate | Lời khuyên |
|---------|--------|----------|------------|
| `oversold_bounce` | RSI < 35 + support bounce | 62% | Chờ RSI divergence xác nhận trước khi entry |
| `breakout_continuation` | Breakout với volume confirmation | 55% | Đảm bảo volume > 2x average |
| `funding_squeeze` | Extreme funding + contrarian trade | 58% | Size nhỏ — squeeze có thể kéo dài hơn dự kiến |
| `regime_change` | Trade trước khi regime shift | 40% | ⚠️ Luôn kiểm tra Hurst divergence trước entry |
| `chase_momentum` | Đuổi momentum đã overextended | 35% | ⚠️ Chờ pullback về VWAP/EMA trước entry |
| `mean_reversion_extreme` | Extreme deviation from mean | 60% | Dùng SL rộng + size nhỏ |

**Tự động nhận diện khi:**
- RSI 4H < 35 → `oversold_bounce`
- RSI 4H > 70 + losing → `chase_momentum`
- |Funding| > 3% → `funding_squeeze`
- PnL < -5% → `regime_change`

---

## 🗄️ DATABASE LOCATIONS

| Component | Database | Location |
|-----------|----------|----------|
| Decision Journal | `decisions.db` | `~/.bonbo/journal/decisions.db` |
| Episodic Memory | `episodes.db` | `~/.bonbo/memory/episodes.db` |

### Xem dữ liệu trực tiếp bằng SQLite:

```bash
# Xem decisions
sqlite3 ~/.bonbo/journal/decisions.db \
  "SELECT decision_id, ticker, action, confidence, regime, strategy, created_at FROM decisions ORDER BY created_at DESC LIMIT 10;"

# Xem outcomes
sqlite3 ~/.bonbo/journal/decisions.db \
  "SELECT decision_id, json_extract(outcome, '$.pnl_pct') as pnl, json_extract(outcome, '$.exit_reason') as exit FROM decisions WHERE outcome IS NOT NULL;"

# Xem reflections
sqlite3 ~/.bonbo/journal/decisions.db \
  "SELECT decision_id, lesson, would_repeat FROM reflections ORDER BY created_at DESC LIMIT 10;"

# Xem episodes
sqlite3 ~/.bonbo/memory/episodes.db \
  "SELECT id, ticker, regime, strategy, direction, pnl_pct, lesson FROM episodes ORDER BY created_at DESC LIMIT 10;"

# Win rate theo ticker
sqlite3 ~/.bonbo/memory/episodes.db \
  "SELECT ticker, COUNT(*) as total, SUM(CASE WHEN pnl_pct > 0 THEN 1 ELSE 0 END) as wins, ROUND(AVG(pnl_pct), 2) as avg_pnl FROM episodes GROUP BY ticker ORDER BY total DESC;"
```

---

## ⚡ TÓM TẮT — Checklist nhanh

### Trước khi vào trade:
- [ ] Chạy `best_trade` → tìm cơ hội
- [ ] Chạy `best_entry <SYMBOL>` → phân tích entry
- [ ] Kiểm tra **Memory** → tìm trade tương tự trong quá khứ
- [ ] Kiểm tra **win rate** regime + strategy → có đáng không?
- [ ] Chạy **Debate Engine** → Bull vs Bear → Risk debate
- [ ] Kiểm tra **Derivatives Bias** → thêm confirm
- [ ] **Hard Guards** passed → OK để trade
- [ ] Lưu **Decision Journal** → ghi lại quyết định

### Sau khi trade đóng:
- [ ] Update **Outcome** vào journal
- [ ] Chạy **Self-Reflection** → rút kinh nghiệm tự động
- [ ] Lưu **Reflection** vào journal
- [ ] Lưu **Episode** vào memory → bài học cho tương lai

### Mỗi tuần:
- [ ] Review win rate theo regime/strategy
- [ ] Xem pattern nào xuất hiện nhiều nhất
- [ ] Điều chỉnh DebateConfig nếu cần

---

⚠️ **Disclaimer:** Đây là công cụ phân tích định lượng, KHÔNG phải lời khuyên tài chính. Luôn quản lý rủi ro cẩn thận!
