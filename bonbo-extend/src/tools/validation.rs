//! Validation MCP Tools.

use crate::plugin::{ParameterSchema, PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::Value;

use bonbo_validation::cpcv::CpcvValidator;

pub struct ValidationPlugin {
    metadata: PluginMetadata,
}
impl Default for ValidationPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-validation".into(),
                name: "Strategy Validation".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "CPCV, DSR, PBO validation".into(),
                author: "BonBo Team".into(),
                tags: vec!["validation".into(), "cpcv".into()],
            },
        }
    }
}

#[async_trait]
impl ToolPlugin for ValidationPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "validate_strategy".into(),
            description: "Validate strategy via CPCV".into(),
            parameters: vec![
                ParameterSchema {
                    name: "returns".into(),
                    param_type: "array".into(),
                    description: "Returns array".into(),
                    required: true,
                    default: None,
                    r#enum: None,
                },
                ParameterSchema {
                    name: "n_groups".into(),
                    param_type: "number".into(),
                    description: "CPCV groups".into(),
                    required: false,
                    default: Some(serde_json::json!(6)),
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
            "validate_strategy" => {
                let returns: Vec<f64> = args
                    .get("returns")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
                    .ok_or_else(|| anyhow::anyhow!("returns array required"))?;
                let ng = args.get("n_groups").and_then(|v| v.as_u64()).unwrap_or(6) as usize;
                let cpcv = CpcvValidator::new(ng, 2, 2, 1);
                let r = cpcv.validate(&returns)?;
                let report =
                    bonbo_validation::report::ValidationReport::generate(&returns, 1, 0.0, 3.0)?;
                Ok(format!(
                    "🔬 **Validation**\nCPCV Sharpe: {:.2} ± {:.2} ({} combos)\nDSR: {:.3} | Haircut: {:.2} | PBO: {:.2}\nSignificant: {}",
                    r.mean_sharpe,
                    r.sharpe_std,
                    r.n_combinations,
                    report.deflated_sharpe_ratio,
                    report.haircut_sharpe,
                    report.pbo,
                    if report.is_statistically_significant {
                        "✅"
                    } else {
                        "❌"
                    }
                ))
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}
