//! Learning MCP Tools.

use crate::plugin::{PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::Value;

use bonbo_learning::dma::DynamicModelAveraging;
use bonbo_learning::weights::ScoringWeights;

pub struct LearningPlugin { metadata: PluginMetadata }
impl LearningPlugin {
    pub fn new() -> Self {
        Self { metadata: PluginMetadata {
            id: "bonbo-learning".into(), name: "Learning Engine".into(),
            version: env!("CARGO_PKG_VERSION").into(), description: "DMA weight adaptation".into(),
            author: "BonBo Team".into(), tags: vec!["learning".into(), "dma".into()],
        }}
    }
}

#[async_trait]
impl ToolPlugin for LearningPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "get_scoring_weights".into(), description: "Current scoring weights".into(), parameters: vec![] },
            ToolSchema { name: "get_learning_stats".into(), description: "DMA learning stats".into(), parameters: vec![] },
        ]
    }
    async fn execute_tool(&self, tool_name: &str, _args: &Value, _ctx: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "get_scoring_weights" => {
                let d = ScoringWeights::default();
                let fw = |w: &ScoringWeights| -> String { w.to_vec().iter().map(|(n,v)| format!("  {}: {:.1}%", n, v*100.0)).collect::<Vec<_>>().join("\n") };
                Ok(format!("⚖️ **Weights**\n\nDefault:\n{}\n\nTrending:\n{}\n\nRanging:\n{}\n\nVolatile:\n{}",
                    fw(&d), fw(&ScoringWeights::for_regime("TrendingUp")), fw(&ScoringWeights::for_regime("Ranging")), fw(&ScoringWeights::for_regime("Volatile"))))
            }
            "get_learning_stats" => {
                let dma = DynamicModelAveraging::new(&ScoringWeights::default(), 0.99, 0.99);
                let models = dma.get_models();
                let mut r = "🧠 **DMA Learning Stats**\n\n".to_string();
                for (_, m) in models { r.push_str(&format!("{}: acc={:.1}% preds={}\n", m.name, m.accuracy()*100.0, m.predictions)); }
                r.push_str("\nα=0.99 λ=0.99 min=3% max_change=5%");
                Ok(r)
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
