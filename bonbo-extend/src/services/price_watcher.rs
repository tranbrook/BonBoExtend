//! Price Watcher Service — monitors crypto prices and triggers alerts.

use crate::plugin::{PluginContext, PluginMetadata, ServicePlugin};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::info;

/// A price watch entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceWatch {
    pub symbol: String,
    pub target_price: f64,
    pub direction: WatchDirection,
    pub callback_url: Option<String>,
}

/// Watch direction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WatchDirection {
    Above,
    Below,
}

/// Service that watches crypto prices.
pub struct PriceWatcherService {
    metadata: PluginMetadata,
    running: Arc<AtomicBool>,
    watches: Arc<RwLock<HashMap<String, Vec<PriceWatch>>>>,
    check_interval_secs: u64,
}

impl PriceWatcherService {
    pub fn new(check_interval_secs: u64) -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-price-watcher".to_string(),
                name: "Price Watcher Service".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Monitors crypto prices and triggers alerts".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec!["trading".to_string(), "monitoring".to_string()],
            },
            running: Arc::new(AtomicBool::new(false)),
            watches: Arc::new(RwLock::new(HashMap::new())),
            check_interval_secs,
        }
    }

    /// Add a price watch.
    pub async fn add_watch(&self, watch: PriceWatch) {
        let mut watches = self.watches.write().await;
        watches.entry(watch.symbol.clone()).or_default().push(watch);
    }

    /// Remove watches for a symbol.
    pub async fn remove_watches(&self, symbol: &str) {
        let mut watches = self.watches.write().await;
        watches.remove(symbol);
    }
}

#[async_trait]
impl ServicePlugin for PriceWatcherService {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    async fn start(&self, _context: PluginContext) -> anyhow::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let watches = self.watches.clone();
        let interval = self.check_interval_secs;

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                let watch_snapshot = watches.read().await.clone();
                if !watch_snapshot.is_empty() {
                    for symbol in watch_snapshot.keys() {
                        // Check current price against watches
                        if let Ok(current_price) = fetch_current_price(symbol).await {
                            let watches_guard = watches.read().await;
                            if let Some(wlist) = watches_guard.get(symbol) {
                                for watch in wlist {
                                    let triggered = match &watch.direction {
                                        WatchDirection::Above => {
                                            current_price >= watch.target_price
                                        }
                                        WatchDirection::Below => {
                                            current_price <= watch.target_price
                                        }
                                    };
                                    if triggered {
                                        info!(
                                            "🔔 ALERT: {} {} ${:.2} (current: ${:.2})",
                                            symbol,
                                            match &watch.direction {
                                                WatchDirection::Above => "ABOVE",
                                                WatchDirection::Below => "BELOW",
                                            },
                                            watch.target_price,
                                            current_price
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
            info!("Price watcher service stopped");
        });

        info!(
            "Price watcher service started (interval: {}s)",
            self.check_interval_secs
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn status(&self) -> String {
        "Price watcher service running".to_string()
    }
}

/// Fetch current price from Binance.
async fn fetch_current_price(symbol: &str) -> anyhow::Result<f64> {
    let url = format!(
        "https://api.binance.com/api/v3/ticker/price?symbol={}",
        symbol.to_uppercase()
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;
    let data: serde_json::Value = resp.json().await?;
    data["price"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Failed to parse price"))
}
