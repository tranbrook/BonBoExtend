//! BonBo Full Workflow Demo — 9-step trading agents workflow
//!
//! Chạy:
//!   Rule-based (mặc định): cargo run --release -p bonbo-workflow-demo -- BTCUSDT
//!   LLM (GPT-4o):          cargo run --release -p bonbo-workflow-demo -- --llm BTCUSDT
//!   LLM (GPT-4o-mini):     cargo run --release -p bonbo-workflow-demo -- --llm-mini BTCUSDT

use anyhow::Result;
use bonbo_debate::reflection::{ReflectionInput, TradeReflector};
use bonbo_debate::{DebateConfig, DebateEngine, LlmConfig, LlmDebateEngine};
use bonbo_decision_journal::{DecisionJournal, JournalConfig};
use bonbo_llm_types::journal::{
    ExitReason, MarketSnapshot, TradeDecision, TradeOutcome, TradeReflection,
};
use bonbo_llm_types::risk::{GuardCheck, GuardType, HardGuardResult};
use bonbo_llm_types::signal::{AgentVote, TakeProfitLevel};
use bonbo_llm_types::{Confidence, Rating, RiskLevel, SignalSource, TradeDirection};
use bonbo_memory::episode::{Episode, EpisodeOutcome};
use bonbo_memory::retrieval::MemoryRetrieval;
use bonbo_memory::{MemoryConfig, MemoryStore};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::env;

// ═══════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════

fn separator(title: &str) {
    println!("\n{}", "═".repeat(70));
    let pad = 70usize.saturating_sub(4 + title.len());
    println!("  {} {}", title, "═".repeat(pad));
    println!("{}", "═".repeat(70));
}

fn sub_header(title: &str) {
    let pad = 60usize.saturating_sub(title.len());
    println!("\n┌─ {} {}", title, "─".repeat(pad));
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn rsi_status(rsi: f64) -> &'static str {
    if rsi < 30.0 {
        "🟢 OVERSOLD"
    } else if rsi < 40.0 {
        "🟡 Low"
    } else if rsi < 60.0 {
        "⚪ Neutral"
    } else if rsi < 70.0 {
        "🟡 High"
    } else {
        "🔴 OVERBOUGHT"
    }
}

fn hurst_status(h: f64) -> &'static str {
    if h > 0.6 {
        "📈 Trending"
    } else if h > 0.45 {
        "➡️  Moderate"
    } else {
        "🔄 Mean-reverting"
    }
}

fn fg_status(fg: u32) -> &'static str {
    if fg < 25 {
        "😱 Extreme Fear"
    } else if fg < 45 {
        "😰 Fear"
    } else if fg < 55 {
        "😐 Neutral"
    } else if fg < 75 {
        "🤑 Greed"
    } else {
        "🤯 Extreme Greed"
    }
}

fn funding_status(f: f64) -> &'static str {
    if f < -0.01 {
        "🟢 Shorts paying → Bullish"
    } else if f < 0.0 {
        "🟡 Mild bullish"
    } else if f < 0.01 {
        "⚪ Neutral"
    } else if f < 0.03 {
        "🟡 Mild bearish"
    } else {
        "🔴 Overleveraged → Bearish"
    }
}

// ═══════════════════════════════════════════════════════════════════
// MARKET DATA SIMULATION
// ═══════════════════════════════════════════════════════════════════

struct MarketData {
    price: f64,
    rsi_4h: f64,
    rsi_1d: f64,
    hurst_4h: f64,
    fear_greed: u32,
    funding_rate: f64,
    volume_24h: f64,
    regime: String,
}

fn simulate_market_analysis(ticker: &str) -> MarketData {
    match ticker {
        "BTCUSDT" => MarketData {
            price: 94_250.0,
            rsi_4h: 38.5,
            rsi_1d: 45.2,
            hurst_4h: 0.62,
            fear_greed: 42,
            funding_rate: -0.015,
            volume_24h: 32_000_000_000.0,
            regime: "Trending Up".to_string(),
        },
        "ETHUSDT" => MarketData {
            price: 3_450.0,
            rsi_4h: 32.8,
            rsi_1d: 40.5,
            hurst_4h: 0.58,
            fear_greed: 42,
            funding_rate: -0.008,
            volume_24h: 18_000_000_000.0,
            regime: "Trending Up".to_string(),
        },
        "SOLUSDT" => MarketData {
            price: 172.50,
            rsi_4h: 55.0,
            rsi_1d: 52.3,
            hurst_4h: 0.48,
            fear_greed: 55,
            funding_rate: 0.012,
            volume_24h: 4_500_000_000.0,
            regime: "Ranging".to_string(),
        },
        _ => MarketData {
            price: 0.0847,
            rsi_4h: 35.2,
            rsi_1d: 42.0,
            hurst_4h: 0.67,
            fear_greed: 45,
            funding_rate: -0.02,
            volume_24h: 150_000_000.0,
            regime: "Trending Up".to_string(),
        },
    }
}

// ═══════════════════════════════════════════════════════════════════
// HARD GUARDS SIMULATION
// ═══════════════════════════════════════════════════════════════════

fn simulate_hard_guards(ticker: &str) -> HardGuardResult {
    HardGuardResult {
        ticker: ticker.to_string(),
        all_passed: true,
        checks: vec![
            GuardCheck {
                guard_name: GuardType::MaxPositionSize,
                passed: true,
                current_value: "8.2%".to_string(),
                limit_value: "25%".to_string(),
                message: "Position within limits".to_string(),
            },
            GuardCheck {
                guard_name: GuardType::MaxDrawdown,
                passed: true,
                current_value: "portfolio OK".to_string(),
                limit_value: "max 15%".to_string(),
                message: "No drawdown breach".to_string(),
            },
            GuardCheck {
                guard_name: GuardType::VolatilitySpike,
                passed: true,
                current_value: "ATR normal".to_string(),
                limit_value: "2.5x average".to_string(),
                message: "No volatility spike detected".to_string(),
            },
            GuardCheck {
                guard_name: GuardType::MaxLeverage,
                passed: true,
                current_value: "5x".to_string(),
                limit_value: "10x".to_string(),
                message: "Leverage within limits".to_string(),
            },
            GuardCheck {
                guard_name: GuardType::MinLiquidity,
                passed: true,
                current_value: "$150M".to_string(),
                limit_value: "$10M".to_string(),
                message: "Sufficient liquidity".to_string(),
            },
            GuardCheck {
                guard_name: GuardType::MaxDailyLoss,
                passed: true,
                current_value: "-1.2%".to_string(),
                limit_value: "-5%".to_string(),
                message: "Daily loss within limits".to_string(),
            },
        ],
        risk_level: RiskLevel::Low,
        max_position_size: dec!(0.082),
        block_reason: None,
        timestamp: Utc::now(),
    }
}

// ═══════════════════════════════════════════════════════════════════
// SEED HISTORICAL EPISODES
// ═══════════════════════════════════════════════════════════════════

fn seed_historical_episodes(store: &MemoryStore) -> Result<()> {
    let episodes = vec![
        Episode {
            id: "ep_hist_001".to_string(),
            ticker: "BTCUSDT".to_string(),
            regime: "Trending Up".to_string(),
            strategy: "ALMA Crossover".to_string(),
            direction: "LONG".to_string(),
            context_description: "Oversold bounce from support with volume surge".to_string(),
            tags: vec!["oversold".to_string(), "trending".to_string(), "volume_surge".to_string()],
            entry_price: "88000".to_string(),
            stop_loss: "84500".to_string(),
            outcome: EpisodeOutcome { is_win: true, pnl_pct: 8.5, exit_reason: "take_profit_1".to_string(), mfe_pct: 12.0, mae_pct: 2.5 },
            lesson: "Oversold bounces work well in trending regime when volume surges".to_string(),
            entry_confidence: 0.75,
            rsi_4h: Some(32.0), hurst_4h: Some(0.63), fear_greed: Some(38),
            pnl_pct: 8.5, holding_hours: 24.0, debate_rounds: 3,
            created_at: Utc::now() - chrono::Duration::days(7),
        },
        Episode {
            id: "ep_hist_002".to_string(),
            ticker: "ETHUSDT".to_string(),
            regime: "Trending Up".to_string(),
            strategy: "ALMA Crossover".to_string(),
            direction: "LONG".to_string(),
            context_description: "Trend continuation after pullback to EMA support".to_string(),
            tags: vec!["trending".to_string(), "ema_support".to_string(), "pullback".to_string()],
            entry_price: "3200".to_string(),
            stop_loss: "3050".to_string(),
            outcome: EpisodeOutcome { is_win: true, pnl_pct: 12.3, exit_reason: "take_profit_2".to_string(), mfe_pct: 15.0, mae_pct: 1.8 },
            lesson: "EMA support pullbacks in trending regime are reliable entries".to_string(),
            entry_confidence: 0.82,
            rsi_4h: Some(45.0), hurst_4h: Some(0.65), fear_greed: Some(48),
            pnl_pct: 12.3, holding_hours: 48.0, debate_rounds: 4,
            created_at: Utc::now() - chrono::Duration::days(3),
        },
        Episode {
            id: "ep_hist_003".to_string(),
            ticker: "BTCUSDT".to_string(),
            regime: "Ranging".to_string(),
            strategy: "ALMA Crossover".to_string(),
            direction: "LONG".to_string(),
            context_description: "False breakout in ranging market — trend strategy failed".to_string(),
            tags: vec!["ranging".to_string(), "false_breakout".to_string(), "loss".to_string()],
            entry_price: "92000".to_string(),
            stop_loss: "88500".to_string(),
            outcome: EpisodeOutcome { is_win: false, pnl_pct: -3.8, exit_reason: "stop_loss".to_string(), mfe_pct: 0.5, mae_pct: 4.2 },
            lesson: "ALMA Crossover fails in ranging regime — use BB strategy instead".to_string(),
            entry_confidence: 0.55,
            rsi_4h: Some(52.0), hurst_4h: Some(0.45), fear_greed: Some(55),
            pnl_pct: -3.8, holding_hours: 12.0, debate_rounds: 2,
            created_at: Utc::now() - chrono::Duration::days(1),
        },
        Episode {
            id: "ep_hist_004".to_string(),
            ticker: "SOLUSDT".to_string(),
            regime: "Trending Up".to_string(),
            strategy: "ALMA Crossover".to_string(),
            direction: "LONG".to_string(),
            context_description: "Strong trend with ALMA cross and volume confirmation".to_string(),
            tags: vec!["trending".to_string(), "volume".to_string(), "strong_trend".to_string()],
            entry_price: "155.0".to_string(),
            stop_loss: "148.0".to_string(),
            outcome: EpisodeOutcome { is_win: true, pnl_pct: 15.2, exit_reason: "take_profit_3".to_string(), mfe_pct: 18.0, mae_pct: 1.2 },
            lesson: "Strong trends with volume confirmation yield best results".to_string(),
            entry_confidence: 0.88,
            rsi_4h: Some(55.0), hurst_4h: Some(0.72), fear_greed: Some(60),
            pnl_pct: 15.2, holding_hours: 72.0, debate_rounds: 5,
            created_at: Utc::now() - chrono::Duration::days(14),
        },
        Episode {
            id: "ep_hist_005".to_string(),
            ticker: "ETHUSDT".to_string(),
            regime: "Ranging".to_string(),
            strategy: "BB Bounce".to_string(),
            direction: "SHORT".to_string(),
            context_description: "BB upper band rejection in ranging market".to_string(),
            tags: vec!["ranging".to_string(), "bb_rejection".to_string(), "mean_reversion".to_string()],
            entry_price: "3600".to_string(),
            stop_loss: "3680".to_string(),
            outcome: EpisodeOutcome { is_win: true, pnl_pct: 6.5, exit_reason: "take_profit_1".to_string(), mfe_pct: 8.0, mae_pct: 1.0 },
            lesson: "BB bounces work in ranging regime — switch strategy based on regime".to_string(),
            entry_confidence: 0.70,
            rsi_4h: Some(68.0), hurst_4h: Some(0.42), fear_greed: Some(52),
            pnl_pct: 6.5, holding_hours: 18.0, debate_rounds: 2,
            created_at: Utc::now() - chrono::Duration::days(5),
        },
    ];

    for ep in &episodes {
        store.store(ep)?;
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// MAIN WORKFLOW
// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file (ignore error if not found)
    let _ = dotenv::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter("bonbo_workflow_demo=info,bonbo_debate=info")
        .init();

    // Parse arguments: [--llm | --llm-mini | --glm5] [SYMBOL]
    let args: Vec<String> = env::args().collect();
    let use_llm = args.iter().any(|a| a == "--llm" || a == "--llm-mini" || a == "--glm5");
    let use_llm_mini = args.iter().any(|a| a == "--llm-mini");
    let use_glm5 = args.iter().any(|a| a == "--glm5");
    let ticker = args.iter()
        .find(|a| !a.starts_with('-') && *a != &args[0])
        .cloned()
        .unwrap_or_else(|| "BTCUSDT".to_string());

    let engine_mode = if use_glm5 {
        "GLM-5 (Z.ai)"
    } else if use_llm {
        if use_llm_mini { "GPT-4o-mini" } else { "GPT-4o" }
    } else {
        "Rule-Based"
    };

    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  BONBOEXTEND v0.3.0 — FULL TRADING AGENTS WORKFLOW          ║");
    println!("║  {:52}║", format!("Ticker: {}", ticker));
    println!("║  {:52}║", format!("Engine: {}", engine_mode));
    println!("║  {:52}║", Utc::now().format("%Y-%m-%d %H:%M UTC").to_string());
    println!("╚══════════════════════════════════════════════════════════════╝");

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 1: Phân tích thị trường
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 1: PHÂN TÍCH THỊ TRƯỜNG (Simulated best_entry)");

    let md = simulate_market_analysis(&ticker);

    sub_header("Market Summary");
    println!("│ 📈 Ticker:    {}", ticker);
    println!("│ 💰 Price:     ${:.2}", md.price);
    println!("│ 📊 RSI 4H:    {:.1}  {}", md.rsi_4h, rsi_status(md.rsi_4h));
    println!("│ 📊 RSI 1D:    {:.1}  {}", md.rsi_1d, rsi_status(md.rsi_1d));
    println!("│ 📈 Hurst 4H:  {:.3}  {}", md.hurst_4h, hurst_status(md.hurst_4h));
    println!("│ 😱 F&G Index: {}  {}", md.fear_greed, fg_status(md.fear_greed));
    println!("│ 💵 Funding:   {:.4}%  {}", md.funding_rate, funding_status(md.funding_rate));
    println!("│ 📊 Volume:    ${:.1}B", md.volume_24h / 1e9);
    println!("│ 🏷️  Regime:   {}", md.regime);
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 2: Kiểm tra Episodic Memory
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 2: KIỂM TRA EPISODIC MEMORY");

    let memory_config = MemoryConfig {
        db_path: format!("/tmp/bonbo_workflow_memory_{}.db", Utc::now().timestamp_millis()),
        max_episodes: 10_000,
        min_similarity: 0.2,
    };
    let store = MemoryStore::open(memory_config)?;
    seed_historical_episodes(&store)?;

    let retrieval = MemoryRetrieval::new(&store, 0.2);
    let query = format!(
        "{} {} oversold trending volume",
        ticker.to_lowercase(),
        md.regime.to_lowercase()
    );

    sub_header("Tìm trade tương tự trong quá khứ");
    println!("│ Query: \"{}\"", query);

    match retrieval.find_similar(&query, 5) {
        Ok(similar) => {
            if similar.is_empty() {
                println!("│ ℹ️  Không tìm thấy trade tương tự");
            } else {
                println!("│");
                for (i, scored) in similar.iter().enumerate() {
                    println!(
                        "│ {}. {} │ sim: {:.0}% │ {} │ PnL: {:+.1}% │ {}",
                        i + 1,
                        scored.episode.ticker,
                        scored.similarity * 100.0,
                        if scored.episode.outcome.is_win { "✅ WIN " } else { "❌ LOSS" },
                        scored.episode.outcome.pnl_pct,
                        scored.episode.regime,
                    );
                    println!("│    Lesson: {}", scored.episode.lesson);
                }
            }
        }
        Err(e) => println!("│ ⚠️  Lỗi retrieval: {}", e),
    }

    // Win rate regime + strategy
    sub_header("Regime + Strategy Win Rate");
    let regime_str = if md.hurst_4h > 0.55 { "Trending Up" } else { "Ranging" };
    match retrieval.regime_strategy_win_rate(regime_str, "ALMA Crossover") {
        Ok(stats) => {
            println!("│ Regime:   {}", stats.regime);
            println!("│ Strategy: {}", stats.strategy);
            println!("│ Win rate: {:.1}% ({}/{})", stats.win_rate * 100.0, stats.wins, stats.total_trades);
            println!("│ Avg PnL:  {:+.2}%", stats.avg_pnl_pct);
            if stats.total_trades == 0 {
                println!("│ ℹ️  Chưa có dữ liệu — default win rate ~55%");
            }
        }
        Err(e) => println!("│ ⚠️  Lỗi: {}", e),
    }
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 3: Debate Engine — Research Debate
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 3: RESEARCH DEBATE (Bull vs Bear)");

    let market_summary = format!(
        "RSI 4H: {:.1} {}, RSI 1D: {:.1}, Hurst: {:.3}, Funding: {:.4}%, F&G: {}, Vol: ${:.0}B, Regime: {}",
        md.rsi_4h, rsi_status(md.rsi_4h), md.rsi_1d, md.hurst_4h,
        md.funding_rate, md.fear_greed, md.volume_24h / 1e9, md.regime,
    );

    let analyst_reports = vec![
        format!("Technical: {} — RSI 4H {:.1}, Hurst {:.3}",
            if md.rsi_4h < 40.0 { "BUY signal — oversold" }
            else if md.rsi_4h > 60.0 { "SELL signal — overbought" }
            else { "NEUTRAL" },
            md.rsi_4h, md.hurst_4h),
        format!("Sentiment: {} — F&G = {}",
            if md.fear_greed < 30 { "Extreme Fear — contrarian bullish" }
            else if md.fear_greed > 70 { "Extreme Greed — contrarian bearish" }
            else { "NEUTRAL" },
            md.fear_greed),
        format!("Derivatives: {} — Funding {:.4}%",
            if md.funding_rate < 0.0 { "BULLISH — shorts paying longs" }
            else if md.funding_rate > 0.03 { "BEARISH — overleveraged longs" }
            else { "NEUTRAL" },
            md.funding_rate),
    ];

    println!("│ Market Summary: {}", truncate(&market_summary, 60));
    for (i, report) in analyst_reports.iter().enumerate() {
        println!("│ Analyst {}: {}", i + 1, report);
    }
    println!("│");

    // Create engine — rule-based or LLM
    let config = DebateConfig::default();

    println!("│ 🔴 Bull Researcher vs 🔵 Bear Researcher");
    println!("│ Engine: {} | {} rounds, threshold {:.0}%",
        engine_mode, config.max_research_rounds, config.consensus_threshold * Decimal::from(100));

    // Run research debate
    let research = if use_llm {
        let llm_config = if use_glm5 {
            LlmConfig {
                api_key: std::env::var("ZAI_API_KEY").unwrap_or_default(),
                model: "glm-5".to_string(),
                base_url: "https://api.z.ai/api/paas/v4".to_string(),
                max_tokens: 1024,
                temperature: 0.3,
            }
        } else if use_llm_mini {
            LlmConfig::gpt4o_mini()
        } else {
            LlmConfig::gpt4o()
        };

        if llm_config.is_configured() {
            println!("│ 🧠 Using LLM: {} @ {}", llm_config.model, llm_config.base_url);
            let llm_engine = LlmDebateEngine::new(llm_config, config.clone());
            llm_engine.run_research_debate(&ticker, &market_summary, &analyst_reports).await?
        } else {
            let key_name = if use_glm5 { "ZAI_API_KEY" } else { "OPENAI_API_KEY" };
            println!("│ ⚠️  {} not set → falling back to rule-based", key_name);
            let rb_engine = DebateEngine::rule_based(config.clone());
            rb_engine.run_research_debate(&ticker, &market_summary, &analyst_reports).await?
        }
    } else {
        let engine = DebateEngine::rule_based(config.clone());
        engine.run_research_debate(&ticker, &market_summary, &analyst_reports).await?
    };

    sub_header("Kết quả Research Debate");
    println!("│ Consensus:  {:?}", research.consensus_direction);
    println!("│ Strength:   {:.2}", research.consensus_strength);
    println!("│ Rounds:     {}/{}", research.rounds_completed, config.max_research_rounds);
    println!("│");
    println!("│ 🔴 Bull arguments ({}):", research.bull_arguments.len());
    for arg in &research.bull_arguments {
        println!("│   Round {}: {} (conf: {:.2})",
            arg.round, truncate(&arg.argument, 65), arg.confidence);
    }
    println!("│ 🔵 Bear arguments ({}):", research.bear_arguments.len());
    for arg in &research.bear_arguments {
        println!("│   Round {}: {} (conf: {:.2})",
            arg.round, truncate(&arg.argument, 65), arg.confidence);
    }
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 4: Risk Debate
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 4: RISK DEBATE (Aggressive vs Neutral vs Conservative)");

    let risk_summary = format!(
        "Funding {:.4}%, OI +8.5%, L/S ratio 0.45, F&G = {}",
        md.funding_rate, md.fear_greed,
    );

    let risk = if use_llm {
        let llm_config = if use_glm5 {
            LlmConfig {
                api_key: std::env::var("ZAI_API_KEY").unwrap_or_default(),
                model: "glm-5".to_string(),
                base_url: "https://api.z.ai/api/paas/v4".to_string(),
                max_tokens: 1024,
                temperature: 0.3,
            }
        } else if use_llm_mini {
            LlmConfig::gpt4o_mini()
        } else {
            LlmConfig::gpt4o()
        };

        if llm_config.is_configured() {
            let llm_engine = LlmDebateEngine::new(llm_config, config.clone());
            llm_engine.run_risk_debate(&ticker, &research, &risk_summary).await?
        } else {
            let rb_engine = DebateEngine::rule_based(config.clone());
            rb_engine.run_risk_debate(&ticker, &research, &risk_summary).await?
        }
    } else {
        let engine = DebateEngine::rule_based(config.clone());
        engine.run_risk_debate(&ticker, &research, &risk_summary).await?
    };

    sub_header("Kết quả Risk Debate");
    println!("│ 🟢 Aggressive:  {:?}  | size: {:.0}% of Kelly",
        risk.aggressive_position.recommended_action,
        risk.aggressive_position.suggested_size_fraction * dec!(100));
    println!("│ 🟡 Neutral:     {:?}  | size: {:.0}% of Kelly",
        risk.neutral_position.recommended_action,
        risk.neutral_position.suggested_size_fraction * dec!(100));
    println!("│ 🔴 Conservative:{:?}  | size: {:.0}% of Kelly",
        risk.conservative_position.recommended_action,
        risk.conservative_position.suggested_size_fraction * dec!(100));
    println!("│");
    println!("│ 📊 Risk Consensus:");
    println!("│   Risk Level:  {:?}", risk.risk_consensus.risk_level);
    println!("│   Direction:   {:?}", risk.risk_consensus.direction);
    println!("│   Size Mult:   {:.2} ({:.0}% of Kelly)",
        risk.position_size_multiplier,
        risk.position_size_multiplier * dec!(100));

    if !risk.risk_factors.is_empty() {
        println!("│   ⚠️  Risk Factors:");
        for factor in &risk.risk_factors {
            println!("│     • {}", factor);
        }
    }
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 5: Hard Guards
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 5: HARD GUARDS (Rust Rule Engine)");

    let guard_result = simulate_hard_guards(&ticker);
    for check in &guard_result.checks {
        let status = if check.passed { "✅ PASS" } else { "❌ FAIL" };
        println!("│ {} {:?} — {} ({} / {})",
            status, check.guard_name, check.message, check.current_value, check.limit_value);
    }
    println!("│");
    if guard_result.all_passed {
        println!("│ ✅ TẤT CẢ GUARDS PASSED — OK để giao dịch");
    } else {
        println!("│ ❌ GUARD FAILED — KHÔNG giao dịch!");
        println!("│ Block reason: {:?}", guard_result.block_reason);
    }
    println!("│ Max position: {:.1}%", guard_result.max_position_size * dec!(100));
    println!("└{}", "─".repeat(60));

    if !guard_result.all_passed {
        println!("\n⛔ Workflow dừng — Hard Guards failed!");
        return Ok(());
    }

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 6: Decision Journal
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 6: DECISION JOURNAL — Lưu quyết định");

    let journal_config = JournalConfig {
        db_path: format!("/tmp/bonbo_workflow_journal_{}.db", Utc::now().timestamp_millis()),
        auto_migrate: true,
    };
    let journal = DecisionJournal::open(&journal_config)?;

    let decision_id = format!("dec_{}", Utc::now().format("%Y%m%d_%H%M%S"));
    let entry_price = Decimal::from_f64_retain(md.price).unwrap_or(dec!(94000));
    let sl_pct: f64 = 0.038;
    let sl_mult = Decimal::from_f64_retain(1.0 - sl_pct).unwrap_or(dec!(96));
    let stop_loss = entry_price * sl_mult;
    let tp1_mult = Decimal::from_f64_retain(1.0 + sl_pct * 2.3).unwrap_or(dec!(108));
    let tp1 = entry_price * tp1_mult;
    let tp2_mult = Decimal::from_f64_retain(1.0 + sl_pct * 4.0).unwrap_or(dec!(115));
    let tp2 = entry_price * tp2_mult;
    let tp3_mult = Decimal::from_f64_retain(1.0 + sl_pct * 6.0).unwrap_or(dec!(122));
    let tp3 = entry_price * tp3_mult;

    let action = risk.risk_consensus.direction.clone();
    let position_pct = risk.position_size_multiplier * dec!(100);

    let decision = TradeDecision {
        decision_id: decision_id.clone(),
        ticker: ticker.clone(),
        action: action.clone(),
        rating: Rating::Buy,
        confidence: research.consensus_strength,
        agent_votes: vec![
            AgentVote {
                source: SignalSource::BullResearcher,
                direction: TradeDirection::Buy,
                confidence: research.consensus_strength,
                weight: dec!(0.5),
                reason: "Bull arguments prevailed".to_string(),
            },
            AgentVote {
                source: SignalSource::BearResearcher,
                direction: TradeDirection::Sell,
                confidence: dec!(1) - research.consensus_strength,
                weight: dec!(0.5),
                reason: "Bear counter-arguments".to_string(),
            },
        ],
        reasoning: format!(
            "Research: {:?} (strength {:.2}). {} bull, {} bear args. Risk: {:?}, size {:.0}%.",
            research.consensus_direction, research.consensus_strength,
            research.bull_arguments.len(), research.bear_arguments.len(),
            risk.risk_consensus.risk_level, position_pct * dec!(100),
        ),
        market_context: MarketSnapshot {
            price: entry_price,
            volume_24h: Decimal::from_f64_retain(md.volume_24h).unwrap_or(dec!(0)),
            rsi_4h: Some(Decimal::from_f64_retain(md.rsi_4h).unwrap_or(dec!(50))),
            rsi_1d: Some(Decimal::from_f64_retain(md.rsi_1d).unwrap_or(dec!(50))),
            hurst_4h: Some(Decimal::from_f64_retain(md.hurst_4h).unwrap_or(dec!(5))),
            fear_greed: Some(md.fear_greed as u8),
            funding_rate: Some(Decimal::from_f64_retain(md.funding_rate / 100.0).unwrap_or(dec!(0))),
            regime: md.regime.clone(),
        },
        entry_price,
        stop_loss,
        take_profits: vec![
            TakeProfitLevel { price: tp1, size_fraction: dec!(0.5), risk_reward: dec!(2) + dec!(3) },
            TakeProfitLevel { price: tp2, size_fraction: dec!(0.3), risk_reward: dec!(4) },
            TakeProfitLevel { price: tp3, size_fraction: dec!(0.2), risk_reward: dec!(6) },
        ],
        position_size: position_pct * dec!(10),
        position_size_pct: position_pct,
        risk_reward_ratio: dec!(2) + dec!(3),
        guard_result: guard_result.clone(),
        risk_level: risk.risk_consensus.risk_level.clone(),
        debate_rounds: research.rounds_completed + risk.rounds_completed,
        regime: md.regime.clone(),
        strategy: "ALMA Crossover".to_string(),
        outcome: None,
        reflection: None,
        timestamp: Utc::now(),
    };

    let store_result = journal.store_decision(&decision)?;

    println!("│ ✅ Decision saved!");
    println!("│   ID:        {}", store_result.decision_id);
    println!("│   Ticker:    {}", ticker);
    println!("│   Action:    {:?}", action);
    println!("│   Entry:     ${:.2}", entry_price);
    println!("│   SL:        ${:.2} ({:.1}%)", stop_loss, sl_pct * 100.0);
    println!("│   TP1:       ${:.2} (+{:.1}%)", tp1, sl_pct * 2.3 * 100.0);
    println!("│   TP2:       ${:.2} (+{:.1}%)", tp2, sl_pct * 4.0 * 100.0);
    println!("│   TP3:       ${:.2} (+{:.1}%)", tp3, sl_pct * 6.0 * 100.0);
    println!("│   Size:      {:.1}% equity", position_pct);
    println!("│   R:R:       2.3:1");
    println!("│   Confidence:{:.2}", research.consensus_strength);
    println!("│   Debate:    {} rounds", decision.debate_rounds);
    println!("│   Saved at:  {}", store_result.stored_at.format("%Y-%m-%d %H:%M UTC"));
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 7: Execute (Simulated)
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 7: EXECUTE (Simulated)");

    let sl_amount = entry_price - stop_loss;
    println!("│ 📋 TRADE EXECUTION PLAN:");
    println!("│ ┌─────────────────────────────────────────────┐");
    println!("│ │ Symbol:    {}", ticker);
    println!("│ │ Direction: {:?}", action);
    println!("│ │ Entry:     ${:.2}", entry_price);
    println!("│ │ SL:        ${:.2} (-${:.2})", stop_loss, sl_amount);
    println!("│ │ TP1:       ${:.2} (+${:.2})", tp1, tp1 - entry_price);
    println!("│ │ TP2:       ${:.2} (+${:.2})", tp2, tp2 - entry_price);
    println!("│ │ TP3:       ${:.2} (+${:.2})", tp3, tp3 - entry_price);
    println!("│ │ Size:      {:.1}% equity (${:.0} trên $10K)", position_pct, position_pct * dec!(100));
    println!("│ └─────────────────────────────────────────────┘");
    println!("│");
    println!("│ ⏳ Simulating... 36 giờ sau → Hit TP1! 🎯");
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 8: Update Outcome
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 8: UPDATE OUTCOME");

    let pnl_pct_f64 = sl_pct * 2.3 * 100.0;
    let pnl_pct_dec = Decimal::from_f64_retain(pnl_pct_f64).unwrap_or(dec!(8));
    let pnl_abs = position_pct * dec!(100) * pnl_pct_dec / dec!(100);
    let mfe_pct = Decimal::from_f64_retain(pnl_pct_f64 * 1.4).unwrap_or(dec!(12));
    let mae_pct = Decimal::from_f64_retain(sl_pct * 100.0 * 0.5).unwrap_or(dec!(2));

    let outcome = TradeOutcome {
        exit_price: tp1,
        pnl_pct: pnl_pct_dec,
        pnl_absolute: pnl_abs,
        holding_hours: dec!(36),
        mfe_pct,
        mae_pct,
        exit_reason: ExitReason::TakeProfit1,
        exit_timestamp: Utc::now(),
    };

    journal.update_outcome(&decision_id, &outcome)?;

    println!("│ ✅ Outcome updated!");
    println!("│   Exit Price:  ${:.2}", tp1);
    println!("│   PnL:         +{:.2}% (${:.2})", pnl_pct_dec, pnl_abs);
    println!("│   Holding:     36 giờ");
    println!("│   MFE:         +{}", mfe_pct);
    println!("│   MAE:         -{}", mae_pct);
    println!("│   Exit Reason: TakeProfit1 ✅");
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // BƯỚC 9: Self-Reflection + Episodic Memory
    // ═══════════════════════════════════════════════════════════════
    separator("BƯỚC 9: SELF-REFLECTION + EPISODIC MEMORY");

    // 9a: Self-Reflection
    sub_header("Self-Reflection (TradeReflector)");
    let reflector = TradeReflector::default_matcher();
    let reflection_output = reflector.reflect(&ReflectionInput {
        decision: decision.clone(),
        outcome: outcome.clone(),
    });

    println!("│ 📝 Decision: {}", reflection_output.decision_id);
    println!("│ Ticker: {}", reflection_output.ticker);
    println!("│");
    println!("│ ✅ What went well:");
    for well in &reflection_output.what_went_well {
        println!("│   • {}", well);
    }
    if reflection_output.what_went_well.is_empty() {
        println!("│   (nothing notable)");
    }

    println!("│");
    println!("│ 🔧 Improvements:");
    for imp in &reflection_output.improvements {
        println!("│   • {}", imp);
    }
    if reflection_output.improvements.is_empty() {
        println!("│   (none — perfect execution!)");
    }

    println!("│");
    println!("│ 📖 Lesson: {}", reflection_output.lesson);
    println!("│ 🔄 Would repeat: {}", if reflection_output.would_repeat { "YES ✅" } else { "NO ❌" });
    println!("│ 📊 Confidence: {}", reflection_output.confidence);

    if !reflection_output.analogous_situations.is_empty() {
        println!("│");
        println!("│ 📌 Patterns matched:");
        for pattern in &reflection_output.analogous_situations {
            println!("│   • {}", pattern);
        }
    }

    // 9b: Save reflection to journal
    let trade_reflection = TradeReflector::to_trade_reflection(&reflection_output);
    journal.add_reflection(&decision_id, &trade_reflection)?;
    println!("│");
    println!("│ ✅ Reflection saved to journal!");

    // 9c: Save episode to memory
    sub_header("Episodic Memory — Lưu bài học");
    let episode = Episode {
        id: format!("ep_{}", Utc::now().format("%Y%m%d_%H%M%S")),
        ticker: ticker.clone(),
        regime: md.regime.clone(),
        strategy: "ALMA Crossover".to_string(),
        direction: format!("{:?}", action),
        context_description: format!(
            "RSI 4H {:.1} {}, Hurst {:.3}, Funding {:.4}%, F&G {}, {}",
            md.rsi_4h, rsi_status(md.rsi_4h), md.hurst_4h,
            md.funding_rate, md.fear_greed, md.regime,
        ),
        tags: vec![
            ticker.to_lowercase(),
            md.regime.to_lowercase().replace(' ', "_"),
            if md.rsi_4h < 40.0 { "oversold".to_string() } else { "neutral_rsi".to_string() },
            if pnl_pct_f64 > 0.0 { "win".to_string() } else { "loss".to_string() },
        ],
        entry_price: entry_price.to_string(),
        stop_loss: stop_loss.to_string(),
        outcome: EpisodeOutcome {
            is_win: pnl_pct_f64 > 0.0,
            pnl_pct: pnl_pct_f64,
            exit_reason: "take_profit_1".to_string(),
            mfe_pct: pnl_pct_f64 * 1.4,
            mae_pct: sl_pct * 100.0 * 0.5,
        },
        lesson: reflection_output.lesson.clone(),
        entry_confidence: 0.78,
        rsi_4h: Some(md.rsi_4h),
        hurst_4h: Some(md.hurst_4h),
        fear_greed: Some(md.fear_greed as u8),
        pnl_pct: pnl_pct_f64,
        holding_hours: 36.0,
        debate_rounds: decision.debate_rounds,
        created_at: Utc::now(),
    };

    store.store(&episode)?;
    println!("│ ✅ Episode saved to memory!");
    println!("│   ID: {}", episode.id);
    println!("│   Tags: {:?}", episode.tags);
    println!("│   Keywords: {:?}", episode.keywords());
    println!("└{}", "─".repeat(60));

    // ═══════════════════════════════════════════════════════════════
    // FINAL SUMMARY
    // ═══════════════════════════════════════════════════════════════
    separator("📊 WORKFLOW SUMMARY");

    println!("│");
    println!("│  Bước 1: 📡 Market Analysis        ✅ {}", ticker);
    println!("│  Bước 2: 🧠 Memory Check           ✅ Checked similar trades");
    println!("│  Bước 3: 🔴🔵 Research Debate       ✅ {:?} (strength: {:.2})",
        research.consensus_direction, research.consensus_strength);
    println!("│  Bước 4: 🟢🟡🔴 Risk Debate          ✅ {:?} risk, size {:.0}% Kelly",
        risk.risk_consensus.risk_level, risk.position_size_multiplier * dec!(100));
    println!("│  Bước 5: 🛡️  Hard Guards             ✅ All passed");
    println!("│  Bước 6: 📝 Decision Journal         ✅ Saved {}", decision_id);
    println!("│  Bước 7: 💰 Execute                  ✅ Simulated");
    println!("│  Bước 8: 📊 Update Outcome           ✅ +{}% (TP1)", pnl_pct_dec);
    println!("│  Bước 9: 🪞 Reflection + Memory      ✅ Lesson saved");
    println!("│");
    println!("│  🏆 KẾT QUẢ: +{}% profit | {} rounds debate | lesson saved",
        pnl_pct_dec, decision.debate_rounds);
    println!("│");
    println!("│  📂 Journal DB: {}", journal_config.db_path);
    println!("│  📂 Memory DB:  {} episodes stored", store.count()?);
    println!("└{}", "─".repeat(60));

    println!("\n✅ Full workflow hoàn thành thành công!\n");

    Ok(())
}
