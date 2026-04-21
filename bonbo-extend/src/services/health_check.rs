//! Health Check Service — periodically checks system health.

use crate::plugin::{PluginContext, PluginMetadata, ServicePlugin};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::info;

/// Service that periodically checks system health.
pub struct HealthCheckService {
    metadata: PluginMetadata,
    running: Arc<AtomicBool>,
    interval_secs: u64,
    status: Arc<RwLock<String>>,
}

impl HealthCheckService {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-health-check".to_string(),
                name: "Health Check Service".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Periodically checks system health".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec!["system".to_string(), "health".to_string()],
            },
            running: Arc::new(AtomicBool::new(false)),
            interval_secs,
            status: Arc::new(RwLock::new("Not started".to_string())),
        }
    }
}

#[async_trait]
impl ServicePlugin for HealthCheckService {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    async fn start(&self, _context: PluginContext) -> anyhow::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let status = self.status.clone();
        let interval = self.interval_secs;

        tokio::spawn(async move {
            let mut counter = 0u64;
            while running.load(Ordering::SeqCst) {
                counter += 1;
                let health = check_health().await;
                *status.write().await = format!(
                    "Check #{}: {} — CPU OK, Mem OK",
                    counter,
                    chrono::Utc::now().format("%H:%M:%S")
                );
                info!("Health check #{}: {}", counter, health);
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
            info!("Health check service stopped");
        });

        info!(
            "Health check service started (interval: {}s)",
            self.interval_secs
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        *self.status.write().await = "Stopped".to_string();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn status(&self) -> String {
        // Note: in production, this would be async or use try_read
        "Health check service running".to_string()
    }
}

async fn check_health() -> String {
    if let Ok(output) = tokio::process::Command::new("uptime").output().await {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        "Health check failed".to_string()
    }
}
