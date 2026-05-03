//! Alert system — notification traits and implementations for the trading agent.
//!
//! Provides:
//! - `AlertNotifier` trait — abstract interface for notifications
//! - `LogAlertNotifier` — logs alerts (always available)
//! - `WebhookAlertNotifier` — HTTP POST to webhook URL (Telegram, Discord, Slack)
//! - `MultiAlertNotifier` — fan-out to multiple notifiers

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Alert severity level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AlertLevel {
    Info,
    Warning,
    Critical,
}

/// Alert payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert level.
    pub level: AlertLevel,
    /// Alert title.
    pub title: String,
    /// Alert body (supports plain text).
    pub body: String,
    /// Optional symbol.
    pub symbol: Option<String>,
    /// Timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl Alert {
    /// Create a new info alert.
    pub fn info(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            level: AlertLevel::Info,
            title: title.into(),
            body: body.into(),
            symbol: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create a new warning alert.
    pub fn warning(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            level: AlertLevel::Warning,
            title: title.into(),
            body: body.into(),
            symbol: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create a new critical alert.
    pub fn critical(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            level: AlertLevel::Critical,
            title: title.into(),
            body: body.into(),
            symbol: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Set symbol.
    pub fn with_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.symbol = Some(symbol.into());
        self
    }

    /// Format as plain text.
    pub fn to_text(&self) -> String {
        let emoji = match self.level {
            AlertLevel::Info => "ℹ️",
            AlertLevel::Warning => "⚠️",
            AlertLevel::Critical => "🚨",
        };
        let symbol_part = self
            .symbol
            .as_ref()
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();
        format!(
            "{} {}{} — {}\n{}",
            emoji,
            self.title,
            symbol_part,
            self.timestamp.format("%H:%M:%S"),
            self.body
        )
    }
}

/// Alert notifier trait — abstract interface for sending notifications.
#[async_trait]
pub trait AlertNotifier: Send + Sync {
    /// Send an alert.
    async fn send(&self, alert: &Alert) -> anyhow::Result<()>;

    /// Get notifier name.
    fn name(&self) -> &str;
}

/// Log-based alert notifier — always available, zero dependencies.
pub struct LogAlertNotifier {
    prefix: String,
}

impl Default for LogAlertNotifier {
    fn default() -> Self {
        Self::new("bonbo-agent")
    }
}

impl LogAlertNotifier {
    /// Create a new log alert notifier.
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
        }
    }
}

#[async_trait]
impl AlertNotifier for LogAlertNotifier {
    async fn send(&self, alert: &Alert) -> anyhow::Result<()> {
        match alert.level {
            AlertLevel::Info => {
                tracing::info!("[{}] {}", self.prefix, alert.to_text());
            }
            AlertLevel::Warning => {
                tracing::warn!("[{}] {}", self.prefix, alert.to_text());
            }
            AlertLevel::Critical => {
                tracing::error!("[{}] {}", self.prefix, alert.to_text());
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "log"
    }
}

/// Webhook alert notifier — POST alerts to an HTTP endpoint.
///
/// Compatible with Telegram Bot API, Discord webhooks, Slack webhooks, etc.
pub struct WebhookAlertNotifier {
    url: String,
    client: reqwest::Client,
}

impl WebhookAlertNotifier {
    /// Create a new webhook notifier.
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl AlertNotifier for WebhookAlertNotifier {
    async fn send(&self, alert: &Alert) -> anyhow::Result<()> {
        let payload = serde_json::json!({
            "text": alert.to_text(),
            "level": format!("{:?}", alert.level),
            "title": alert.title,
            "body": alert.body,
            "symbol": alert.symbol,
            "timestamp": alert.timestamp.to_rfc3339(),
        });

        let resp = self
            .client
            .post(&self.url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Webhook returned status {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "webhook"
    }
}

/// Multi-alert notifier — fans out to multiple notifiers.
pub struct MultiAlertNotifier {
    notifiers: Vec<Box<dyn AlertNotifier>>,
}

impl MultiAlertNotifier {
    /// Create a new multi-notifier.
    pub fn new() -> Self {
        Self {
            notifiers: Vec::new(),
        }
    }

    /// Add a notifier.
    pub fn add_notifier(mut self, notifier: impl AlertNotifier + 'static) -> Self {
        self.notifiers.push(Box::new(notifier));
        self
    }

    /// Get notifier count.
    pub fn count(&self) -> usize {
        self.notifiers.len()
    }
}

impl Default for MultiAlertNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AlertNotifier for MultiAlertNotifier {
    async fn send(&self, alert: &Alert) -> anyhow::Result<()> {
        let mut errors = Vec::new();
        for notifier in &self.notifiers {
            if let Err(e) = notifier.send(alert).await {
                tracing::warn!("Failed to send alert via {}: {}", notifier.name(), e);
                errors.push(e);
            }
        }
        if errors.len() == self.notifiers.len() && !self.notifiers.is_empty() {
            return Err(anyhow::anyhow!("All {} notifiers failed", errors.len()));
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "multi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_creation() {
        let alert = Alert::info("Test", "Body text");
        assert_eq!(alert.level, AlertLevel::Info);
        assert_eq!(alert.title, "Test");
        assert!(alert.symbol.is_none());
    }

    #[test]
    fn test_alert_with_symbol() {
        let alert = Alert::warning("Price Alert", "BTC dropped 5%").with_symbol("BTCUSDT");
        assert_eq!(alert.symbol, Some("BTCUSDT".to_string()));
    }

    #[test]
    fn test_alert_text_format() {
        let alert = Alert::critical("Kill Switch", "Activated by user").with_symbol("ALL");
        let text = alert.to_text();
        assert!(text.contains("🚨"));
        assert!(text.contains("Kill Switch"));
        assert!(text.contains("[ALL]"));
    }

    #[tokio::test]
    async fn test_log_notifier() {
        let notifier = LogAlertNotifier::new("test");
        let alert = Alert::info("Test Alert", "Test body");
        let result = notifier.send(&alert).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_log_notifier_name() {
        let notifier = LogAlertNotifier::new("test");
        assert_eq!(notifier.name(), "log");
    }

    #[tokio::test]
    async fn test_multi_notifier() {
        let multi = MultiAlertNotifier::new().add_notifier(LogAlertNotifier::new("test1"));
        assert_eq!(multi.count(), 1);
        let alert = Alert::info("Test", "Multi");
        let result = multi.send(&alert).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multi_notifier_empty() {
        let multi = MultiAlertNotifier::new();
        assert_eq!(multi.count(), 0);
        let alert = Alert::info("Test", "Empty");
        let result = multi.send(&alert).await;
        assert!(result.is_ok()); // empty notifier is fine
    }
}
