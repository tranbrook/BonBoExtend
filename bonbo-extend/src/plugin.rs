//! Plugin trait definitions for BonBo Extend.
//!
//! All plugins implement one or more of these traits:
//! - `ToolPlugin` — adds a new AI-callable tool
//! - `ServicePlugin` — adds a background service

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata about a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Unique plugin identifier (e.g., "bonbo-trade-tools").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Brief description.
    pub description: String,
    /// Author name.
    pub author: String,
    /// Tags for categorization.
    pub tags: Vec<String>,
}

/// Shared context passed to all plugins.
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// Path to BonBo data directory (~/.bonbo).
    pub bonbo_data_dir: std::path::PathBuf,
    /// Path to plugin-specific data directory.
    pub plugin_data_dir: std::path::PathBuf,
    /// Environment configuration.
    pub env_vars: HashMap<String, String>,
    /// Shared state (thread-safe).
    pub state: std::sync::Arc<tokio::sync::RwLock<serde_json::Value>>,
}

impl PluginContext {
    /// Create a new plugin context.
    pub fn new(bonbo_data_dir: std::path::PathBuf, plugin_id: &str) -> Self {
        let plugin_data_dir = bonbo_data_dir.join("plugins").join(plugin_id);
        Self {
            bonbo_data_dir,
            plugin_data_dir,
            env_vars: std::env::vars().collect(),
            state: std::sync::Arc::new(tokio::sync::RwLock::new(serde_json::Value::Null)),
        }
    }

    /// Ensure plugin data directory exists.
    pub fn ensure_data_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.plugin_data_dir)
    }

    /// Get an environment variable.
    pub fn env(&self, key: &str) -> Option<&String> {
        self.env_vars.get(key)
    }

    /// Get an environment variable or default.
    pub fn env_or(&self, key: &str, default: &str) -> String {
        self.env_vars.get(key).cloned().unwrap_or_else(|| default.to_string())
    }
}

/// JSON Schema for a tool parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    /// Parameter name.
    pub name: String,
    /// JSON Schema type ("string", "number", "boolean", etc.).
    #[serde(rename = "type")]
    pub param_type: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this parameter is required.
    #[serde(default)]
    pub required: bool,
    /// Default value (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Enum values (if constrained).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#enum: Option<Vec<String>>,
}

/// Schema definition for a tool — compatible with OpenAI/BonBo function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    /// Tool name (must be unique across all plugins).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter schemas.
    pub parameters: Vec<ParameterSchema>,
}

impl ToolSchema {
    /// Convert to OpenAI-compatible function definition JSON.
    pub fn to_function_def(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            let mut prop = serde_json::Map::new();
            prop.insert("type".to_string(), serde_json::Value::String(param.param_type.clone()));
            prop.insert("description".to_string(), serde_json::Value::String(param.description.clone()));

            if let Some(default) = &param.default {
                prop.insert("default".to_string(), default.clone());
            }

            if let Some(enum_vals) = &param.r#enum {
                let arr: Vec<serde_json::Value> = enum_vals
                    .iter()
                    .map(|v| serde_json::Value::String(v.clone()))
                    .collect();
                prop.insert("enum".to_string(), serde_json::Value::Array(arr));
            }

            properties.insert(param.name.clone(), serde_json::Value::Object(prop));

            if param.required {
                required.push(serde_json::Value::String(param.name.clone()));
            }
        }

        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        })
    }
}

/// A plugin that provides one or more AI-callable tools.
#[async_trait]
pub trait ToolPlugin: Send + Sync {
    /// Return plugin metadata.
    fn metadata(&self) -> &PluginMetadata;

    /// Return all tool schemas provided by this plugin.
    fn tools(&self) -> Vec<ToolSchema>;

    /// Execute a tool by name.
    ///
    /// # Arguments
    /// * `tool_name` — Name of the tool to execute
    /// * `arguments` — JSON string of arguments
    /// * `context` — Shared plugin context
    ///
    /// # Returns
    /// * `Ok(String)` — Tool result (text/markdown)
    /// * `Err` — Execution error
    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        context: &PluginContext,
    ) -> anyhow::Result<String>;

    /// Check if this plugin provides a tool with the given name.
    fn has_tool(&self, name: &str) -> bool {
        self.tools().iter().any(|t| t.name == name)
    }

    /// Initialize the plugin (called once at startup).
    async fn init(&self, _context: &PluginContext) -> anyhow::Result<()> {
        Ok(())
    }

    /// Shutdown the plugin (called once at exit).
    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A plugin that provides a background service.
#[async_trait]
pub trait ServicePlugin: Send + Sync {
    /// Return plugin metadata.
    fn metadata(&self) -> &PluginMetadata;

    /// Start the background service.
    /// Called once at startup.
    async fn start(&self, context: PluginContext) -> anyhow::Result<()>;

    /// Stop the background service.
    /// Called once at shutdown.
    async fn stop(&self) -> anyhow::Result<()>;

    /// Check if the service is currently running.
    fn is_running(&self) -> bool;

    /// Get service status as a human-readable string.
    fn status(&self) -> String;
}
