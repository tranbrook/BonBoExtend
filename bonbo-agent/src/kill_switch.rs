//! Kill switch — emergency stop via file or Telegram command.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Kill switch state.
#[derive(Debug, Clone)]
pub struct KillSwitch {
    /// Path to kill file.
    kill_file: PathBuf,
    /// Whether the kill switch is active.
    activated: Arc<RwLock<bool>>,
}

impl KillSwitch {
    /// Create a new kill switch.
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self {
            kill_file: data_dir.join("kill_switch.flag"),
            activated: Arc::new(RwLock::new(false)),
        }
    }

    /// Check if kill switch is activated.
    pub async fn is_activated(&self) -> bool {
        // Check file-based kill switch
        if self.kill_file.exists() {
            return true;
        }
        // Check in-memory flag
        *self.activated.read().await
    }

    /// Activate the kill switch.
    pub async fn activate(&self) -> anyhow::Result<()> {
        *self.activated.write().await = true;
        std::fs::write(&self.kill_file, "KILLED")?;
        tracing::error!("🛑 KILL SWITCH ACTIVATED — all trading stopped");
        Ok(())
    }

    /// Deactivate the kill switch.
    pub async fn deactivate(&self) -> anyhow::Result<()> {
        *self.activated.write().await = false;
        if self.kill_file.exists() {
            std::fs::remove_file(&self.kill_file)?;
        }
        tracing::info!("✅ Kill switch deactivated — trading resumed");
        Ok(())
    }

    /// Get kill file path.
    pub fn kill_file_path(&self) -> &std::path::Path {
        &self.kill_file
    }
}
