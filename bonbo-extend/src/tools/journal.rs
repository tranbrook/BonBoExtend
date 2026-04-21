//! Journal MCP Tools — trade journal with DMA learning integration.

use crate::plugin::{ParameterSchema, PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Mutex;

use bonbo_journal::journal::JournalStore;
use bonbo_journal::models::*;
use bonbo_journal::performance::PerformanceTracker;
use bonbo_learning::dma::DynamicModelAveraging;
use bonbo_learning::models::LearningState;
use bonbo_learning::weights::ScoringWeights;

pub struct JournalPlugin {
    metadata: PluginMetadata,
    store: Mutex<Option<JournalStore>>,
}

impl Default for JournalPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl JournalPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-journal".into(),
                name: "Trade Journal".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Trade journal with DMA learning".into(),
                author: "BonBo Team".into(),
                tags: vec!["journal".into(), "learning".into()],
            },
            store: Mutex::new(None),
        }
    }

    fn ensure_store(&self) -> anyhow::Result<()> {
        let mut store = self.store.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        if store.is_none() {
            let db_path = if let Ok(path) = std::env::var("BONBO_JOURNAL_DB") {
                std::path::PathBuf::from(path)
            } else {
                dirs::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("bonbo")
                    .join("journal.db")
            };
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            *store = Some(JournalStore::open(&db_path)?);
        }
        Ok(())
    }

    /// Load DMA, feed it all past outcomes from journal, return updated DMA.
    fn rebuild_dma_from_journal(
        &self,
        store: &JournalStore,
    ) -> anyhow::Result<DynamicModelAveraging> {
        let defaults = ScoringWeights::default();

        // Try to load saved state first
        let saved_state: Option<LearningState> = store.load_state("dma_state")?;
        let mut dma = DynamicModelAveraging::new(&defaults, 0.99, 0.99);

        if let Some(state) = saved_state {
            for model in state.models {
                if let Some(dma_model) = dma.get_mut_model(&model.name) {
                    *dma_model = model;
                }
            }
            if let Some(snap) = state.weight_history.last() {
                let weights: Vec<f64> = snap.weights.iter().map(|(_, w)| *w).collect();
                dma.set_weights(&weights);
            }
        }

        Ok(dma)
    }

    /// Save DMA state after update.
    fn save_dma_state(
        &self,
        store: &JournalStore,
        dma: &DynamicModelAveraging,
    ) -> anyhow::Result<()> {
        let models: Vec<_> = dma.get_models().values().cloned().collect();
        let history = dma.get_history().to_vec();

        let state = LearningState {
            total_updates: history.len() as u32,
            last_update_timestamp: history.last().map(|s| s.timestamp).unwrap_or(0),
            dma_alpha: 0.99,
            dma_lambda: 0.99,
            models,
            weight_history: history,
        };

        store.save_state("dma_state", &state)?;
        Ok(())
    }

    /// Derive per-indicator accuracy from entry snapshot + outcome.
    fn derive_indicator_accuracy(
        entry: &TradeJournalEntry,
        outcome: &TradeOutcome,
    ) -> std::collections::HashMap<String, bool> {
        let mut accuracy = outcome.indicator_accuracy.clone();
        let went_up = outcome.actual_return_pct > 0.0;

        // If no indicator accuracy provided, derive from snapshot signals
        if accuracy.is_empty() {
            // RSI: <30 oversold (buy), >70 overbought (sell)
            let rsi_signal = if entry.snapshot.rsi_14 < 30.0 {
                true
            }
            // predicted up
            else if entry.snapshot.rsi_14 > 70.0 {
                false
            }
            // predicted down
            else {
                went_up
            }; // neutral — count as correct for neutral
            accuracy.insert(
                "rsi".to_string(),
                rsi_signal == went_up
                    || (entry.snapshot.rsi_14 > 30.0 && entry.snapshot.rsi_14 < 70.0),
            );

            // MACD: histogram > 0 = bullish
            let macd_correct = if entry.snapshot.macd_histogram > 0.0 {
                went_up
            } else {
                !went_up
            };
            accuracy.insert("macd".to_string(), macd_correct);

            // BB: percent_b < 0.2 = oversold (buy), > 0.8 = overbought (sell)
            let bb_correct = if entry.snapshot.bb_percent_b < 0.2 {
                went_up
            } else if entry.snapshot.bb_percent_b > 0.8 {
                !went_up
            } else {
                true
            };
            accuracy.insert("bb".to_string(), bb_correct);

            // Signals count: more buy signals → should go up
            let signals_correct =
                if entry.snapshot.buy_signals_count > entry.snapshot.sell_signals_count {
                    went_up
                } else {
                    !went_up
                };
            accuracy.insert("signals".to_string(), signals_correct);

            // Sentiment: positive → should go up
            let sent_correct = if entry.snapshot.composite_sentiment > 0.0 {
                went_up
            } else {
                !went_up
            };
            accuracy.insert("sentiment".to_string(), sent_correct);

            // Backtest: positive backtest return → should go up
            let bt_correct = if entry.snapshot.backtest_return > 0.0 {
                went_up
            } else {
                !went_up
            };
            accuracy.insert("backtest".to_string(), bt_correct);

            // Momentum (EMA cross): ema12 > ema26 = bullish
            let mom_correct = if entry.snapshot.ema_12 > entry.snapshot.ema_26 {
                went_up
            } else {
                !went_up
            };
            accuracy.insert("momentum".to_string(), mom_correct);

            // Regime prediction: ranging = neutral, trending = directional
            accuracy.insert("regime".to_string(), true); // regime is contextual

            // Risk/reward: correct direction
            accuracy.insert("risk_reward".to_string(), outcome.direction_correct);
        }

        accuracy
    }
}

#[async_trait]
impl ToolPlugin for JournalPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "journal_trade_entry".into(),
                description: "Record a trade entry with analysis snapshot to the journal".into(),
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
                        name: "price".into(),
                        param_type: "number".into(),
                        description: "Current price".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "recommendation".into(),
                        param_type: "string".into(),
                        description: "STRONG_BUY/BUY/HOLD/SELL/STRONG_SELL".into(),
                        required: true,
                        default: None,
                        r#enum: Some(vec![
                            "STRONG_BUY".into(),
                            "BUY".into(),
                            "HOLD".into(),
                            "SELL".into(),
                            "STRONG_SELL".into(),
                        ]),
                    },
                    ParameterSchema {
                        name: "quant_score".into(),
                        param_type: "number".into(),
                        description: "Score 0-100".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "stop_loss".into(),
                        param_type: "number".into(),
                        description: "Stop loss price".into(),
                        required: false,
                        default: Some(json!(0)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "target_price".into(),
                        param_type: "number".into(),
                        description: "Target price".into(),
                        required: false,
                        default: Some(json!(0)),
                        r#enum: None,
                    },
                    // Optional indicator values for richer learning
                    ParameterSchema {
                        name: "rsi".into(),
                        param_type: "number".into(),
                        description: "RSI(14) value".into(),
                        required: false,
                        default: Some(json!(50)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "macd_histogram".into(),
                        param_type: "number".into(),
                        description: "MACD histogram".into(),
                        required: false,
                        default: Some(json!(0)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "bb_percent_b".into(),
                        param_type: "number".into(),
                        description: "BB %B (0-1)".into(),
                        required: false,
                        default: Some(json!(0.5)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "buy_signals".into(),
                        param_type: "number".into(),
                        description: "Buy signal count".into(),
                        required: false,
                        default: Some(json!(0)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "sell_signals".into(),
                        param_type: "number".into(),
                        description: "Sell signal count".into(),
                        required: false,
                        default: Some(json!(0)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "journal_trade_outcome".into(),
                description: "Record actual outcome — triggers DMA learning automatically".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "entry_id".into(),
                        param_type: "string".into(),
                        description: "Journal entry ID".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "exit_price".into(),
                        param_type: "number".into(),
                        description: "Actual exit price".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "direction_correct".into(),
                        param_type: "boolean".into(),
                        description: "Direction prediction correct?".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_trade_journal".into(),
                description: "Query trade journal history".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Filter symbol".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "limit".into(),
                        param_type: "number".into(),
                        description: "Max entries".into(),
                        required: false,
                        default: Some(json!(20)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_learning_metrics".into(),
                description:
                    "Learning metrics: accuracy, Sharpe, per-indicator stats from past trades"
                        .into(),
                parameters: vec![],
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        args: &Value,
        _ctx: &PluginContext,
    ) -> anyhow::Result<String> {
        self.ensure_store()?;
        let guard = self.store.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let store = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Store not initialized"))?;

        match tool_name {
            "journal_trade_entry" => {
                let symbol = args["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol required"))?;
                let price = args["price"]
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("price required"))?;
                let rec_str = args["recommendation"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("recommendation required"))?;
                let score = args["quant_score"].as_f64().unwrap_or(50.0);
                let sl = args["stop_loss"].as_f64().unwrap_or(price * 0.96);
                let tp = args["target_price"].as_f64().unwrap_or(price * 1.08);

                let recommendation = match rec_str {
                    "STRONG_BUY" => Recommendation::StrongBuy,
                    "BUY" => Recommendation::Buy,
                    "SELL" => Recommendation::Sell,
                    "STRONG_SELL" => Recommendation::StrongSell,
                    _ => Recommendation::Hold,
                };

                let snapshot = AnalysisSnapshot {
                    symbol: symbol.to_string(),
                    price,
                    quant_score: score,
                    timestamp: chrono::Utc::now().timestamp(),
                    rsi_14: args.get("rsi").and_then(|v| v.as_f64()).unwrap_or(50.0),
                    macd_histogram: args
                        .get("macd_histogram")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    bb_percent_b: args
                        .get("bb_percent_b")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5),
                    buy_signals_count: args
                        .get("buy_signals")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    sell_signals_count: args
                        .get("sell_signals")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    ..Default::default()
                };

                let rr = if sl > 0.0 {
                    (tp - price) / (price - sl)
                } else {
                    0.0
                };

                let entry = TradeJournalEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: snapshot.timestamp,
                    snapshot,
                    recommendation,
                    entry_price: price,
                    stop_loss: sl,
                    target_price: tp,
                    risk_reward_ratio: rr,
                    position_size_usd: 0.0,
                    outcome: None,
                };

                let id = entry.id.clone();
                store.insert_entry(&entry)?;

                Ok(format!(
                    "📝 **Trade Entry Recorded**\n\nID: `{}`\n{} @ ${:.2}\n{} | Score: {:.0}/100\nSL: ${:.2} | TP: ${:.2} | R:R = {:.1}\n📋 Snapshot saved with RSI={:.0} MACD_H={:.2} BB%B={:.2}",
                    id,
                    symbol,
                    price,
                    rec_str,
                    score,
                    sl,
                    tp,
                    rr,
                    entry.snapshot.rsi_14,
                    entry.snapshot.macd_histogram,
                    entry.snapshot.bb_percent_b
                ))
            }

            "journal_trade_outcome" => {
                let entry_id = args["entry_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("entry_id required"))?;
                let exit_price = args["exit_price"]
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("exit_price required"))?;
                let dir_ok = args["direction_correct"]
                    .as_bool()
                    .ok_or_else(|| anyhow::anyhow!("direction_correct required"))?;

                let entry = store.get_entry(entry_id)?;
                let ret = if entry.entry_price > 0.0 {
                    (exit_price - entry.entry_price) / entry.entry_price * 100.0
                } else {
                    0.0
                };

                let outcome = TradeOutcome {
                    close_timestamp: chrono::Utc::now().timestamp(),
                    exit_price,
                    actual_return_pct: ret,
                    hit_target: exit_price >= entry.target_price,
                    hit_stoploss: exit_price <= entry.stop_loss,
                    holding_period_hours: 0,
                    max_favorable_excursion: ret.max(0.0),
                    max_adverse_excursion: ret.min(0.0),
                    direction_correct: dir_ok,
                    score_accuracy: ret.abs(),
                    indicator_accuracy: std::collections::HashMap::new(), // Will be derived
                };

                store.record_outcome(entry_id, &outcome)?;

                // === KEY: Trigger DMA learning ===
                let indicator_accuracy = Self::derive_indicator_accuracy(&entry, &outcome);
                let regime = format!("{:?}", entry.snapshot.market_regime);

                // Rebuild DMA from persisted state
                let mut dma = self.rebuild_dma_from_journal(store)?;
                // Feed this outcome to DMA
                dma.update(&indicator_accuracy, &regime, chrono::Utc::now().timestamp())?;
                // Persist updated DMA state
                self.save_dma_state(store, &dma)?;

                let learned_weights = dma.get_weights();
                let n_updates = dma.get_history().len();

                let emoji = if dir_ok { "✅" } else { "❌" };
                let mut result = format!(
                    "{} **Trade Outcome Recorded**\n\nID: `{}`\nExit: ${:.2} | Return: {:+.2}%\nDirection: {}\n\n",
                    emoji,
                    entry_id,
                    exit_price,
                    ret,
                    if dir_ok { "Correct ✅" } else { "Wrong ❌" }
                );

                // Show what DMA learned from this outcome
                result.push_str("🧠 **DMA Learning Update:**\n");
                result.push_str(&format!("  Total updates: {}\n", n_updates));
                result.push_str("**Indicator accuracy this trade:**\n");
                let mut correct_count = 0;
                let total = indicator_accuracy.len();
                for (name, &correct) in &indicator_accuracy {
                    result.push_str(&format!(
                        "  {}: {}\n",
                        name,
                        if correct { "✅" } else { "❌" }
                    ));
                    if correct {
                        correct_count += 1;
                    }
                }
                result.push_str(&format!(
                    "\n📊 {}/{} indicators correct → weights adapted\n",
                    correct_count, total
                ));

                if n_updates >= 5 {
                    result.push_str("\n**Current learned weights:**\n");
                    for (n, w) in learned_weights.to_vec() {
                        result.push_str(&format!("  {}: {:.1}%\n", n, w * 100.0));
                    }
                }

                Ok(result)
            }

            "get_trade_journal" => {
                let mut q = JournalQuery::default();
                if let Some(s) = args.get("symbol").and_then(|v| v.as_str()) {
                    q.symbol = Some(s.to_string());
                }
                q.limit = args.get("limit").and_then(|v| v.as_u64()).map(|l| l as u32);

                let entries = store.query_entries(&q)?;
                let count = store.count_entries(&q)?;

                let mut result = format!("📋 **Trade Journal** ({} entries)\n\n", count);
                for e in &entries {
                    let o = match &e.outcome {
                        Some(x) if x.direction_correct => "✅",
                        Some(_) => "❌",
                        None => "⏳",
                    };
                    result.push_str(&format!(
                        "{} `{}` | {} @ ${:.0} | {} | Score: {:.0}\n",
                        o,
                        &e.id[..8],
                        e.snapshot.symbol,
                        e.snapshot.price,
                        e.recommendation.as_str(),
                        e.snapshot.quant_score
                    ));
                    if let Some(out) = &e.outcome {
                        result.push_str(&format!("    Return: {:+.2}%\n", out.actual_return_pct));
                    }
                }
                Ok(result)
            }

            "get_learning_metrics" => {
                let tracker = PerformanceTracker::new(store);
                let m = tracker.compute_metrics()?;

                // Also show DMA learned weights
                let dma = self.rebuild_dma_from_journal(store)?;
                let learned = dma.get_weights();
                let n_updates = dma.get_history().len();

                let mut result = format!(
                    "📊 **Learning Metrics** (from {} trades with outcomes)\n\n\
                     📝 Total Predictions: {}\n\
                     ✅ With Outcomes: {}\n\
                     🎯 Direction Accuracy: {:.1}%\n\
                     💰 Win Rate: {:.1}%\n\
                     📈 Avg Return: {:+.2}%\n\
                     📉 Sharpe (annualized): {:.2}\n\
                     💪 Profit Factor: {:.2}\n\
                     🕐 Recent 10 Accuracy: {:.1}%\n",
                    m.total_with_outcome,
                    m.total_predictions,
                    m.total_with_outcome,
                    m.direction_accuracy * 100.0,
                    m.win_rate * 100.0,
                    m.avg_return_pct,
                    m.sharpe_of_predictions,
                    m.profit_factor,
                    m.recent_10_accuracy * 100.0
                );

                if n_updates > 0 {
                    result.push_str(&format!("\n🧠 **DMA Updates:** {}\n", n_updates));
                    result.push_str("**Learned weights (adapted from data):**\n");
                    for (n, w) in learned.to_vec() {
                        result.push_str(&format!("  {}: {:.1}%\n", n, w * 100.0));
                    }
                }

                // Per-regime accuracy
                if !m.per_regime_accuracy.is_empty() {
                    result.push_str("\n📍 **Per-Regime Accuracy:**\n");
                    for ra in m.per_regime_accuracy.values() {
                        result.push_str(&format!(
                            "  {}: {:.0}% ({}/{}) avg ret {:+.2}%\n",
                            ra.regime,
                            ra.accuracy * 100.0,
                            ra.correct_direction,
                            ra.total_predictions,
                            ra.avg_return
                        ));
                    }
                }

                Ok(result)
            }

            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
