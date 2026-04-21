//! Regime MCP Tools — BOCPD regime detection from real market data.

use crate::plugin::{ParameterSchema, PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{Value, json};

use bonbo_regime::classifier::RegimeClassifier;
use bonbo_regime::models::{MarketRegime, RegimeConfig};

pub struct RegimePlugin {
    metadata: PluginMetadata,
}
impl Default for RegimePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl RegimePlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-regime".into(),
                name: "Regime Detection".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "BOCPD regime detection from real data".into(),
                author: "BonBo Team".into(),
                tags: vec!["regime".into()],
            },
        }
    }
}

#[async_trait]
impl ToolPlugin for RegimePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "detect_market_regime".into(),
            description:
                "Detect market regime from real price data using BOCPD + statistical analysis"
                    .into(),
            parameters: vec![
                ParameterSchema {
                    name: "symbol".into(),
                    param_type: "string".into(),
                    description: "Symbol (e.g., BTCUSDT)".into(),
                    required: true,
                    default: None,
                    r#enum: None,
                },
                ParameterSchema {
                    name: "timeframe".into(),
                    param_type: "string".into(),
                    description: "Timeframe for data".into(),
                    required: false,
                    default: Some(json!("4h")),
                    r#enum: Some(vec!["1h".into(), "4h".into(), "1d".into()]),
                },
                ParameterSchema {
                    name: "closes".into(),
                    param_type: "array".into(),
                    description: "Optional: provide close prices directly (auto-fetched if empty)"
                        .into(),
                    required: false,
                    default: None,
                    r#enum: None,
                },
            ],
        }]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        args: &Value,
        _ctx: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "detect_market_regime" => {
                let symbol = args["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol required"))?;
                let timeframe = args
                    .get("timeframe")
                    .and_then(|v| v.as_str())
                    .unwrap_or("4h");

                // Try to use provided closes, or fetch from Binance
                let closes = if let Some(arr) = args.get("closes").and_then(|v| v.as_array()) {
                    arr.iter().filter_map(|v| v.as_f64()).collect::<Vec<f64>>()
                } else {
                    self.fetch_closes(symbol, timeframe).await?
                };

                if closes.len() < 10 {
                    anyhow::bail!(
                        "Need at least 10 close prices, got {}. Try calling analyze_indicators first to cache data.",
                        closes.len()
                    );
                }

                let config = RegimeConfig::default();
                let mut classifier = RegimeClassifier::new(config);
                let state = classifier.detect_from_closes(&closes, chrono::Utc::now().timestamp());

                let emoji = match state.current_regime {
                    MarketRegime::TrendingUp => "📈",
                    MarketRegime::TrendingDown => "📉",
                    MarketRegime::Ranging => "↔️",
                    MarketRegime::Volatile => "⚡",
                    MarketRegime::Quiet => "😴",
                };

                let mut r = format!(
                    "{} **{} Regime: {}**\n📊 Analyzed: {} candles ({})\n🎯 Confidence: {:.1}% | 🔄 Change Prob: {:.1}%\n\n",
                    emoji,
                    symbol,
                    state.current_regime,
                    closes.len(),
                    timeframe,
                    state.confidence * 100.0,
                    state.change_probability * 100.0
                );

                r.push_str("**Regime Probabilities:**\n");
                for (regime, prob) in &state.regime_probabilities {
                    let filled = (prob * 20.0) as usize;
                    let bar = "█".repeat(filled) + &"░".repeat(20 - filled);
                    r.push_str(&format!("  {} [{}] {:.1}%\n", regime, bar, prob * 100.0));
                }

                if let Some(cp) = &state.last_change_point {
                    r.push_str(&format!(
                        "\n📍 Last change: index {} ({:.0}% conf) {} → {}\n",
                        cp.index,
                        cp.confidence * 100.0,
                        cp.prev_regime,
                        cp.new_regime
                    ));
                }

                // Price context
                if closes.len() >= 2 {
                    let first = closes.first().unwrap();
                    let last = closes.last().unwrap();
                    let change_pct = (last - first) / first * 100.0;
                    r.push_str(&format!(
                        "\n💰 Price range: ${:.2} → ${:.2} ({:+.2}%)\n",
                        first, last, change_pct
                    ));
                }

                r.push_str("\n💡 **Regime-specific advice:**\n");
                match state.current_regime {
                    MarketRegime::TrendingUp => r.push_str("  → Use trend-following (MACD, EMA cross)\n  → Increase momentum weight\n  → Favor breakout entries\n"),
                    MarketRegime::TrendingDown => r.push_str("  → Consider shorts or reduce exposure\n  → Bearish trend-following\n  → Avoid catch-falling-knife buys\n"),
                    MarketRegime::Ranging => r.push_str("  → Use mean-reversion (RSI, BB)\n  → Buy support, sell resistance\n  → Reduce position sizes\n"),
                    MarketRegime::Volatile => r.push_str("  → Reduce position size significantly\n  → Use wider stops\n  → Wait for volatility to settle\n"),
                    MarketRegime::Quiet => r.push_str("  → Low opportunity — watch for breakouts\n  → Accumulate positions gradually\n"),
                }

                Ok(r)
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

impl RegimePlugin {
    /// Fetch real close prices from Binance API.
    async fn fetch_closes(&self, symbol: &str, timeframe: &str) -> anyhow::Result<Vec<f64>> {
        let limit = 100;
        let url = format!(
            "https://api.binance.com/api/v3/klines?symbol={}&interval={}&limit={}",
            symbol, timeframe, limit
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let resp = client.get(&url).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("Binance API error: {}", resp.status());
        }

        let klines: Vec<Vec<Value>> = resp.json().await?;

        let closes: Vec<f64> = klines
            .iter()
            .filter_map(|k| {
                k.get(4)
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .collect();

        Ok(closes)
    }
}
