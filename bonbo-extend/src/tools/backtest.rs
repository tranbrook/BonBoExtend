//! Backtest Plugin — exposes bonbo-quant backtesting via MCP tools.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct BacktestPlugin { metadata: PluginMetadata }
impl BacktestPlugin {
    pub fn new() -> Self {
        Self { metadata: PluginMetadata {
            id: "bonbo-backtest".into(), name: "Backtesting Engine".into(),
            version: env!("CARGO_PKG_VERSION").into(), description: "Backtest trading strategies on historical data".into(),
            author: "BonBo Team".into(), tags: vec!["backtest".into(), "strategy".into()],
        }}
    }
}

#[async_trait]
impl ToolPlugin for BacktestPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema { name: "run_backtest".into(), description: "Run a backtest of a trading strategy on historical crypto data".into(), parameters: vec![
            ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair".into(), required: true, default: None, r#enum: None },
            ParameterSchema { name: "interval".into(), param_type: "string".into(), description: "Candle interval".into(), required: false, default: Some(json!("1d")), r#enum: None },
            ParameterSchema { name: "strategy".into(), param_type: "string".into(), description: "Strategy: sma_crossover or rsi_mean_reversion".into(), required: true, default: None, r#enum: Some(vec!["sma_crossover".into(),"rsi_mean_reversion".into()]) },
            ParameterSchema { name: "initial_capital".into(), param_type: "number".into(), description: "Starting capital USDT".into(), required: false, default: Some(json!(10000)), r#enum: None },
            ParameterSchema { name: "fast_period".into(), param_type: "integer".into(), description: "Fast period".into(), required: false, default: Some(json!(10)), r#enum: None },
            ParameterSchema { name: "slow_period".into(), param_type: "integer".into(), description: "Slow period".into(), required: false, default: Some(json!(30)), r#enum: None },
        ]}]
    }
    async fn execute_tool(&self, tool_name: &str, args: &Value, _context: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "run_backtest" => self.run_backtest(args).await,
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

impl BacktestPlugin {
    async fn run_backtest(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let strategy_name = args["strategy"].as_str().unwrap_or("sma_crossover");
        let initial_capital = args["initial_capital"].as_f64().unwrap_or(10000.0);
        let fast = args["fast_period"].as_u64().unwrap_or(10) as usize;
        let slow = args["slow_period"].as_u64().unwrap_or(30) as usize;

        let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();
        let raw = fetcher.fetch_klines(symbol, interval, Some(200)).await?;
        let candles: Vec<bonbo_ta::models::OhlcvCandle> = raw.into_iter().map(|c| bonbo_ta::models::OhlcvCandle {
            timestamp: c.timestamp, open: c.open, high: c.high, low: c.low, close: c.close, volume: c.volume,
        }).collect();
        if candles.len() < 50 { return Ok("⚠️ Not enough data (need 50+)".to_string()); }

        let config = bonbo_quant::models::BacktestConfig { initial_capital, ..Default::default() };
        let report = match strategy_name {
            "sma_crossover" => {
                let s = bonbo_quant::strategy::SmaCrossoverStrategy::new(fast, slow);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "rsi_mean_reversion" => {
                let s = bonbo_quant::strategy::RsiMeanReversionStrategy::new(fast.max(14), 30.0, 70.0);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            _ => anyhow::bail!("Unknown strategy: {}", strategy_name),
        };
        Ok(report.format_report())
    }
}
