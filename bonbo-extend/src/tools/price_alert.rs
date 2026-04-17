//! Price Alert Plugin — create and manage price alerts for crypto.

use async_trait::async_trait;
use crate::plugin::*;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory price alert store.
type AlertStore = Arc<RwLock<Vec<PriceAlert>>>;

/// A single price alert.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceAlert {
    pub id: String,
    pub symbol: String,
    pub target_price: f64,
    pub direction: AlertDirection,
    pub created_at: String,
    pub triggered: bool,
}

/// Alert direction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AlertDirection {
    Above,
    Below,
}

/// Plugin for managing price alerts.
pub struct PriceAlertPlugin {
    metadata: PluginMetadata,
    alerts: AlertStore,
}

impl PriceAlertPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-price-alert".to_string(),
                name: "Price Alert Tools".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Create and manage crypto price alerts".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec!["trading".to_string(), "alert".to_string()],
            },
            alerts: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for PriceAlertPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolPlugin for PriceAlertPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "create_price_alert".to_string(),
                description: "Create a price alert for a cryptocurrency. Notifies when price crosses the target.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair (e.g., BTCUSDT)".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "target_price".to_string(),
                        param_type: "number".to_string(),
                        description: "Target price".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "direction".to_string(),
                        param_type: "string".to_string(),
                        description: "Alert when price goes above or below target".to_string(),
                        required: false,
                        default: Some(serde_json::Value::String("above".to_string())),
                        r#enum: Some(vec!["above".to_string(), "below".to_string()]),
                    },
                ],
            },
            ToolSchema {
                name: "list_price_alerts".to_string(),
                description: "List all active price alerts.".to_string(),
                parameters: vec![],
            },
            ToolSchema {
                name: "delete_price_alert".to_string(),
                description: "Delete a price alert by ID.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "alert_id".to_string(),
                        param_type: "string".to_string(),
                        description: "Alert ID to delete".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        _context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "create_price_alert" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let target_price = arguments["target_price"]
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("target_price is required"))?;
                let direction = match arguments["direction"].as_str().unwrap_or("above") {
                    "below" => AlertDirection::Below,
                    _ => AlertDirection::Above,
                };

                let alert = PriceAlert {
                    id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
                    symbol: symbol.to_uppercase(),
                    target_price,
                    direction,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    triggered: false,
                };

                let summary = format!(
                    "✅ Alert #{}: {} {} ${:.2}",
                    alert.id, alert.symbol,
                    match &alert.direction {
                        AlertDirection::Above => "goes ABOVE",
                        AlertDirection::Below => "goes BELOW",
                    },
                    alert.target_price
                );

                self.alerts.write().await.push(alert);
                Ok(summary)
            }
            "list_price_alerts" => {
                let alerts = self.alerts.read().await;
                if alerts.is_empty() {
                    return Ok("📋 No active alerts.".to_string());
                }
                let mut lines = vec![format!("📋 **{} Active Alert(s)**\n", alerts.len())];
                for alert in alerts.iter() {
                    let dir_emoji = match &alert.direction {
                        AlertDirection::Above => "📈",
                        AlertDirection::Below => "📉",
                    };
                    let status = if alert.triggered { "🔔 TRIGGERED" } else { "⏳ Active" };
                    lines.push(format!(
                        "- #{} {} {} ${:.2} — {} — {}",
                        alert.id, dir_emoji, alert.symbol,
                        alert.target_price,
                        match &alert.direction {
                            AlertDirection::Above => "above",
                            AlertDirection::Below => "below",
                        },
                        status
                    ));
                }
                Ok(lines.join("\n"))
            }
            "delete_price_alert" => {
                let alert_id = arguments["alert_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("alert_id is required"))?;
                let mut alerts = self.alerts.write().await;
                let before = alerts.len();
                alerts.retain(|a| a.id != alert_id);
                if alerts.len() < before {
                    Ok(format!("✅ Alert #{} deleted.", alert_id))
                } else {
                    Ok(format!("❌ Alert #{} not found.", alert_id))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }
}
