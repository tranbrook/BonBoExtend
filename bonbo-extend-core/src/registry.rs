//! Plugin registry — manages all registered plugins.

use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::error::{ExtendError, ExtendResult};
use crate::plugin::{PluginContext, PluginMetadata, ToolPlugin, ToolSchema};

/// Registry of all loaded plugins.
pub struct PluginRegistry {
    /// Tool plugins, keyed by plugin ID.
    tool_plugins: HashMap<String, Arc<dyn ToolPlugin>>,
    /// Fast lookup: tool name → plugin ID.
    tool_to_plugin: HashMap<String, String>,
    /// Plugin contexts, keyed by plugin ID.
    contexts: HashMap<String, PluginContext>,
    /// Path to BonBo data directory.
    bonbo_data_dir: std::path::PathBuf,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        let bonbo_data_dir = dirs_data_dir();
        Self {
            tool_plugins: HashMap::new(),
            tool_to_plugin: HashMap::new(),
            contexts: HashMap::new(),
            bonbo_data_dir,
        }
    }

    /// Create registry with custom data directory (for testing).
    pub fn with_data_dir(data_dir: std::path::PathBuf) -> Self {
        Self {
            tool_plugins: HashMap::new(),
            tool_to_plugin: HashMap::new(),
            contexts: HashMap::new(),
            bonbo_data_dir: data_dir,
        }
    }

    /// Register a tool plugin.
    pub fn register_tool_plugin(&mut self, plugin: impl ToolPlugin + 'static) -> ExtendResult<()> {
        let metadata = plugin.metadata();
        let plugin_id = metadata.id.clone();

        if self.tool_plugins.contains_key(&plugin_id) {
            return Err(ExtendError::PluginAlreadyRegistered(plugin_id));
        }

        // Register all tools from this plugin.
        for tool in plugin.tools() {
            if let Some(existing) = self.tool_to_plugin.get(&tool.name) {
                warn!(
                    "Tool '{}' already registered by plugin '{}', skipping from '{}'",
                    tool.name, existing, plugin_id
                );
                continue;
            }
            debug!(
                "Registered tool '{}' from plugin '{}'",
                tool.name, plugin_id
            );
            self.tool_to_plugin
                .insert(tool.name.clone(), plugin_id.clone());
        }

        // Create context for this plugin.
        let context = PluginContext::new(self.bonbo_data_dir.clone(), &plugin_id);
        self.contexts.insert(plugin_id.clone(), context);

        info!(
            "Registered tool plugin: {} v{}",
            metadata.name, metadata.version
        );
        self.tool_plugins.insert(plugin_id, Arc::new(plugin));
        Ok(())
    }

    /// Initialize all registered plugins.
    pub async fn init_all(&self) -> ExtendResult<()> {
        for (id, plugin) in &self.tool_plugins {
            if let Some(context) = self.contexts.get(id) {
                if let Err(e) = plugin.init(context).await {
                    warn!("Failed to init plugin '{}': {}", id, e);
                } else {
                    info!("Initialized plugin: {}", id);
                }
            }
        }
        Ok(())
    }

    /// Shutdown all registered plugins.
    pub async fn shutdown_all(&self) -> ExtendResult<()> {
        for (id, plugin) in &self.tool_plugins {
            if let Err(e) = plugin.shutdown().await {
                warn!("Failed to shutdown plugin '{}': {}", id, e);
            }
        }
        Ok(())
    }

    /// Execute a tool by name.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> ExtendResult<String> {
        let plugin_id = self
            .tool_to_plugin
            .get(tool_name)
            .ok_or_else(|| {
                ExtendError::PluginNotFound(format!("No plugin provides tool: {}", tool_name))
            })?
            .clone();

        let plugin = self
            .tool_plugins
            .get(&plugin_id)
            .ok_or_else(|| ExtendError::PluginNotFound(plugin_id.clone()))?;

        let context = self
            .contexts
            .get(&plugin_id)
            .ok_or_else(|| ExtendError::PluginNotFound(format!("No context for: {}", plugin_id)))?;

        debug!("Executing tool '{}' via plugin '{}'", tool_name, plugin_id);
        plugin
            .execute_tool(tool_name, arguments, context)
            .await
            .map_err(|e| ExtendError::ToolExecutionFailed(e.to_string()))
    }

    /// Check if a tool is available.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tool_to_plugin.contains_key(name)
    }

    /// Get all tool schemas from all plugins.
    pub fn all_tool_schemas(&self) -> Vec<ToolSchema> {
        let mut schemas = Vec::new();
        for plugin in self.tool_plugins.values() {
            schemas.extend(plugin.tools());
        }
        schemas
    }

    /// Get tool schemas as OpenAI-compatible function definitions.
    pub fn all_function_defs(&self) -> Vec<serde_json::Value> {
        self.all_tool_schemas()
            .iter()
            .map(|s| s.to_function_def())
            .collect()
    }

    /// List all registered plugins.
    pub fn list_plugins(&self) -> Vec<&PluginMetadata> {
        self.tool_plugins.values().map(|p| p.metadata()).collect()
    }

    /// Get total tool count.
    pub fn tool_count(&self) -> usize {
        self.tool_to_plugin.len()
    }

    /// Get plugin count.
    pub fn plugin_count(&self) -> usize {
        self.tool_plugins.len()
    }
}

/// Get the default BonBo data directory.
fn dirs_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("bonbo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginMetadata;
    use async_trait::async_trait;

    struct MockPlugin {
        metadata: PluginMetadata,
        tools: Vec<ToolSchema>,
    }

    impl MockPlugin {
        fn new(id: &str, tool_names: &[&str]) -> Self {
            let tools = tool_names
                .iter()
                .map(|&name| ToolSchema {
                    name: name.to_string(),
                    description: format!("Mock tool: {}", name),
                    parameters: vec![],
                })
                .collect();

            Self {
                metadata: PluginMetadata {
                    id: id.to_string(),
                    name: format!("Mock Plugin ({})", id),
                    version: "0.1.0".to_string(),
                    description: "A mock plugin for testing".to_string(),
                    author: "Test".to_string(),
                    tags: vec!["test".to_string()],
                },
                tools,
            }
        }
    }

    #[async_trait]
    impl ToolPlugin for MockPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }

        fn tools(&self) -> Vec<ToolSchema> {
            self.tools.clone()
        }

        async fn execute_tool(
            &self,
            tool_name: &str,
            _arguments: &serde_json::Value,
            _context: &PluginContext,
        ) -> anyhow::Result<String> {
            Ok(format!("Executed: {}", tool_name))
        }
    }

    #[test]
    fn test_register_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = MockPlugin::new("test-plugin", &["tool_a", "tool_b"]);
        assert!(registry.register_tool_plugin(plugin).is_ok());
        assert_eq!(registry.plugin_count(), 1);
        assert_eq!(registry.tool_count(), 2);
    }

    #[test]
    fn test_duplicate_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin1 = MockPlugin::new("test-plugin", &["tool_a"]);
        let plugin2 = MockPlugin::new("test-plugin", &["tool_b"]);
        assert!(registry.register_tool_plugin(plugin1).is_ok());
        assert!(registry.register_tool_plugin(plugin2).is_err());
    }

    #[test]
    fn test_has_tool() {
        let mut registry = PluginRegistry::new();
        let plugin = MockPlugin::new("test-plugin", &["tool_a", "tool_b"]);
        registry.register_tool_plugin(plugin).unwrap();
        assert!(registry.has_tool("tool_a"));
        assert!(registry.has_tool("tool_b"));
        assert!(!registry.has_tool("tool_c"));
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let mut registry = PluginRegistry::new();
        let plugin = MockPlugin::new("test-plugin", &["hello"]);
        registry.register_tool_plugin(plugin).unwrap();
        let result = registry.execute_tool("hello", &serde_json::json!({})).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Executed: hello");
    }

    #[tokio::test]
    async fn test_execute_unknown_tool() {
        let registry = PluginRegistry::new();
        let result = registry
            .execute_tool("unknown", &serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_all_tool_schemas() {
        let mut registry = PluginRegistry::new();
        let plugin1 = MockPlugin::new("p1", &["tool_a"]);
        let plugin2 = MockPlugin::new("p2", &["tool_b"]);
        registry.register_tool_plugin(plugin1).unwrap();
        registry.register_tool_plugin(plugin2).unwrap();
        let schemas = registry.all_tool_schemas();
        assert_eq!(schemas.len(), 2);
    }
}
