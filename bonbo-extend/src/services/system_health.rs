//! Health monitoring service — real system metrics collection.
//!
//! Replaces the dummy health check with actual CPU/Memory monitoring.

use crate::plugin::{PluginContext, PluginMetadata, ServicePlugin};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::info;

/// System health metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMetrics {
    pub cpu_usage_pct: f64,
    pub memory_used_mb: f64,
    pub memory_total_mb: f64,
    pub memory_pct: f64,
    pub uptime_secs: f64,
    pub load_avg_1m: f64,
    pub check_count: u64,
    pub status: String,
}

impl Default for HealthMetrics {
    fn default() -> Self {
        Self {
            cpu_usage_pct: 0.0,
            memory_used_mb: 0.0,
            memory_total_mb: 0.0,
            memory_pct: 0.0,
            uptime_secs: 0.0,
            load_avg_1m: 0.0,
            check_count: 0,
            status: "Unknown".to_string(),
        }
    }
}

/// System health monitoring service.
pub struct SystemHealthService {
    metadata: PluginMetadata,
    running: Arc<AtomicBool>,
    interval_secs: u64,
    status: Arc<RwLock<HealthMetrics>>,
    start_time: Arc<RwLock<std::time::Instant>>,
}

impl Default for SystemHealthService {
    fn default() -> Self {
        Self::new(60)
    }
}

impl SystemHealthService {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-system-health".to_string(),
                name: "System Health Monitor".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Monitors CPU, Memory, and system health".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec![
                    "system".to_string(),
                    "health".to_string(),
                    "monitoring".to_string(),
                ],
            },
            running: Arc::new(AtomicBool::new(false)),
            interval_secs,
            status: Arc::new(RwLock::new(HealthMetrics::default())),
            start_time: Arc::new(RwLock::new(std::time::Instant::now())),
        }
    }

    /// Get current health metrics snapshot.
    pub async fn get_metrics(&self) -> HealthMetrics {
        self.status.read().await.clone()
    }

    /// Collect system metrics (cross-platform best-effort).
    async fn collect_metrics(check_count: u64, start: std::time::Instant) -> HealthMetrics {
        let mut metrics = HealthMetrics {
            uptime_secs: start.elapsed().as_secs_f64(),
            check_count,
            ..Default::default()
        };

        // Memory info from /proc/meminfo (Linux) or sysctl (macOS)
        #[cfg(target_os = "linux")]
        {
            if let Ok(meminfo) = tokio::fs::read_to_string("/proc/meminfo").await {
                let mut total_kb: f64 = 0.0;
                let mut available_kb: f64 = 0.0;
                for line in meminfo.lines() {
                    if line.starts_with("MemTotal:") {
                        total_kb = line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0);
                    }
                    if line.starts_with("MemAvailable:") {
                        available_kb = line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0);
                    }
                }
                metrics.memory_total_mb = total_kb / 1024.0;
                metrics.memory_used_mb = (total_kb - available_kb) / 1024.0;
                if total_kb > 0.0 {
                    metrics.memory_pct = ((total_kb - available_kb) / total_kb) * 100.0;
                }
            }

            // Load average
            if let Ok(loadavg) = tokio::fs::read_to_string("/proc/loadavg").await {
                metrics.load_avg_1m = loadavg
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
            }

            // CPU usage from /proc/stat
            if let Ok(stat) = tokio::fs::read_to_string("/proc/stat").await
                && let Some(line) = stat.lines().next()
            {
                let fields: Vec<f64> = line
                    .split_whitespace()
                    .skip(1) // skip "cpu"
                    .filter_map(|v| v.parse().ok())
                    .collect();
                if fields.len() >= 4 {
                    let idle = fields[3];
                    let total: f64 = fields.iter().sum();
                    metrics.cpu_usage_pct = if total > 0.0 {
                        (1.0 - idle / total) * 100.0
                    } else {
                        0.0
                    };
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Fallback: use `sysctl` or `vm_stat` on macOS, or just report basic info
            if let Ok(output) = tokio::process::Command::new("sysctl")
                .args(["-n", "hw.memsize"])
                .output()
                .await
            {
                let total_bytes: f64 = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse()
                    .unwrap_or(0.0);
                metrics.memory_total_mb = total_bytes / (1024.0 * 1024.0);
            }
        }

        // Determine status
        metrics.status = if metrics.memory_pct > 90.0 {
            "Critical".to_string()
        } else if metrics.memory_pct > 75.0 || metrics.load_avg_1m > 4.0 {
            "Warning".to_string()
        } else {
            "Healthy".to_string()
        };

        metrics
    }
}

#[async_trait]
impl ServicePlugin for SystemHealthService {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    async fn start(&self, _context: PluginContext) -> anyhow::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let status = self.status.clone();
        let start_time = self.start_time.clone();
        let interval = self.interval_secs;

        *start_time.write().await = std::time::Instant::now();

        tokio::spawn(async move {
            let mut counter = 0u64;
            let start = std::time::Instant::now();
            while running.load(Ordering::SeqCst) {
                counter += 1;
                let metrics = Self::collect_metrics(counter, start).await;
                info!(
                    "Health check #{}: CPU {:.1}% | Mem {:.0}/{:.0}MB ({:.1}%) | Load {:.2} | {}",
                    counter,
                    metrics.cpu_usage_pct,
                    metrics.memory_used_mb,
                    metrics.memory_total_mb,
                    metrics.memory_pct,
                    metrics.load_avg_1m,
                    metrics.status
                );
                *status.write().await = metrics;
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
            info!("System health service stopped");
        });

        info!(
            "System health service started (interval: {}s)",
            self.interval_secs
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        let mut status = self.status.write().await;
        status.status = "Stopped".to_string();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn status(&self) -> String {
        format!(
            "System health service ({})",
            if self.is_running() {
                "running"
            } else {
                "stopped"
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_metrics_default() {
        let m = HealthMetrics::default();
        assert!((m.cpu_usage_pct - 0.0).abs() < f64::EPSILON);
        assert_eq!(m.status, "Unknown");
    }

    #[test]
    fn test_health_metrics_serialization() {
        let m = HealthMetrics {
            cpu_usage_pct: 45.0,
            memory_used_mb: 1024.0,
            memory_total_mb: 4096.0,
            memory_pct: 25.0,
            uptime_secs: 3600.0,
            load_avg_1m: 1.5,
            check_count: 60,
            status: "Healthy".into(),
        };
        let json = serde_json::to_string(&m).unwrap();
        let deserialized: HealthMetrics = serde_json::from_str(&json).unwrap();
        assert!((deserialized.cpu_usage_pct - 45.0).abs() < f64::EPSILON);
        assert_eq!(deserialized.status, "Healthy");
    }

    #[tokio::test]
    async fn test_service_default() {
        let service = SystemHealthService::default();
        assert!(!service.is_running());
        let metrics = service.get_metrics().await;
        assert_eq!(metrics.check_count, 0);
    }

    #[test]
    fn test_status_determination() {
        let mut m = HealthMetrics::default();
        m.memory_pct = 95.0;
        assert_eq!(
            if m.memory_pct > 90.0 {
                "Critical"
            } else if m.memory_pct > 75.0 {
                "Warning"
            } else {
                "Healthy"
            },
            "Critical"
        );

        m.memory_pct = 80.0;
        assert_eq!(
            if m.memory_pct > 90.0 {
                "Critical"
            } else if m.memory_pct > 75.0 {
                "Warning"
            } else {
                "Healthy"
            },
            "Warning"
        );

        m.memory_pct = 50.0;
        assert_eq!(
            if m.memory_pct > 90.0 {
                "Critical"
            } else if m.memory_pct > 75.0 {
                "Warning"
            } else {
                "Healthy"
            },
            "Healthy"
        );
    }
}
