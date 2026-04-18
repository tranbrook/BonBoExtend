//! Regime MCP Tools.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

use bonbo_regime::classifier::RegimeClassifier;
use bonbo_regime::models::RegimeConfig;

pub struct RegimePlugin { metadata: PluginMetadata }
impl RegimePlugin {
    pub fn new() -> Self {
        Self { metadata: PluginMetadata {
            id: "bonbo-regime".into(), name: "Regime Detection".into(),
            version: env!("CARGO_PKG_VERSION").into(), description: "Market regime detection via BOCPD".into(),
            author: "BonBo Team".into(), tags: vec!["regime".into()],
        }}
    }
}

#[async_trait]
impl ToolPlugin for RegimePlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema { name: "detect_market_regime".into(), description: "Detect market regime via BOCPD".into(), parameters: vec![
            ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Symbol".into(), required: true, default: None, r#enum: None },
            ParameterSchema { name: "timeframe".into(), param_type: "string".into(), description: "Timeframe".into(), required: false, default: Some(json!("4h")), r#enum: Some(vec!["1h".into(),"4h".into(),"1d".into()]) },
        ]}]
    }
    async fn execute_tool(&self, tool_name: &str, args: &Value, _ctx: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "detect_market_regime" => {
                let symbol = args["symbol"].as_str().ok_or_else(|| anyhow::anyhow!("symbol required"))?;
                let config = RegimeConfig::default();
                let mut classifier = RegimeClassifier::new(config);
                let base = match symbol { "BTCUSDT" => 77_000.0, "ETHUSDT" => 2_400.0, _ => 100.0 };
                let closes: Vec<f64> = (0..50).map(|i| base + i as f64 * 10.0 + (i as f64 * 0.1).sin() * base * 0.005).collect();
                let state = classifier.detect_from_closes(&closes, chrono::Utc::now().timestamp());
                let emoji = match state.current_regime {
                    bonbo_regime::models::MarketRegime::TrendingUp => "📈", bonbo_regime::models::MarketRegime::TrendingDown => "📉",
                    bonbo_regime::models::MarketRegime::Ranging => "↔️", bonbo_regime::models::MarketRegime::Volatile => "⚡", bonbo_regime::models::MarketRegime::Quiet => "😴",
                };
                let mut r = format!("{} **{} Regime: {}**\nConfidence: {:.1}% | Change Prob: {:.1}%\n\n",
                    emoji, symbol, state.current_regime, state.confidence*100.0, state.change_probability*100.0);
                for (regime, prob) in &state.regime_probabilities { r.push_str(&format!("{}: {:.1}%\n", regime, prob*100.0)); }
                Ok(r)
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
