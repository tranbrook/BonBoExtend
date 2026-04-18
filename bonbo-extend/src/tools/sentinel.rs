//! Sentinel Plugin — on-chain analytics and sentiment via MCP tools.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct SentinelPlugin { metadata: PluginMetadata }
impl SentinelPlugin {
    pub fn new() -> Self {
        Self { metadata: PluginMetadata {
            id: "bonbo-sentinel".into(), name: "Sentinel — On-chain & Sentiment".into(),
            version: env!("CARGO_PKG_VERSION").into(), description: "On-chain analytics, sentiment analysis, whale alerts".into(),
            author: "BonBo Team".into(), tags: vec!["sentiment".into(), "on-chain".into()],
        }}
    }
}

#[async_trait]
impl ToolPlugin for SentinelPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "get_fear_greed_index".into(), description: "Get crypto Fear & Greed Index".into(), parameters: vec![
                ParameterSchema { name: "history".into(), param_type: "integer".into(), description: "Days of history (1-30)".into(), required: false, default: Some(json!(1)), r#enum: None },
            ]},
            ToolSchema { name: "get_whale_alerts".into(), description: "Get recent large crypto transactions (simulated)".into(), parameters: vec![
                ParameterSchema { name: "min_usd".into(), param_type: "number".into(), description: "Min USD value".into(), required: false, default: Some(json!(1000000)), r#enum: None },
            ]},
            ToolSchema { name: "get_composite_sentiment".into(), description: "Composite market sentiment from multiple sources".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Symbol for context".into(), required: false, default: Some(json!("BTCUSDT")), r#enum: None },
            ]},
        ]
    }
    async fn execute_tool(&self, tool_name: &str, args: &Value, _context: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "get_fear_greed_index" => self.get_fear_greed(args).await,
            "get_whale_alerts" => self.get_whale_alerts(args),
            "get_composite_sentiment" => self.get_composite(args).await,
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

impl SentinelPlugin {
    async fn get_fear_greed(&self, args: &Value) -> anyhow::Result<String> {
        let history = args["history"].as_u64().unwrap_or(1) as u32;
        let fg = bonbo_sentinel::FearGreedIndex::new();
        if history <= 1 {
            let signal = fg.fetch().await?;
            let emoji = match signal.raw_value as u32 {
                0..=25 => "😱 Extreme Fear",
                26..=45 => "😟 Fear",
                46..=55 => "😐 Neutral",
                56..=75 => "😊 Greed",
                _ => "🤑 Extreme Greed",
            };
            Ok(format!("📊 **Fear & Greed Index**\n\n{}: {} ({:.0}/100)\nNormalized: {:.2}",
                emoji, signal.label, signal.raw_value, signal.value))
        } else {
            let signals = fg.fetch_history(history).await?;
            let mut result = format!("📊 **Fear & Greed — {} Days**\n\n| Date | Value | Class |\n|------|-------|-------|\n", signals.len());
            for s in &signals {
                let date = chrono::DateTime::from_timestamp(s.timestamp, 0)
                    .map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| s.timestamp.to_string());
                result.push_str(&format!("| {} | {:.0} | {} |\n", date, s.raw_value, s.label));
            }
            Ok(result)
        }
    }

    fn get_whale_alerts(&self, args: &Value) -> anyhow::Result<String> {
        let min_usd = args["min_usd"].as_f64().unwrap_or(1_000_000.0);
        let fetcher = bonbo_sentinel::WhaleAlertFetcher::new();
        let txs = fetcher.fetch_large_transactions(min_usd)?;
        if txs.is_empty() { return Ok(format!("🐋 **Whale Alerts** (>${:.0})\n\nNo large transactions.", min_usd)); }
        let mut result = format!("🐋 **Whale Alerts** (>${:.0})\n\n", min_usd);
        for tx in &txs {
            let dir = if tx.is_exchange_inflow { "🔴 Inflow (sell pressure)" }
                else if tx.is_exchange_outflow { "🟢 Outflow (hold)" } else { "⚪ Transfer" };
            result.push_str(&format!("{} **${:.0}** {} — {} ({}...→{}...)\n",
                dir, tx.amount_usd, tx.token, tx.blockchain,
                &tx.from_addr[..10.min(tx.from_addr.len())], &tx.to_addr[..10.min(tx.to_addr.len())]));
        }
        Ok(result)
    }

    async fn get_composite(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let fg = bonbo_sentinel::FearGreedIndex::new();
        let fg_signal = fg.fetch().await.ok();
        let whale_fetcher = bonbo_sentinel::WhaleAlertFetcher::new();
        let whale_txs = whale_fetcher.fetch_large_transactions(5_000_000.0).ok();
        let whale_signal = whale_txs.as_ref().and_then(|txs| bonbo_sentinel::WhaleAlertFetcher::to_sentiment_signal(txs));
        let mut signals: Vec<bonbo_sentinel::models::SentimentSignal> = Vec::new();
        if let Some(s) = &fg_signal { signals.push(s.clone()); }
        if let Some(s) = &whale_signal { signals.push(s.clone()); }
        let report = bonbo_sentinel::composite::generate_sentiment_report(fg_signal, signals);
        let interpretation = bonbo_sentinel::composite::interpret_score(report.composite_score);
        let mut result = format!("📊 **Composite Sentiment — {}**\n\n🎯 Score: {:.2} ({})\n\n", symbol, report.composite_score, interpretation);
        for s in &report.signals {
            let icon = if s.value > 0.2 { "🟢" } else if s.value < -0.2 { "🔴" } else { "⚪" };
            result.push_str(&format!("{} {} = {:.2} ({})\n", icon, s.source, s.value, s.label));
        }
        result.push_str("\n💡 ");
        if report.composite_score > 0.3 { result.push_str("Bullish sentiment — consider longs with risk management."); }
        else if report.composite_score < -0.3 { result.push_str("Bearish sentiment — defensive positioning advised."); }
        else { result.push_str("Neutral sentiment — wait for clearer signals."); }
        Ok(result)
    }
}
