//! BonBo Extend MCP Server
//!
//! Exposes bonbo-extend tools via the Model Context Protocol (MCP).
//! BonBo AI agent can discover and use these tools through MCP.
//!
//! ## Usage
//!
//! ```bash
//! # Start MCP server (stdio transport)
//! bonbo-extend-mcp
//!
//! # Or configure in BonBo's MCP config
//! # ~/.bonbo/mcp-servers.json
//! ```

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use tracing::{debug, error, info};

use bonbo_extend::registry::PluginRegistry;
use bonbo_extend::tools::{MarketDataPlugin, PriceAlertPlugin, SystemMonitorPlugin};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is for MCP protocol)
    tracing_subscriber::fmt()
        .with_env_filter("bonbo_extend_mcp=debug")
        .with_writer(std::io::stderr)
        .init();

    info!("Starting BonBo Extend MCP Server");

    // Build plugin registry
    let registry = build_registry()?;
    info!(
        "Loaded {} plugins with {} tools",
        registry.plugin_count(),
        registry.tool_count()
    );

    // Initialize plugins
    registry.init_all().await?;

    // Run MCP server on stdio
    run_mcp_server(registry).await?;

    Ok(())
}

fn build_registry() -> Result<PluginRegistry> {
    let mut registry = PluginRegistry::new();

    // Register built-in plugins
    registry.register_tool_plugin(MarketDataPlugin::new())?;
    registry.register_tool_plugin(PriceAlertPlugin::new())?;
    registry.register_tool_plugin(SystemMonitorPlugin::new())?;

    // TODO: Auto-discover external plugins from ~/.bonbo/plugins/

    Ok(registry)
}

/// Simple MCP stdio server.
/// Implements JSON-RPC 2.0 over stdin/stdout.
async fn run_mcp_server(registry: PluginRegistry) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    info!("MCP server listening on stdio");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("Error reading stdin: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        debug!("Received: {}", line);

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "error": {"code": -32700, "message": format!("Parse error: {}", e)},
                    "id": null
                });
                write_response(&mut stdout, &response)?;
                continue;
            }
        };

        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => handle_initialize(id.clone()),
            "tools/list" => handle_tools_list(&registry, id.clone()),
            "tools/call" => handle_tools_call(&registry, params, id.clone()).await,
            "ping" => json!({"jsonrpc": "2.0", "result": {}, "id": id}),
            _ => json!({
                "jsonrpc": "2.0",
                "error": {"code": -32601, "message": format!("Method not found: {}", method)},
                "id": id
            }),
        };

        write_response(&mut stdout, &response)?;
    }

    registry.shutdown_all().await?;
    Ok(())
}

fn write_response(stdout: &mut io::Stdout, response: &Value) -> Result<()> {
    let mut output = serde_json::to_string(response)?;
    output.push('\n');
    stdout.write_all(output.as_bytes())?;
    stdout.flush()?;
    debug!("Sent: {}", output.trim());
    Ok(())
}

fn handle_initialize(id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "bonbo-extend",
                "version": env!("CARGO_PKG_VERSION")
            }
        },
        "id": id
    })
}

fn handle_tools_list(registry: &PluginRegistry, id: Option<Value>) -> Value {
    let tools: Vec<Value> = registry
        .all_tool_schemas()
        .iter()
        .map(|schema| {
            // Convert our ToolSchema to MCP tool format
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for param in &schema.parameters {
                let mut prop = json!({
                    "type": param.param_type,
                    "description": param.description
                });
                if let Some(default) = &param.default {
                    prop["default"] = default.clone();
                }
                if let Some(enum_vals) = &param.r#enum {
                    prop["enum"] = Value::Array(
                        enum_vals.iter().map(|v| Value::String(v.clone())).collect()
                    );
                }
                properties.insert(param.name.clone(), prop);
                if param.required {
                    required.push(Value::String(param.name.clone()));
                }
            }

            json!({
                "name": schema.name,
                "description": schema.description,
                "inputSchema": {
                    "type": "object",
                    "properties": properties,
                    "required": required
                }
            })
        })
        .collect();

    json!({
        "jsonrpc": "2.0",
        "result": { "tools": tools },
        "id": id
    })
}

async fn handle_tools_call(
    registry: &PluginRegistry,
    params: Value,
    id: Option<Value>,
) -> Value {
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("");

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(json!({}));

    if tool_name.is_empty() {
        return json!({
            "jsonrpc": "2.0",
            "error": {"code": -32602, "message": "Missing tool name"},
            "id": id
        });
    }

    info!("Calling tool: {}", tool_name);

    match registry.execute_tool(tool_name, &arguments).await {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{
                    "type": "text",
                    "text": result
                }]
            },
            "id": id
        }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", e)
                }],
                "isError": true
            },
            "id": id
        }),
    }
}
