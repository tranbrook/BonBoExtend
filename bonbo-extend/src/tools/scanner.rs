//! Scanner MCP Tools.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

use bonbo_scanner::scanner::MarketScanner;
use bonbo_scanner::models::ScanConfig;
use bonbo_scanner::scheduler::ScanScheduler;

pub struct ScannerPlugin { metadata: PluginMetadata }
impl ScannerPlugin {
    pub fn new() -> Self {
        Self { metadata: PluginMetadata {
            id: "bonbo-scanner".into(), name: "Market Scanner".into(),
            version: env!("CARGO_PKG_VERSION").into(), description: "Autonomous market scanning".into(),
            author: "BonBo Team".into(), tags: vec!["scanner".into()],
        }}
    }
}

#[async_trait]
impl ToolPlugin for ScannerPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "scan_market".into(), description: "Scan crypto markets".into(), parameters: vec![
                ParameterSchema { name: "min_score".into(), param_type: "number".into(), description: "Min score to alert".into(), required: false, default: Some(json!(55)), r#enum: None },
            ]},
            ToolSchema { name: "get_scan_schedule".into(), description: "Scheduled scan config".into(), parameters: vec![] },
        ]
    }
    async fn execute_tool(&self, tool_name: &str, args: &Value, _ctx: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "scan_market" => {
                let mut config = ScanConfig::default();
                config.min_score = args.get("min_score").and_then(|v| v.as_f64()).unwrap_or(55.0);
                let scanner = MarketScanner::new(config);
                let data = vec![
                    ("BTCUSDT".into(), 77_000.0, 68.0, "Ranging".into(), vec!["RSI bullish".into(), "MACD cross".into()], 0.3, 1.2),
                    ("ETHUSDT".into(), 2_400.0, 62.0, "Ranging".into(), vec!["BB bounce".into()], -0.2, 0.9),
                    ("SOLUSDT".into(), 150.0, 74.0, "TrendingUp".into(), vec!["EMA cross".into()], 0.5, 1.8),
                    ("BNBUSDT".into(), 600.0, 48.0, "Ranging".into(), vec![], 0.0, 0.5),
                    ("XRPUSDT".into(), 2.1, 35.0, "Volatile".into(), vec!["MACD bearish".into()], -0.4, -0.3),
                ];
                let report = scanner.generate_report(data)?;
                let mut r = format!("🔍 **Scan** | {} symbols | Regime: {}\n\n", report.symbols_scanned, report.regime);
                for p in &report.top_picks {
                    let e = match p.recommendation.as_str() { "STRONG_BUY"=>"🟢🟢","BUY"=>"🟢","SELL"=>"🔴","STRONG_SELL"=>"🔴🔴",_=>"⚪" };
                    r.push_str(&format!("{} {} {:.0} ({}) ${:.0} {}\n", e, p.symbol, p.quant_score, p.recommendation, p.price, p.regime));
                }
                Ok(r)
            }
            "get_scan_schedule" => {
                let scheduler = ScanScheduler::new();
                let mut r = "⏰ **Schedule**\n".to_string();
                for s in scheduler.list_scans() {
                    let st = if s.enabled { "✅" } else { "❌" };
                    r.push_str(&format!("{} **{}** ({}h)\n", st, s.name, s.interval_hours));
                }
                Ok(r)
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
