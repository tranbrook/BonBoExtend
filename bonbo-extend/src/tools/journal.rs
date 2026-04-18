//! Journal MCP Tools — trade journal and learning metrics.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Mutex;

use bonbo_journal::journal::JournalStore;
use bonbo_journal::models::*;
use bonbo_journal::performance::PerformanceTracker;

pub struct JournalPlugin {
    metadata: PluginMetadata,
    store: Mutex<Option<JournalStore>>,
}

impl JournalPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-journal".into(), name: "Trade Journal".into(),
                version: env!("CARGO_PKG_VERSION").into(), description: "Trade journal with learning metrics".into(),
                author: "BonBo Team".into(), tags: vec!["journal".into(), "learning".into()],
            },
            store: Mutex::new(None),
        }
    }

    fn ensure_store(&self) -> anyhow::Result<()> {
        let mut store = self.store.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        if store.is_none() {
            let db_path = std::env::var("BONBO_JOURNAL_DB")
                .unwrap_or_else(|_| "bonbo_journal.db".to_string());
            *store = Some(JournalStore::open(std::path::Path::new(&db_path))?);
        }
        Ok(())
    }
}

#[async_trait]
impl ToolPlugin for JournalPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "journal_trade_entry".into(), description: "Record a trade entry with analysis snapshot".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Symbol (e.g., BTCUSDT)".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "price".into(), param_type: "number".into(), description: "Current price".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "recommendation".into(), param_type: "string".into(), description: "STRONG_BUY/BUY/HOLD/SELL/STRONG_SELL".into(), required: true, default: None, r#enum: Some(vec!["STRONG_BUY".into(),"BUY".into(),"HOLD".into(),"SELL".into(),"STRONG_SELL".into()]) },
                ParameterSchema { name: "quant_score".into(), param_type: "number".into(), description: "Score 0-100".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "stop_loss".into(), param_type: "number".into(), description: "Stop loss".into(), required: false, default: Some(json!(0)), r#enum: None },
                ParameterSchema { name: "target_price".into(), param_type: "number".into(), description: "Target price".into(), required: false, default: Some(json!(0)), r#enum: None },
            ]},
            ToolSchema { name: "journal_trade_outcome".into(), description: "Record actual outcome for a trade".into(), parameters: vec![
                ParameterSchema { name: "entry_id".into(), param_type: "string".into(), description: "Entry ID".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "exit_price".into(), param_type: "number".into(), description: "Exit price".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "direction_correct".into(), param_type: "boolean".into(), description: "Direction correct?".into(), required: true, default: None, r#enum: None },
            ]},
            ToolSchema { name: "get_trade_journal".into(), description: "Query trade journal".into(), parameters: vec![
                ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Filter symbol".into(), required: false, default: None, r#enum: None },
                ParameterSchema { name: "limit".into(), param_type: "number".into(), description: "Max entries".into(), required: false, default: Some(json!(20)), r#enum: None },
            ]},
            ToolSchema { name: "get_learning_metrics".into(), description: "Learning metrics: accuracy, per-indicator, per-regime".into(), parameters: vec![] },
        ]
    }

    async fn execute_tool(&self, tool_name: &str, args: &Value, _ctx: &PluginContext) -> anyhow::Result<String> {
        self.ensure_store()?;
        let guard = self.store.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let store = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Store not initialized"))?;

        match tool_name {
            "journal_trade_entry" => {
                let symbol = args["symbol"].as_str().ok_or_else(|| anyhow::anyhow!("symbol required"))?;
                let price = args["price"].as_f64().ok_or_else(|| anyhow::anyhow!("price required"))?;
                let rec_str = args["recommendation"].as_str().ok_or_else(|| anyhow::anyhow!("recommendation required"))?;
                let score = args["quant_score"].as_f64().unwrap_or(50.0);
                let sl = args["stop_loss"].as_f64().unwrap_or(price * 0.96);
                let tp = args["target_price"].as_f64().unwrap_or(price * 1.08);
                let recommendation = match rec_str {
                    "STRONG_BUY" => Recommendation::StrongBuy, "BUY" => Recommendation::Buy,
                    "SELL" => Recommendation::Sell, "STRONG_SELL" => Recommendation::StrongSell, _ => Recommendation::Hold,
                };
                let mut snapshot = AnalysisSnapshot::default();
                snapshot.symbol = symbol.to_string(); snapshot.price = price; snapshot.quant_score = score;
                snapshot.timestamp = chrono::Utc::now().timestamp();
                let rr = if sl > 0.0 { (tp - price) / (price - sl) } else { 0.0 };
                let entry = TradeJournalEntry {
                    id: uuid::Uuid::new_v4().to_string(), timestamp: snapshot.timestamp, snapshot,
                    recommendation, entry_price: price, stop_loss: sl, target_price: tp,
                    risk_reward_ratio: rr, position_size_usd: 0.0, outcome: None,
                };
                let id = entry.id.clone();
                store.insert_entry(&entry)?;
                Ok(format!("📝 **Trade Entry**\nID: `{}`\n{} @ ${:.2}\n{} | Score: {:.0}\nSL: ${:.2} TP: ${:.2} R:R {:.1}", id, symbol, price, rec_str, score, sl, tp, rr))
            }
            "journal_trade_outcome" => {
                let entry_id = args["entry_id"].as_str().ok_or_else(|| anyhow::anyhow!("entry_id required"))?;
                let exit_price = args["exit_price"].as_f64().ok_or_else(|| anyhow::anyhow!("exit_price required"))?;
                let dir_ok = args["direction_correct"].as_bool().ok_or_else(|| anyhow::anyhow!("direction_correct required"))?;
                let entry = store.get_entry(entry_id)?;
                let ret = if entry.entry_price > 0.0 { (exit_price - entry.entry_price) / entry.entry_price * 100.0 } else { 0.0 };
                let outcome = TradeOutcome {
                    close_timestamp: chrono::Utc::now().timestamp(), exit_price, actual_return_pct: ret,
                    hit_target: exit_price >= entry.target_price, hit_stoploss: exit_price <= entry.stop_loss,
                    holding_period_hours: 0, max_favorable_excursion: ret.max(0.0), max_adverse_excursion: ret.min(0.0),
                    direction_correct: dir_ok, score_accuracy: ret.abs(), indicator_accuracy: std::collections::HashMap::new(),
                };
                store.record_outcome(entry_id, &outcome)?;
                let e = if dir_ok { "✅" } else { "❌" };
                Ok(format!("{} **Outcome**: `{}` exit ${:.2} ret {:+.2}%", e, entry_id, exit_price, ret))
            }
            "get_trade_journal" => {
                let mut q = JournalQuery::default();
                if let Some(s) = args.get("symbol").and_then(|v| v.as_str()) { q.symbol = Some(s.to_string()); }
                q.limit = args.get("limit").and_then(|v| v.as_u64()).map(|l| l as u32);
                let entries = store.query_entries(&q)?;
                let count = store.count_entries(&q)?;
                let mut r = format!("📋 **Journal** ({} entries)\n", count);
                for e in &entries {
                    let o = match &e.outcome { Some(x) if x.direction_correct => "✅", Some(_) => "❌", None => "⏳" };
                    r.push_str(&format!("{} `{}` | {} ${:.0} {} Score:{:.0}\n", o, &e.id[..8], e.snapshot.symbol, e.snapshot.price, e.recommendation.as_str(), e.snapshot.quant_score));
                }
                Ok(r)
            }
            "get_learning_metrics" => {
                let tracker = PerformanceTracker::new(store);
                let m = tracker.compute_metrics()?;
                Ok(format!("📊 **Learning Metrics**\nPredictions: {} | Outcomes: {}\nDirection: {:.1}% | Win: {:.1}% | Avg Ret: {:+.2}%\nSharpe: {:.2} | PF: {:.2} | Recent10: {:.1}%",
                    m.total_predictions, m.total_with_outcome, m.direction_accuracy*100.0, m.win_rate*100.0, m.avg_return_pct, m.sharpe_of_predictions, m.profit_factor, m.recent_10_accuracy*100.0))
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
