//! BonBo Extend MCP Server
//!
//! Exposes bonbo-extend tools via the Model Context Protocol (MCP).
//! Supports two transport modes:
//!   - **stdio** (default): for CLI integration
//!   - **http/sse**: for BonBo AI agent (McpClient uses HTTP POST)
//!
//! ## Usage
//!
//! ```bash
//! # Mode 1: stdio (for pipes, subprocesses)
//! bonbo-extend-mcp
//!
//! # Mode 2: HTTP server (for BonBo MCP client)
//! bonbo-extend-mcp --http --port 9876
//!
//! # Mode 3: Background daemon
//! bonbo-extend-mcp --http --port 9876 --daemon
//! ```

use anyhow::Result;
use bonbo_extend::PluginRegistry;
use bonbo_extend::tools::{
    BacktestPlugin, DerivativesPlugin, JournalPlugin, LearningPlugin, MarketDataPlugin,
    PortfolioPlugin, PositionAnalyzerPlugin, PriceAlertPlugin, RegimePlugin, RiskPlugin,
    ScannerPlugin, SentinelPlugin, SystemMonitorPlugin, TechnicalAnalysisPlugin, TradingPlugin,
    ValidationPlugin,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

// ─── Rate Limiter ─────────────────────────────────────────────

/// Simple per-tool rate limiter using sliding window.
struct RateLimiter {
    /// Tool name → list of recent request timestamps.
    requests: RwLock<HashMap<String, Vec<Instant>>>,
    /// Max requests per tool per minute.
    max_per_minute: usize,
}

impl RateLimiter {
    fn new(max_per_minute: usize) -> Self {
        Self {
            requests: RwLock::new(HashMap::new()),
            max_per_minute,
        }
    }

    /// Check if a request is allowed. Returns false if rate limited.
    async fn check(&self, tool: &str) -> bool {
        let now = Instant::now();
        let cutoff = now - std::time::Duration::from_secs(60);
        let mut map = self.requests.write().await;
        let entry = map.entry(tool.to_string()).or_default();

        // Remove expired entries
        entry.retain(|t| *t > cutoff);

        if entry.len() >= self.max_per_minute {
            warn!("Rate limited: {} ({} req/min)", tool, entry.len());
            return false;
        }

        entry.push(now);
        true
    }
}

// ─── Shared MCP Handler ──────────────────────────────────────────

fn build_registry() -> Result<PluginRegistry> {
    let mut registry = PluginRegistry::new();
    // Phase 1: Market data & monitoring
    registry.register_tool_plugin(MarketDataPlugin::new())?;
    registry.register_tool_plugin(PriceAlertPlugin::new())?;
    registry.register_tool_plugin(SystemMonitorPlugin::new())?;
    // Phase A: Technical Analysis
    registry.register_tool_plugin(TechnicalAnalysisPlugin::new())?;
    // Phase C: Backtesting
    registry.register_tool_plugin(BacktestPlugin::new())?;
    // Phase D: Sentiment & On-chain
    registry.register_tool_plugin(SentinelPlugin::new())?;
    // Phase E: Risk Management
    registry.register_tool_plugin(RiskPlugin::new())?;
    // Phase 1: Journal
    registry.register_tool_plugin(JournalPlugin::new())?;
    // Phase 2: Regime Detection
    registry.register_tool_plugin(RegimePlugin::new())?;
    // Phase 3: Learning Engine
    registry.register_tool_plugin(LearningPlugin::new())?;
    // Phase 4: Validation
    registry.register_tool_plugin(ValidationPlugin::new())?;
    // Phase 5: Scanner
    registry.register_tool_plugin(ScannerPlugin::new())?;
    // Phase 6: Trading (Binance Futures direct access)
    registry.register_tool_plugin(TradingPlugin::new())?;
    // Phase 7: Portfolio Analysis
    registry.register_tool_plugin(PortfolioPlugin::new())?;
    registry.register_tool_plugin(PositionAnalyzerPlugin::new())?;
    // Phase 8: Derivatives (Funding, OI, Sentiment, Taker, Volume Profile)
    registry.register_tool_plugin(DerivativesPlugin::new())?;
    Ok(registry)
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
                    prop["enum"] =
                        Value::Array(enum_vals.iter().map(|v| Value::String(v.clone())).collect());
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
    rate_limiter: &RateLimiter,
    params: Value,
    id: Option<Value>,
) -> Value {
    let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if tool_name.is_empty() {
        return json!({
            "jsonrpc": "2.0",
            "error": {"code": -32602, "message": "Missing tool name"},
            "id": id
        });
    }

    // Validate tool exists
    let available_tools = registry.all_tool_schemas();
    let tool_exists = available_tools.iter().any(|t| t.name == tool_name);
    if !tool_exists {
        return json!({
            "jsonrpc": "2.0",
            "error": {"code": -32602, "message": format!("Unknown tool: {}", tool_name)},
            "id": id
        });
    }

    // Rate limiting check
    if !rate_limiter.check(tool_name).await {
        return json!({
            "jsonrpc": "2.0",
            "error": {"code": -32000, "message": format!("Rate limited: {} — too many requests", tool_name)},
            "id": id
        });
    }

    info!("Calling tool: {}", tool_name);

    match registry.execute_tool(tool_name, &arguments).await {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": result}]
            },
            "id": id
        }),
        Err(e) => {
            error!("Tool error ({}): {}", tool_name, e);
            json!({
                "jsonrpc": "2.0",
                "result": {
                    "content": [{"type": "text", "text": format!("Error: {}", e)}],
                    "isError": true
                },
                "id": id
            })
        }
    }
}

/// Route any JSON-RPC request to the correct handler.
async fn route_request(
    registry: &PluginRegistry,
    rate_limiter: &RateLimiter,
    request: Value,
) -> Value {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => handle_initialize(id),
        "tools/list" => handle_tools_list(registry, id),
        "tools/call" => handle_tools_call(registry, rate_limiter, params, id).await,
        "ping" => json!({"jsonrpc": "2.0", "result": {}, "id": id}),
        _ => json!({
            "jsonrpc": "2.0",
            "error": {"code": -32601, "message": format!("Method not found: {}", method)},
            "id": id
        }),
    }
}

// ─── STDIO Transport ─────────────────────────────────────────────

async fn run_stdio(registry: &PluginRegistry, rate_limiter: &RateLimiter) -> Result<()> {
    use std::io::{self, BufRead, Write};

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
                let mut out = serde_json::to_string(&response)?;
                out.push('\n');
                stdout.write_all(out.as_bytes())?;
                stdout.flush()?;
                continue;
            }
        };

        let response = route_request(registry, rate_limiter, request).await;
        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes())?;
        stdout.flush()?;
    }

    Ok(())
}

// ─── HTTP Transport (for BonBo McpClient) ────────────────────────

async fn run_http(
    registry: Arc<PluginRegistry>,
    rate_limiter: Arc<RateLimiter>,
    port: u16,
) -> Result<()> {
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::{Json, Router, routing::post};
    use tower_http::cors::CorsLayer;

    async fn mcp_endpoint(
        State((registry, rate_limiter)): State<(Arc<PluginRegistry>, Arc<RateLimiter>)>,
        Json(request): Json<Value>,
    ) -> impl IntoResponse {
        debug!(
            "HTTP request: {}",
            serde_json::to_string(&request).unwrap_or_default()
        );
        let response = route_request(&registry, &rate_limiter, request).await;
        (StatusCode::OK, Json(response))
    }

    let app = Router::new()
        .route("/mcp", post(mcp_endpoint))
        .route("/", post(mcp_endpoint)) // also accept root
        .layer(CorsLayer::permissive())
        .with_state((registry, rate_limiter));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!("MCP HTTP server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    info!("Shutdown signal received");
}

// ─── Main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let use_http = args.iter().any(|a| a == "--http" || a == "-H");
    let port = extract_port(&args).unwrap_or(9876);

    // Init logging to stderr (stdout reserved for MCP stdio protocol)
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("BONBO_EXTEND_LOG")
                .unwrap_or_else(|_| "bonbo_extend_mcp=info".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    info!(
        "Starting BonBo Extend MCP Server v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Build plugin registry
    let registry = build_registry()?;
    info!(
        "Loaded {} plugins with {} tools",
        registry.plugin_count(),
        registry.tool_count()
    );

    // Initialize plugins
    registry.init_all().await?;

    // Create rate limiter (60 requests per tool per minute)
    let rate_limiter = RateLimiter::new(60);

    if use_http {
        // HTTP mode — for BonBo McpClient (JSON-RPC over HTTP POST)
        let registry = Arc::new(registry);
        let rate_limiter = Arc::new(rate_limiter);
        run_http(registry.clone(), rate_limiter, port).await?;
        registry.shutdown_all().await?;
    } else {
        // Stdio mode — for subprocess/pipe integration
        run_stdio(&registry, &rate_limiter).await?;
        registry.shutdown_all().await?;
    }

    info!("Server shut down cleanly");
    Ok(())
}

fn extract_port(args: &[String]) -> Option<u16> {
    for i in 0..args.len() {
        if (args[i] == "--port" || args[i] == "-p") && i + 1 < args.len() {
            return args[i + 1].parse().ok();
        }
        if let Some(rest) = args[i].strip_prefix("--port=") {
            return rest.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(5);
        for _ in 0..5 {
            assert!(limiter.check("test_tool").await);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(3);
        assert!(limiter.check("tool_a").await);
        assert!(limiter.check("tool_a").await);
        assert!(limiter.check("tool_a").await);
        assert!(!limiter.check("tool_a").await); // 4th should be blocked
    }

    #[tokio::test]
    async fn test_rate_limiter_per_tool_isolation() {
        let limiter = RateLimiter::new(1);
        assert!(limiter.check("tool_a").await);
        assert!(limiter.check("tool_b").await); // Different tool, should pass
        assert!(!limiter.check("tool_a").await); // Same tool, blocked
    }

    #[test]
    fn test_extract_port_with_flag() {
        let args = vec!["prog".to_string(), "--port".to_string(), "9876".to_string()];
        assert_eq!(extract_port(&args), Some(9876));
    }

    #[test]
    fn test_extract_port_with_equals() {
        let args = vec!["prog".to_string(), "--port=8080".to_string()];
        assert_eq!(extract_port(&args), Some(8080));
    }

    #[test]
    fn test_extract_port_missing() {
        let args = vec!["prog".to_string()];
        assert_eq!(extract_port(&args), None);
    }

    #[test]
    fn test_extract_port_invalid() {
        let args = vec!["prog".to_string(), "--port".to_string(), "abc".to_string()];
        assert_eq!(extract_port(&args), None);
    }

    #[test]
    fn test_build_registry_succeeds() {
        let registry = build_registry();
        assert!(registry.is_ok(), "Registry should build successfully");
    }
}
