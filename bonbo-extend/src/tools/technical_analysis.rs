//! Technical Analysis Plugin — exposes bonbo-ta indicators via MCP tools.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct TechnicalAnalysisPlugin {
    metadata: PluginMetadata,
}

impl TechnicalAnalysisPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-technical-analysis".to_string(),
                name: "Technical Analysis".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Technical analysis indicators, signals, and market regime detection".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec!["analysis".to_string(), "indicators".to_string(), "ta".to_string()],
            },
        }
    }

    async fn fetch_candles(&self, symbol: &str, interval: &str, limit: u32) -> anyhow::Result<Vec<bonbo_ta::models::OhlcvCandle>> {
        let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();
        let raw = fetcher.fetch_klines(symbol, interval, Some(limit)).await?;
        Ok(raw.into_iter().map(|c| bonbo_ta::models::OhlcvCandle {
            timestamp: c.timestamp, open: c.open, high: c.high, low: c.low, close: c.close, volume: c.volume,
        }).collect())
    }

    async fn do_analyze_indicators(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let limit = args["limit"].as_u64().unwrap_or(100) as u32;
        let candles = self.fetch_candles(symbol, interval, limit).await?;
        if candles.len() < 30 { return Ok("⚠️ Not enough candles for analysis (need at least 30)".to_string()); }
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let analysis = bonbo_ta::batch::compute_full_analysis(&closes);
        let mut result = format!("📊 **Technical Analysis — {} ({})**\n\n", symbol, interval);
        if let Some(Some(v)) = analysis.sma20.last() { result.push_str(&format!("📈 **SMA(20)**: ${:.2}\n", v)); }
        if let Some(Some(v)) = analysis.ema12.last() { result.push_str(&format!("📈 **EMA(12)**: ${:.2}\n", v)); }
        if let Some(Some(v)) = analysis.ema26.last() { result.push_str(&format!("📈 **EMA(26)**: ${:.2}\n", v)); }
        if let Some(Some(v)) = analysis.rsi14.last() {
            let label = if *v > 70.0 { "🔴 Overbought" } else if *v < 30.0 { "🟢 Oversold" } else { "⚪ Neutral" };
            result.push_str(&format!("\n📉 **RSI(14)**: {:.1} {}\n", v, label));
        }
        if let Some(Some(m)) = analysis.macd.last() {
            result.push_str(&format!("\n📊 **MACD**: line={:.4} signal={:.4} hist={:.4} {}\n",
                m.macd_line, m.signal_line, m.histogram, if m.histogram > 0.0 { "🟢" } else { "🔴" }));
        }
        if let Some(Some(bb)) = analysis.bb.last() {
            result.push_str(&format!("\n🎯 **BB(20,2)**: upper=${:.2} mid=${:.2} lower=${:.2} %B={:.2}\n", bb.upper, bb.middle, bb.lower, bb.percent_b));
        }
        if let Some(p) = closes.last() { result.push_str(&format!("\n💰 **Price**: ${:.2}", p)); }
        Ok(result)
    }

    async fn do_get_trading_signals(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let candles = self.fetch_candles(symbol, interval, 100).await?;
        if candles.len() < 30 { return Ok("⚠️ Not enough data".to_string()); }
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let analysis = bonbo_ta::batch::compute_full_analysis(&closes);
        let price = closes.last().copied().unwrap_or(0.0);
        let signals = bonbo_ta::batch::generate_signals(&analysis, price);
        let mut result = format!("🎯 **Trading Signals — {} ({})**\n💰 Price: ${:.2}\n\n", symbol, interval, price);
        if signals.is_empty() {
            result.push_str("⚪ No strong signals detected\n");
        } else {
            for sig in &signals {
                let icon = match sig.signal_type {
                    bonbo_ta::models::SignalType::StrongBuy => "🟢🟢",
                    bonbo_ta::models::SignalType::Buy => "🟢",
                    bonbo_ta::models::SignalType::Neutral => "⚪",
                    bonbo_ta::models::SignalType::Sell => "🔴",
                    bonbo_ta::models::SignalType::StrongSell => "🔴🔴",
                };
                result.push_str(&format!("{} **{:?}** [{}] ({:.0}%)\n   {}\n", icon, sig.signal_type, sig.source, sig.confidence * 100.0, sig.reason));
            }
        }
        Ok(result)
    }

    async fn do_detect_market_regime(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let candles = self.fetch_candles(symbol, interval, 50).await?;
        if candles.len() < 20 { return Ok("⚠️ Not enough data".to_string()); }
        let regime = bonbo_ta::batch::detect_market_regime(&candles);
        let price = candles.last().map(|c| c.close).unwrap_or(0.0);
        let desc = match regime {
            bonbo_ta::models::MarketRegime::TrendingUp => "Uptrend — follow the trend",
            bonbo_ta::models::MarketRegime::TrendingDown => "Downtrend — consider shorts",
            bonbo_ta::models::MarketRegime::Ranging => "Sideways — range strategies",
            bonbo_ta::models::MarketRegime::Volatile => "High volatility — use wider stops",
            bonbo_ta::models::MarketRegime::Quiet => "Low volatility — breakout incoming",
        };
        Ok(format!("{}\n\n💰 **{}** @ ${:.2}\n📝 {}", regime, symbol, price, desc))
    }

    async fn do_get_support_resistance(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let lookback = args["lookback"].as_u64().unwrap_or(60) as u32;
        let candles = self.fetch_candles(symbol, interval, lookback).await?;
        if candles.len() < 10 { return Ok("⚠️ Not enough data".to_string()); }
        let highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let lows: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let (supports, resistances) = bonbo_ta::batch::get_support_resistance(&highs, &lows);
        let price = candles.last().map(|c| c.close).unwrap_or(0.0);
        let mut result = format!("🎯 **S/R — {} ({})** @ ${:.2}\n\n", symbol, interval, price);
        result.push_str("🔴 **Resistance**:\n");
        for (i, r) in resistances.iter().enumerate() { result.push_str(&format!("  R{}: ${:.2} (+{:.1}%)\n", i+1, r, (r-price)/price*100.0)); }
        result.push_str("\n🟢 **Support**:\n");
        for (i, s) in supports.iter().enumerate() { result.push_str(&format!("  S{}: ${:.2} (-{:.1}%)\n", i+1, s, (price-s)/price*100.0)); }
        Ok(result)
    }
}

#[async_trait]
impl ToolPlugin for TechnicalAnalysisPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "analyze_indicators".into(), description: "Compute technical indicators (SMA, EMA, RSI, MACD, BB) for a crypto symbol".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair (e.g. BTCUSDT)".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "interval".into(), param_type: "string".into(), description: "Candle interval".into(), required: false, default: Some(json!("1d")), r#enum: Some(vec!["1m".into(),"5m".into(),"15m".into(),"1h".into(),"4h".into(),"1d".into()]) },
                ParameterSchema { name: "limit".into(), param_type: "integer".into(), description: "Number of candles".into(), required: false, default: Some(json!(100)), r#enum: None },
            ]},
            ToolSchema { name: "get_trading_signals".into(), description: "Generate buy/sell signals from indicator confluence".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "interval".into(), param_type: "string".into(), description: "Candle interval".into(), required: false, default: Some(json!("1d")), r#enum: None },
            ]},
            ToolSchema { name: "detect_market_regime".into(), description: "Detect market regime (trending/ranging/volatile)".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "interval".into(), param_type: "string".into(), description: "Candle interval".into(), required: false, default: Some(json!("1d")), r#enum: None },
            ]},
            ToolSchema { name: "get_support_resistance".into(), description: "Identify support and resistance levels".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "interval".into(), param_type: "string".into(), description: "Candle interval".into(), required: false, default: Some(json!("1d")), r#enum: None },
                ParameterSchema { name: "lookback".into(), param_type: "integer".into(), description: "Lookback period".into(), required: false, default: Some(json!(60)), r#enum: None },
            ]},
        ]
    }
    async fn execute_tool(&self, tool_name: &str, arguments: &Value, _context: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "analyze_indicators" => self.do_analyze_indicators(arguments).await,
            "get_trading_signals" => self.do_get_trading_signals(arguments).await,
            "detect_market_regime" => self.do_detect_market_regime(arguments).await,
            "get_support_resistance" => self.do_get_support_resistance(arguments).await,
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
