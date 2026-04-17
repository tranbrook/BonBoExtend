//! System Monitor Plugin — system health and resource monitoring.

use async_trait::async_trait;
use crate::plugin::*;

/// Plugin for system monitoring tools.
pub struct SystemMonitorPlugin {
    metadata: PluginMetadata,
}

impl SystemMonitorPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-system-monitor".to_string(),
                name: "System Monitor Tools".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "System health: CPU, memory, disk, processes".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec!["system".to_string(), "monitoring".to_string()],
            },
        }
    }
}

impl Default for SystemMonitorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolPlugin for SystemMonitorPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "system_status".to_string(),
                description: "Get system status: CPU usage, memory usage, disk space, uptime, and load average.".to_string(),
                parameters: vec![],
            },
            ToolSchema {
                name: "check_port".to_string(),
                description: "Check if a network port is open and listening.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "port".to_string(),
                        param_type: "integer".to_string(),
                        description: "Port number to check".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "host".to_string(),
                        param_type: "string".to_string(),
                        description: "Host to check (default: 127.0.0.1)".to_string(),
                        required: false,
                        default: Some(serde_json::Value::String("127.0.0.1".to_string())),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "disk_usage".to_string(),
                description: "Get disk usage information for all mounted filesystems.".to_string(),
                parameters: vec![],
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
            "system_status" => get_system_status().await,
            "check_port" => {
                let port = arguments["port"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("port is required"))? as u16;
                let host = arguments["host"]
                    .as_str()
                    .unwrap_or("127.0.0.1");
                check_port(host, port).await
            }
            "disk_usage" => get_disk_usage().await,
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }
}

async fn get_system_status() -> anyhow::Result<String> {
    let mut lines = vec!["🖥️ **System Status**\n".to_string()];

    // Uptime
    if let Ok(output) = tokio::process::Command::new("uptime").output().await {
        let uptime = String::from_utf8_lossy(&output.stdout);
        lines.push(format!("⏱️ Uptime: {}", uptime.trim()));
    }

    // Memory
    if let Ok(output) = tokio::process::Command::new("free").args(["-h"]).output().await {
        let mem = String::from_utf8_lossy(&output.stdout);
        for line in mem.lines().take(2) {
            lines.push(line.to_string());
        }
    }

    // Load
    if let Ok(load) = std::fs::read_to_string("/proc/loadavg") {
        lines.push(format!("⚡ Load: {}", load.trim()));
    }

    // CPU info
    if let Ok(output) = tokio::process::Command::new("nproc").output().await {
        let cpus = String::from_utf8_lossy(&output.stdout);
        lines.push(format!("🔧 CPUs: {}", cpus.trim()));
    }

    Ok(lines.join("\n"))
}

async fn check_port(host: &str, port: u16) -> anyhow::Result<String> {
    use tokio::net::TcpStream;
    use std::time::Duration;

    let addr = format!("{}:{}", host, port);
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        TcpStream::connect(&addr),
    ).await;

    match result {
        Ok(Ok(_)) => Ok(format!("✅ Port {} on {} is OPEN", port, host)),
        Ok(Err(e)) => Ok(format!("❌ Port {} on {} is CLOSED ({})", port, host, e.kind())),
        Err(_) => Ok(format!("⏱️ Port {} on {} — TIMEOUT", port, host)),
    }
}

async fn get_disk_usage() -> anyhow::Result<String> {
    let output = tokio::process::Command::new("df")
        .args(["-h", "-x", "squashfs", "-x", "tmpfs", "-x", "devtmpfs"])
        .output()
        .await?;

    let df = String::from_utf8_lossy(&output.stdout);
    let mut lines = vec!["💾 **Disk Usage**\n".to_string()];
    for line in df.lines() {
        lines.push(line.to_string());
    }
    Ok(lines.join("\n"))
}
