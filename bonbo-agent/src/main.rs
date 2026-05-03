//! BonBo Agent — 24/7 autonomous live trading agent.
//!
//! Entry point for the trading agent. Reads config from `config/trading.toml`
//! and environment variables from `.env`, then runs the Orchestrator loop.
//!
//! # Usage
//! ```bash
//! # Dry-run (paper trading)
//! BONBO_MODE=dry_run cargo run -p bonbo-agent
//!
//! # Testnet
//! BONBO_MODE=testnet cargo run -p bonbo-agent
//!
//! # Live (production)
//! cargo run -p bonbo-agent
//! ```

use anyhow::Context;
use bonbo_agent::config::AgentConfig;
use bonbo_agent::orchestrator::Orchestrator;
use std::path::Path;

/// Load `.env` file into environment variables.
fn load_dotenv() {
    let env_path = Path::new(".env");
    if !env_path.exists() {
        tracing::debug!("No .env file found — using existing environment variables");
        return;
    }

    let content = match std::fs::read_to_string(env_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read .env: {}", e);
            return;
        }
    };

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            // Only set if not already present (env vars take precedence)
            if std::env::var(key).is_err() {
                // SAFETY: set_var is unsafe in edition 2024 because it can
                // cause data races in multi-threaded contexts. We call this
                // before spawning any threads, so it's safe.
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
}

/// Load config from `config/trading.toml`.
fn load_config() -> anyhow::Result<AgentConfig> {
    let config_path = Path::new("config/trading.toml");

    if config_path.exists() {
        let content = std::fs::read_to_string(config_path)
            .with_context(|| "Failed to read config/trading.toml")?;
        let config: AgentConfig =
            toml::from_str(&content).with_context(|| "Failed to parse config/trading.toml")?;
        Ok(config)
    } else {
        tracing::warn!("No config/trading.toml found — using testnet defaults");
        Ok(AgentConfig::testnet_default())
    }
}

/// Setup tracing/logging with colored output.
fn setup_tracing(log_level: &str) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load .env
    load_dotenv();

    // 2. Setup logging
    let log_level = std::env::var("BONBO_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    setup_tracing(&log_level);

    // 3. Print banner
    tracing::info!("╔══════════════════════════════════════════╗");
    tracing::info!("║       🤖 BonBo Trading Agent v0.2.0      ║");
    tracing::info!("║       24/7 Autonomous Live Trading        ║");
    tracing::info!("╚══════════════════════════════════════════╝");

    // 4. Load config
    let config = load_config()?;

    // 5. Validate mode
    let mode = &config.account.mode;
    match mode.as_str() {
        "live" => {
            tracing::warn!("⚠️  LIVE TRADING MODE — real money at risk!");
            // Verify API keys exist
            if std::env::var("BINANCE_API_KEY")
                .unwrap_or_default()
                .is_empty()
            {
                anyhow::bail!("BINANCE_API_KEY not set — required for live trading");
            }
            if std::env::var("BINANCE_API_SECRET")
                .unwrap_or_default()
                .is_empty()
            {
                anyhow::bail!("BINANCE_API_SECRET not set — required for live trading");
            }
        }
        "testnet" => {
            tracing::info!("🧪 TESTNET MODE — using Binance testnet");
        }
        "dry_run" => {
            tracing::info!("📋 DRY-RUN MODE — paper trading (no real orders)");
        }
        _ => {
            anyhow::bail!(
                "Unknown trading mode: '{}'. Must be 'live', 'testnet', or 'dry_run'",
                mode
            );
        }
    }

    // 6. Print config summary
    tracing::info!("Mode: {}", mode);
    tracing::info!(
        "Initial capital: {} {}",
        config.account.initial_capital,
        config.account.currency
    );
    tracing::info!(
        "Watchlist: {} symbols — {:?}",
        config.watchlist.symbols.len(),
        config.watchlist.symbols
    );
    tracing::info!("Max leverage: {}x", config.risk.max_leverage);
    tracing::info!("Max open positions: {}", config.risk.max_open_positions);
    tracing::info!("Daily loss limit: {}%", config.risk.daily_loss_limit_pct);
    tracing::info!("Max drawdown: {}%", config.risk.max_drawdown_pct);
    tracing::info!("Min risk:reward: {}", config.risk.min_risk_reward);
    tracing::info!(
        "Scan interval: {} min",
        config.strategy.scan_interval_minutes
    );
    tracing::info!("Min quant score: {}", config.strategy.min_quant_score);

    // 7. Setup graceful shutdown (Ctrl+C)
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let shutdown_mode = mode.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");

        tracing::warn!("🛑 Ctrl+C received — shutting down gracefully...");

        if shutdown_mode == "live" {
            tracing::warn!("⚠️  LIVE MODE: Waiting for current cycle to complete before exit...");
        }

        let _ = shutdown_tx.send(true);
    });

    // 8. Create and run orchestrator
    let orchestrator = Orchestrator::new(config);

    tracing::info!("🚀 Starting BonBo Agent...");

    // Run the agent in a task
    let agent_handle = tokio::spawn(async move {
        if let Err(e) = orchestrator.run().await {
            tracing::error!("Agent error: {}", e);
            Err(e)
        } else {
            Ok(())
        }
    });

    // Wait for shutdown signal
    let mut shutdown_rx = shutdown_rx;
    shutdown_rx.changed().await?;

    tracing::info!("🛑 Shutdown signal received — agent stopping...");

    // Abort the agent (the orchestrator loop checks for stop state)
    agent_handle.abort();

    tracing::info!("✅ BonBo Agent shutdown complete.");
    Ok(())
}
