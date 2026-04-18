//! Learning MCP Tools — DMA engine with persisted state.

use crate::plugin::{PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Mutex;

use bonbo_journal::journal::JournalStore;
use bonbo_learning::dma::DynamicModelAveraging;
use bonbo_learning::models::LearningState;
use bonbo_learning::weights::ScoringWeights;

pub struct LearningPlugin {
    metadata: PluginMetadata,
    store: Mutex<Option<JournalStore>>,
}

impl LearningPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-learning".into(),
                name: "Learning Engine".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "DMA weight adaptation with persisted state".into(),
                author: "BonBo Team".into(),
                tags: vec!["learning".into(), "dma".into()],
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

    /// Load or create DMA engine, restoring state from journal.
    fn load_dma(&self) -> anyhow::Result<DynamicModelAveraging> {
        let guard = self.store.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let store = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Store not initialized"))?;

        let state: Option<LearningState> = store.load_state("dma_state")?;

        let defaults = ScoringWeights::default();
        let mut dma = DynamicModelAveraging::new(&defaults, 0.99, 0.99);

        if let Some(s) = state {
            // Restore models from saved state
            for model in s.models {
                if let Some(dma_model) = dma.get_mut_model(&model.name) {
                    *dma_model = model;
                }
            }
            // Restore weights from last snapshot
            if let Some(snap) = s.weight_history.last() {
                let weights: Vec<f64> = snap.weights.iter().map(|(_, w)| *w).collect();
                dma.set_weights(&weights);
            }
        }

        Ok(dma)
    }

    /// Persist DMA state to journal.
    fn save_dma(&self, dma: &DynamicModelAveraging) -> anyhow::Result<()> {
        let guard = self.store.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let store = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Store not initialized"))?;

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
}

#[async_trait]
impl ToolPlugin for LearningPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "get_scoring_weights".into(), description: "Current learned scoring weights (adapted from past outcomes)".into(), parameters: vec![] },
            ToolSchema { name: "get_learning_stats".into(), description: "DMA learning stats — shows what the engine has learned".into(), parameters: vec![] },
            ToolSchema { name: "reset_learning".into(), description: "Reset DMA learning back to defaults".into(), parameters: vec![] },
        ]
    }

    async fn execute_tool(&self, tool_name: &str, _args: &Value, _ctx: &PluginContext) -> anyhow::Result<String> {
        self.ensure_store()?;

        match tool_name {
            "get_scoring_weights" => {
                let dma = self.load_dma()?;
                let learned = dma.get_weights();
                let defaults = ScoringWeights::default();

                let fw = |w: &ScoringWeights| -> String {
                    w.to_vec().iter().map(|(n, v)| format!("  {}: {:.1}%", n, v * 100.0)).collect::<Vec<_>>().join("\n")
                };

                let mut result = "⚖️ **Scoring Weights**\n\n".to_string();

                result.push_str(&format!("🧠 **Learned (adapted from past trades):**\n{}\n", fw(&learned)));

                // Show diff
                let learned_pairs = learned.to_vec();
                let default_pairs = defaults.to_vec();
                result.push_str("\n📊 **Change from defaults:**\n");
                for ((n, lv), (_, dv)) in learned_pairs.iter().zip(default_pairs.iter()) {
                    let diff = (lv - dv) * 100.0;
                    let arrow = if diff.abs() < 0.1 { "→" } else if diff > 0.0 { "↑" } else { "↓" };
                    result.push_str(&format!("  {} {} {:+.1}% ({:.1}% → {:.1}%)\n",
                        arrow, n, diff, dv * 100.0, lv * 100.0));
                }

                // Also show regime-specific weights
                for regime in &["TrendingUp", "Ranging", "Volatile"] {
                    result.push_str(&format!("\n🎯 **{} Regime:**\n{}\n", regime, fw(&ScoringWeights::for_regime(regime))));
                }

                Ok(result)
            }

            "get_learning_stats" => {
                let dma = self.load_dma()?;
                let models = dma.get_models();
                let history = dma.get_history();
                let total_updates = history.len();

                let mut result = "🧠 **DMA Learning Stats**\n\n".to_string();

                result.push_str(&format!("📝 Total weight updates: {}\n", total_updates));
                result.push_str(&format!("🔄 Should revert to defaults: {}\n\n",
                    if dma.should_revert_to_defaults() { "⚠️ YES — accuracy too low" } else { "✅ No" }));

                result.push_str("**Indicator Models:**\n");
                for (_, m) in models {
                    let acc_bar: String = "█".repeat((m.accuracy() * 20.0) as usize)
                        + &"░".repeat(20 - (m.accuracy() * 20.0) as usize);
                    result.push_str(&format!("  {} [{}] {:.1}% ({} predictions)\n",
                        m.name, acc_bar, m.accuracy() * 100.0, m.predictions));
                }

                result.push_str("\n**DMA Parameters:**\n");
                result.push_str("  α (parameter forgetting): 0.99\n");
                result.push_str("  λ (weight forgetting): 0.99\n");
                result.push_str("  Min weight: 3% | Max change: 5%\n");

                if total_updates > 0 {
                    let first = &history[0];
                    let last = &history[total_updates - 1];
                    result.push_str(&format!("\n**History:** {} → {} ({} updates)\n",
                        first.regime, last.regime, total_updates));
                }

                Ok(result)
            }

            "reset_learning" => {
                let mut dma = self.load_dma()?;
                dma.reset(&ScoringWeights::default());
                self.save_dma(&dma)?;
                Ok("🔄 **Learning Reset**\n\nAll weights reverted to defaults.\nDMA models cleared.\nStart fresh with new trades.".to_string())
            }

            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
