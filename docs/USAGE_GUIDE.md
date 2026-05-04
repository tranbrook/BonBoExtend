# BonBoExtend v0.3.0 — Hướng dẫn Build, Test & Sử dụng

> Quantitative Crypto Trading Platform — 22 crates, 63K+ LOC, 760 tests

---

## 📊 Tổng quan hệ thống

```
╔══════════════════════════════════════════════════════════════╗
║  BonBoExtend v0.3.0 — Quantitative Crypto Platform         ║
╠══════════════════════════════════════════════════════════════╣
║  📦 22 crates | 📝 63,463 LOC | 🧪 760 tests (0 fail)     ║
║  📦 Binary: bonbo-agent (3.9MB) + bonbo-extend-mcp (7.1MB) ║
║  ⚡ 10 analysis examples | 🔧 35+ MCP tools                ║
╚══════════════════════════════════════════════════════════════╝
```

---

## 1. BUILD

### 1.1 Build toàn bộ workspace

```bash
cd ~/BonBoExtend

# Build debug (nhanh)
cargo build --workspace

# Build release (tối ưu, khuyến nghị)
cargo build --workspace --release
```

### 1.2 Build từng component

```bash
# MCP server
cargo build --release -p bonbo-extend-mcp

# Live trading agent
cargo build --release -p bonbo-agent

# Debate engine
cargo build --release -p bonbo-debate

# Decision journal
cargo build --release -p bonbo-decision-journal

# Episodic memory
cargo build --release -p bonbo-memory
```

### 1.3 Binary outputs

| Binary | Size | Vị trí | Mô tả |
|--------|------|--------|-------|
| `bonbo-agent` | 3.9MB | `target/release/bonbo-agent` | Live trading agent + Telegram alerts |
| `bonbo-extend-mcp` | 7.1MB | `target/release/bonbo-extend-mcp` | MCP server (35+ tools) |
| `best_trade` | 2.3MB | `target/release/examples/best_trade` | Full market scanner |
| `best_entry` | 2.2MB | `target/release/examples/best_entry` | Single coin deep analysis |
| `momentum_scan` | 2.1MB | `target/release/examples/momentum_scan` | Momentum long/short scanner |
| `advanced_scan` | 2.4MB | `target/release/examples/advanced_scan` | Advanced multi-strategy scan |
| `btc_analysis` | 2.5MB | `target/release/examples/btc_analysis` | BTC full analysis |
| `market_scan` | 2.3MB | `target/release/examples/market_scan` | Market overview scan |
| `xau_full_analysis` | 2.0MB | `target/release/examples/xau_full_analysis` | Gold (XAU) analysis |

---

## 2. TEST

### 2.1 Chạy tests

```bash
# Chạy tất cả 760 tests
cargo test --workspace

# Chạy với output chi tiết
cargo test --workspace -- --nocapture

# Chạy tests cho 1 crate cụ thể
cargo test -p bonbo-llm-types        # 10 tests — LLM shared types
cargo test -p bonbo-debate            # 14 tests — Debate engine + reflection + patterns
cargo test -p bonbo-decision-journal  # 8 tests — Journal CRUD
cargo test -p bonbo-memory            # 8 tests — Episode store + retrieval
cargo test -p bonbo-ta                # 236 tests — Technical indicators
cargo test -p bonbo-quant             # 15 tests — Backtesting engine
cargo test -p bonbo-risk              # 56 tests — Risk management

# Chạy 1 test cụ thể
cargo test -p bonbo-debate -- test_reflect_winning_trade --nocapture
```

### 2.2 Lint & Format

```bash
# Clippy lint
cargo clippy --workspace -- -W clippy::all

# Format check
cargo fmt --all -- --check

# Auto-fix format
cargo fmt --all
```

### 2.3 Kết quả benchmark

```
✅ 760 tests PASSED
❌ 0 failures
⏭️  6 ignored (require API keys)
🔧 0 clippy errors
📝 63,463 lines of Rust code
```

---

## 3. MCP SERVER (35+ Tools)

### 3.1 Chạy MCP Server

```bash
# Stdio mode (cho AI agents như Claude, BonBo)
BINANCE_MARKET_TYPE=futures target/release/bonbo-extend-mcp

# Hoặc qua cargo
BINANCE_MARKET_TYPE=futures cargo run --release -p bonbo-extend-mcp
```

### 3.2 Các MCP Tools

#### Market Data
| Tool | Mô tả |
|------|--------|
| `get_klines` | Lấy nến OHLCV (1m, 5m, 15m, 1h, 4h, 1d, 1w) |
| `get_ticker_price` | Giá hiện tại |
| `get_orderbook` | Order book depth |
| `get_recent_trades` | Trades gần đây |

#### Technical Analysis
| Tool | Mô tả |
|------|--------|
| `analyze_position_full` | Phân tích kỹ thuật đầy đủ (RSI, MACD, BB, ALMA, Hurst, ADX, CMO, LaguerreRSI) |
| `calculate_rsi` | RSI indicator |
| `calculate_macd` | MACD indicator |
| `calculate_ema` / `calculate_sma` | Moving averages |
| `calculate_bollinger_bands` | Bollinger Bands |
| `calculate_atr` | Average True Range |
| `calculate_alma` | Arnaud Legoux Moving Average |
| `calculate_super_smoother` | Super Smoother Filter |
| `get_hurst_exponent` | Hurst exponent (regime detection) |

#### Sentiment & Derivatives
| Tool | Mô tả |
|------|--------|
| `get_composite_sentiment` | Sentiment tổng hợp |
| `get_fear_greed_index` | Fear & Greed Index |
| `get_derivatives_data` | Funding rate, OI, L/S ratio |
| `get_top_trader_positions` | Top trader long/short |
| `get_taker_buy_sell_volume` | Taker buy/sell ratio |

#### Backtesting & Strategy
| Tool | Mô tả |
|------|--------|
| `backtest_strategy` | Backtest chiến lược trên dữ liệu lịch sử |
| `get_strategy_recommendation` | Đề xuất chiến lược phù hợp regime |

### 3.3 Sử dụng MCP qua JSON-RPC

```json
// Initialize
{"jsonrpc": "2.0", "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "my-client", "version": "1.0"}}, "id": "0"}

// Call tool
{"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "analyze_position_full", "arguments": {"symbol": "BTCUSDT"}}, "id": "1"}
```

---

## 4. CÔNG CỤ PHÂN TÍCH (Examples)

### 4.1 `best_trade` — Quét toàn thị trường

**Mục đích:** Quét 87 coins futures, xếp hạng, deep analysis top 5 + backtest validation.

```bash
BINANCE_MARKET_TYPE=futures target/release/examples/best_trade
```

**Output:**
- Market Sentiment (Fear & Greed)
- Multi-TF scanning 87 symbols (1H × 30% + 4H × 40% + 1D × 30%)
- Bảng xếp hạng TOP 15 theo score
- Deep Analysis TOP 5: per-TF breakdown + Laguerre Divergence + Derivatives
- Backtest Validation: SMA Crossover, ALMA Crossover, Hurst Regime-Switch
- Risk Management: ATR×regime SL + Kelly position sizing

**Khi nào dùng:** Mỗi buổi sáng để tìm cơ hội giao dịch trong ngày.

---

### 4.2 `best_entry` — Phân tích entry point tối ưu

**Mục đích:** Phân tích chi tiết 1 coin — entry/SL/TP + multi-TF indicators + derivatives.

```bash
# Cú pháp
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry <SYMBOL>

# Ví dụ
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry BTCUSDT
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry SOLUSDT
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry PENGUUSDT
```

**Output:**
- **Entry price** (BUY/SELL)
- **Stop Loss** (ATR-based)
- **Take Profit 1/2/3** với R:R ratio
- **Kelly position size** (% equity)
- **Multi-TF breakdown** (1D/4H/1H/15M):
  - RSI, MACD (bullish/bearish crossover)
  - BB %B (vị trí trong band)
  - ALMA cross (bullish/bearish)
  - LaguerreRSI (overbought/oversold)
  - CMO (momentum direction)
  - ADX (trend strength)
- **Hurst divergence** (short vs long — emerging/fading trend)
- **Derivatives:** Funding, OI, L/S ratio, Taker B/S
- **Liquidation price**

**Khi nào dùng:** Sau khi `best_trade` tìm được coin tiềm năng → chạy `best_entry` để xác định entry point.

---

### 4.3 `momentum_scan` — Momentum scanner

**Mục đích:** Tìm coins có momentum mạnh nhất cho cả 2 phía Long và Short.

```bash
BINANCE_MARKET_TYPE=futures target/release/examples/momentum_scan
```

**Output:**
- **TOP LONG:** Coins có price↑ + volume surge + CMF dương
  - Price momentum score, Volume × average, Bull Vol 1H/4H
- **TOP SHORT:** Coins có distribution + CMF âm + dumping
  - Distribution score, Bear volume dominance
- **Tóm tắt khuyến nghị** cho cả 2 phía

**Khi nào dùng:** Khi cần tìm cơ hội directional trade nhanh.

---

### 4.4 `quick_scan` — Python quick scanner

**Mục đích:** Quét nhanh 40 coins phổ biến qua Python script.

```bash
cd ~/BonBoExtend && python3 scripts/quick_scan.py
```

**Output:**
- Sentiment + Fear & Greed overview
- Bảng xếp hạng 40 symbols theo score
- TOP 3 recommendation chi tiết

---

### 4.5 Các công cụ khác

```bash
# Phân tích BTC chi tiết
BINANCE_MARKET_TYPE=futures target/release/examples/btc_analysis

# Quét thị trường nâng cao
BINANCE_MARKET_TYPE=futures target/release/examples/advanced_scan

# Phân tích Gold (XAU)
BINANCE_MARKET_TYPE=futures target/release/examples/xau_full_analysis

# Market scan tổng quan
BINANCE_MARKET_TYPE=futures target/release/examples/market_scan

# Entry analysis
BINANCE_MARKET_TYPE=futures target/release/examples/entry_analysis

# Risk management demo
target/release/examples/risk_management

# Technical indicators demo
target/release/examples/ta_indicators

# 5-step analysis
BINANCE_MARKET_TYPE=futures target/release/examples/analysis_5steps
```

---

## 5. TRADING AGENTS INTEGRATION (v0.3.0 MỚI)

### 5.1 Kiến trúc tổng quan

```
┌──────────────────────────────────────────────────────────────┐
│                    BONBOEXTEND v0.3.0                        │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ 🔴 RUST HARD GUARDS (AUTHORITATIVE)                  │   │
│  │ • Max position size    • Max drawdown circuit breaker │   │
│  │ • Volatility spike     • Correlation limits           │   │
│  │ • Time-of-day rules    • Max leverage                 │   │
│  │ • Min liquidity        • Max daily loss               │   │
│  └──────────────────┬───────────────────────────────────┘   │
│                     │ wraps ALL outputs                      │
│  ┌──────────────────▼───────────────────────────────────┐   │
│  │ 🟡 MULTI-AGENT DEBATE ENGINE                         │   │
│  │                                                       │   │
│  │ Research Debate: Bull Researcher ↔ Bear Researcher   │   │
│  │       ↓ (n rounds, early termination on consensus)   │   │
│  │ Research Manager → synthesis                          │   │
│  │       ↓                                               │   │
│  │ Risk Debate: Aggressive ↔ Neutral ↔ Conservative     │   │
│  │       ↓ (conservative veto if high confidence HOLD)   │   │
│  │ Portfolio Manager → final decision                    │   │
│  └──────────────────┬───────────────────────────────────┘   │
│                     │                                        │
│  ┌──────────────────▼───────────────────────────────────┐   │
│  │ 🟢 CRYPTO-SPECIFIC DEBATORS                          │   │
│  │ • OnChainDebator: network health, active addresses    │   │
│  │ • WhaleDebator: exchange flows, whale tracking        │   │
│  │ • FundingDebator: funding rate, L/S ratio contrarian  │   │
│  └──────────────────┬───────────────────────────────────┘   │
│                     │                                        │
│  ┌──────────────────▼───────────────────────────────────┐   │
│  │ 🔵 DECISION JOURNAL + SELF-REFLECTION                │   │
│  │ • SQLite structured journal (replace flat markdown)   │   │
│  │ • TradeReflector: auto-generate lessons from outcomes │   │
│  │ • PatternMatcher: 6 known trade patterns              │   │
│  │ • Episodic Memory: keyword-based similar retrieval    │   │
│  └──────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### 5.2 Sử dụng Debate Engine

```rust
use bonbo_debate::{DebateEngine, DebateConfig, RuleBasedDebator};
use bonbo_debate::crypto::{OnChainDebator, WhaleDebator, FundingDebator};

// Cấu hình debate
let config = DebateConfig::default();   // 3 rounds research, 2 rounds risk
let fast_config = DebateConfig::fast(); // 1 round each — cho decisions nhanh

// Tạo engine với rule-based debators (không cần LLM)
let engine = DebateEngine::rule_based(config);

// Chạy research debate (Bull vs Bear)
let research = engine.run_research_debate(
    "BTCUSDT",
    "RSI oversold at 28, MACD bullish crossover, volume surge 3x, Hurst 0.67 trending",
    &[
        "Technical analyst: BUY signal — oversold bounce".to_string(),
        "Sentiment analyst: NEUTRAL — F&G = 45".to_string(),
    ],
).await?;

println!("Consensus: {:?}", research.consensus_direction); // Buy/Sell/Hold
println!("Strength: {}", research.consensus_strength);       // 0.0-1.0
println!("Bull args: {} | Bear args: {}", 
    research.bull_arguments.len(), 
    research.bear_arguments.len());

// Chạy risk debate (Aggressive vs Neutral vs Conservative)
let risk = engine.run_risk_debate(
    "BTCUSDT",
    &research,
    "Funding -0.02%, OI rising, L/S = 0.6",
).await?;

println!("Risk level: {:?}", risk.risk_consensus.risk_level);
println!("Direction: {:?}", risk.risk_consensus.direction);
println!("Position size multiplier: {}", risk.position_size_multiplier); // 0.0-0.25

// Conservative veto check
if risk.risk_factors.contains(&"Conservative veto".to_string()) {
    println!("⚠️ Conservative debator vetoed this trade!");
}
```

### 5.3 Crypto-specific Analysis

```rust
use bonbo_debate::crypto::CryptoDebateContext;

let ctx = CryptoDebateContext {
    ticker: "BTCUSDT".to_string(),
    btc_dominance: 52.0,
    funding_rate: -0.02,           // Negative = shorts paying longs (bullish)
    oi_change_24h: 8.0,            // Rising OI
    long_short_ratio: 0.4,         // More shorts → contrarian bullish
    taker_buy_sell_ratio: 1.5,     // Buying dominance
    top_trader_ls_ratio: 1.2,
    liquidation_24h_usd: 500_000_000.0,
    mcap_rank: Some(1),
    defi_tvl: None,
    active_addresses_24h: Some(1_200_000),
    whale_tx_count_24h: Some(250),
    network_vol_change_pct: Some(15.0),
    exchange_netflow: Some(-5000.0), // Outflow = accumulation (bullish)
    market_summary: "BTC trending up with strong buying".to_string(),
    fear_greed_index: 45,
};

// Derivatives bias: Bullish/Bearish/Neutral
let bias = ctx.derivatives_bias();
println!("Derivatives bias: {:?}", bias);

// On-chain health score: 0-100
let health = ctx.onchain_health();
println!("On-chain health: {} ({})", health.score, health.assessment);
```

### 5.4 Decision Journal

```rust
use bonbo_decision_journal::{DecisionJournal, JournalConfig, JournalQueries};

// Mở journal
let journal = DecisionJournal::open(&JournalConfig::default())?;

// Store a trade decision
let result = journal.store_decision(&trade_decision)?;
println!("Stored: {} at {}", result.decision_id, result.stored_at);

// Store a signal
journal.store_signal(&trading_signal)?;

// Update outcome after trade completes
journal.update_outcome("dec_001", &trade_outcome)?;

// Add self-reflection
journal.add_reflection("dec_001", &trade_reflection)?;

// Query pending decisions
let queries = JournalQueries::new(&journal);
let pending = queries.get_pending_decisions("BTCUSDT")?;
let count = queries.count_decisions(Some("BTCUSDT"))?;
let win_rate = queries.win_rate("BTCUSDT")?;
println!("Win rate: {:.1}% ({}/{})", 
    win_rate.win_rate * 100.0, win_rate.winning_trades, win_rate.total_trades);
```

### 5.5 Self-Reflection

```rust
use bonbo_debate::reflection::{TradeReflector, ReflectionInput};

let reflector = TradeReflector::default_matcher();

// Generate reflection from completed trade
let output = reflector.reflect(&ReflectionInput {
    decision: trade_decision,
    outcome: trade_outcome,
});

println!("✅ What went well: {:?}", output.what_went_well);
println!("🔧 Improvements: {:?}", output.improvements);
println!("📝 Lesson: {}", output.lesson);
println!("🔄 Would repeat: {}", output.would_repeat);
println!("📊 Confidence: {}", output.confidence);
println!("📌 Similar patterns: {:?}", output.analogous_situations);

// Convert to TradeReflection for journal storage
let reflection = TradeReflector::to_trade_reflection(&output);
journal.add_reflection("dec_001", &reflection)?;
```

### 5.6 Episodic Memory

```rust
use bonbo_memory::{MemoryStore, MemoryRetrieval, MemoryConfig, Episode};

// Open memory store
let store = MemoryStore::open(MemoryConfig::default())?;

// Store a trade episode
let episode = Episode {
    id: "ep_001".to_string(),
    ticker: "BTCUSDT".to_string(),
    regime: "Trending Up".to_string(),
    strategy: "ALMA Crossover".to_string(),
    direction: "LONG".to_string(),
    context_description: "Oversold bounce from support with volume surge".to_string(),
    tags: vec!["oversold".to_string(), "support".to_string()],
    // ... fields ...
};
store.store(&episode)?;

// Find similar past situations
let retrieval = MemoryRetrieval::new(&store, 0.3); // min similarity 0.3
let similar = retrieval.find_similar("BTCUSDT oversold trending up", 5)?;
for scored in &similar {
    println!("{} | similarity: {:.2} | outcome: {} | lesson: {}",
        scored.episode.ticker,
        scored.similarity,
        if scored.episode.outcome.is_win { "WIN" } else { "LOSS" },
        scored.episode.lesson,
    );
}

// Check regime+strategy win rate
let stats = retrieval.regime_strategy_win_rate("Trending Up", "ALMA Crossover")?;
println!("📊 Regime: {} | Strategy: {}", stats.regime, stats.strategy);
println!("   Win rate: {:.1}% ({}/{})", 
    stats.win_rate * 100.0, stats.wins, stats.total_trades);
println!("   Avg PnL: {:.2}%", stats.avg_pnl_pct);

// Find by ticker
let btc_trades = retrieval.find_by_ticker("BTCUSDT", 10)?;
println!("Found {} past BTC trades", btc_trades.len());
```

---

## 6. CẤU TRÚC DỰ ÁN

```
~/BonBoExtend/
├── Cargo.toml                        # Workspace root
├── .env                              # API keys configuration
├── Makefile                          # Build/test/lint shortcuts
├── docs/
│   ├── USAGE_GUIDE.md               # ← File này
│   ├── PROJECT_README.md            # Project overview
│   └── activity.md                  # Activity log
├── tasks/
│   └── integration_todo.md          # Task tracking
│
├── 🔧 Core Crates:
│   ├── bonbo-ta/                    # Technical indicators (10: RSI, MACD, EMA, ALMA, BB, ATR, ADX, CMO, LaguerreRSI, SuperSmoother)
│   ├── bonbo-regime/                # Regime detection (BOCPD + Hurst exponent)
│   ├── bonbo-risk/                  # Risk management (Kelly, CVaR, VaR, circuit breaker)
│   ├── bonbo-data/                  # Market data (Binance REST + WebSocket + SQLite cache)
│   ├── bonbo-quant/                 # Event-driven backtesting engine
│   ├── bonbo-sentinel/              # Sentiment (Fear & Greed, whale alerts, composite)
│   ├── bonbo-validation/            # Strategy validation framework
│   ├── bonbo-extend/                # Scanner, strategies, examples
│   ├── bonbo-extend-mcp/            # MCP server (35+ tools, HTTP + stdio)
│   ├── bonbo-agent/                 # Live trading agent + Telegram alerts
│   ├── bonbo-executor/              # Order execution (TWAP, VWAP, POV, smart)
│   ├── bonbo-binance-futures/       # Binance futures client
│   ├── bonbo-journal/               # Basic trade journal
│   ├── bonbo-learning/              # Ensemble self-learning
│   ├── bonbo-portfolio/             # Portfolio management
│   ├── bonbo-position-manager/      # Position sizing & management
│   ├── bonbo-extend-core/           # Core abstractions
│   │
│   ├── 🆕 TradingAgents Integration (v0.3.0):
│   ├── bonbo-llm-types/             # Shared LLM agent types (serde + JSON)
│   ├── bonbo-decision-journal/      # Structured SQLite decision journal
│   ├── bonbo-debate/                # Multi-agent adversarial debate engine
│   │   ├── src/config.rs            # DebateConfig (rounds, threshold, veto)
│   │   ├── src/engine.rs            # DebateEngine (research + risk debate)
│   │   ├── src/debators.rs          # RuleBasedDebator (keyword analysis)
│   │   ├── src/crypto/              # Crypto-specific debators
│   │   │   ├── crypto_debate_context.rs  # CryptoDebateContext
│   │   │   ├── on_chain_debator.rs       # Network metrics
│   │   │   ├── whale_debator.rs          # Exchange flow + whale tracking
│   │   │   └── funding_debator.rs        # Funding rate + L/S contrarian
│   │   └── src/reflection/          # Self-reflection service
│   │       ├── reflector.rs         # TradeReflector
│   │       └── patterns.rs          # PatternMatcher (6 patterns)
│   └── bonbo-memory/                # Episodic memory + retrieval
│       ├── src/episode.rs           # Episode type + keyword extraction
│       ├── src/store.rs             # SQLite MemoryStore
│       └── src/retrieval.rs         # Similarity search + stats
│
├── 📜 Scripts:
│   └── scripts/
│       ├── quick_scan.py            # Quick scanner (Python)
│       └── ...
│
└── 📊 Examples:
    └── target/release/examples/
        ├── best_trade               # Full market scanner (87 coins)
        ├── best_entry               # Single coin deep analysis
        ├── momentum_scan            # Momentum long/short scanner
        ├── advanced_scan            # Advanced multi-strategy
        ├── btc_analysis             # BTC full analysis
        ├── market_scan              # Market overview
        ├── xau_full_analysis        # Gold (XAU) analysis
        ├── entry_analysis           # Entry point analysis
        ├── analysis_5steps          # 5-step analysis
        ├── risk_management          # Risk management demo
        └── ta_indicators            # Technical indicators demo
```

---

## 7. CẤU HÌNH

### 7.1 File `.env`

Tạo file `~/BonBoExtend/.env`:

```env
# === Binance API ===
BINANCE_API_KEY=your_api_key_here
BINANCE_SECRET_KEY=your_secret_key_here
BINANCE_MARKET_TYPE=futures

# === MCP Server ===
MCP_TRANSPORT=stdio

# === Decision Journal ===
JOURNAL_DB_PATH=~/.bonbo/journal/decisions.db

# === Episodic Memory ===
MEMORY_DB_PATH=~/.bonbo/memory/episodes.db

# === Telegram (optional, for bonbo-agent alerts) ===
TELEGRAM_BOT_TOKEN=your_bot_token
TELEGRAM_CHAT_ID=your_chat_id

# === LLM Integration (future) ===
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
```

### 7.2 DebateConfig Options

```rust
// Default: balanced
DebateConfig::default()
// max_research_rounds: 3, max_risk_rounds: 2, consensus_threshold: 0.8

// Research: more thorough
DebateConfig::research()
// max_research_rounds: 5, max_risk_rounds: 3, consensus_threshold: 0.9

// Fast: for time-sensitive decisions
DebateConfig::fast()
// max_research_rounds: 1, max_risk_rounds: 1, consensus_threshold: 0.7
```

---

## 8. WORKFLOW GIAO DỊCH ĐỀ XUẤT

### 8.1 Daily Trading Workflow

```
Bước 1: Market Overview
├── best_trade → Quét 87 coins, xem TOP 5
├── Kiểm tra Sentiment (Fear & Greed)
└── Xác định regime thị trường

Bước 2: Deep Analysis
├── best_entry <TOP_PICK_1> → Entry/SL/TP chi tiết
├── best_entry <TOP_PICK_2> → So sánh
└── best_entry <TOP_PICK_3> → So sánh

Bước 3: Confirmation
├── momentum_scan → Kiểm tra momentum Long/Short
└── Kiểm tra derivatives (funding, OI, L/S)

Bước 4: Risk Management
├── Kiểm tra position size (Kelly criterion)
├── Đảm bảo SL < max acceptable loss
└── Kiểm tra portfolio exposure tổng

Bước 5: Execute & Monitor
├── Entry tại giá đề xuất
├── Đặt SL/TP tự động
├── Monitor qua Telegram alerts (bonbo-agent)
└── Ghi journal (bonbo-decision-journal)

Bước 6: Post-Trade Reflection
├── Update outcome vào journal
├── TradeReflector → tự động generate lessons
├── PatternMatcher → identify recurring patterns
└── Episodic Memory → store for future retrieval
```

### 8.2 Ví dụ thực tế

```bash
# Buổi sáng: Quét thị trường
BINANCE_MARKET_TYPE=futures target/release/examples/best_trade
# → Kết quả: CHZUSDT score=87, MEGAUSDT score=83, PENGUUSDT score=79

# Deep analysis top 3
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry CHZUSDT
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry MEGAUSDT
BINANCE_MARKET_TYPE=futures target/release/examples/best_entry PENGUUSDT

# Kiểm tra momentum
BINANCE_MARKET_TYPE=futures target/release/examples/momentum_scan
# → Confirm: PENGUUSDT có momentum tốt nhất cho LONG

# Quyết định: LONG PENGUUSDT
# Entry: $0.0097 | SL: $0.0083 (-14%) | TP1: $0.0117 (+20.6%)
```

---

## 9. TROUBLESHOOTING

### 9.1 Lỗi thường gặp

| Lỗi | Nguyên nhân | Giải pháp |
|-----|-------------|-----------|
| `Connection refused` | Không có internet / Binance block | Kiểm tra network, thử dùng VPN |
| `Invalid symbol` | Symbol không tồn tại trên Binance | Kiểm tra symbol name (VD: phải là BTCUSDT) |
| `API key invalid` | API key sai hoặc hết hạn | Cập nhật `.env` với API key mới |
| `No data` | Symbol quá mới hoặc ít volume | Chọn symbol phổ biến hơn |

### 9.2 Rebuild từ đầu

```bash
# Clean và rebuild
cargo clean
cargo build --workspace --release
cargo test --workspace
```

---

## 10. VERSION HISTORY

| Version | Date | Changes |
|---------|------|---------|
| v0.1.0 | 2026-04-18 | Initial release: 7 crates, 21 MCP tools, 120 tests |
| v0.2.0 | 2026-04-25 | Refactoring: 17 crates, 567 tests, 0 clippy warnings |
| v0.3.0 | 2026-05-03 | TradingAgents integration: 22 crates, 760 tests |
| | | - P0: bonbo-llm-types, bonbo-decision-journal, hard guards |
| | | - P1: bonbo-debate engine, Debator trait, risk debate |
| | | - P2: Crypto debators, self-reflection, pattern matching |
| | | - P3: Episodic memory, keyword-based retrieval |

---

⚠️ **Disclaimer:** Đây là công cụ phân tích định lượng, KHÔNG phải lời khuyên tài chính. Luôn quản lý rủi ro cẩn thận và chỉ giao dịch với số tiền bạn có thể afford để mất.
