//! BonBo Agent — 24/7 autonomous trading agent.

pub mod alert;
pub mod config;
pub mod decision_loop;
pub mod kill_switch;
pub mod live_mcp;
pub mod mcp_client;
pub mod mock_mcp;
pub mod monitor;
pub mod orchestrator;
pub mod order_executor;
pub mod risk_gate;
pub mod state_machine;

pub use alert::{
    Alert, AlertLevel, AlertNotifier, LogAlertNotifier, MultiAlertNotifier, WebhookAlertNotifier,
};
pub use config::AgentConfig;
pub use decision_loop::DecisionLoop;
pub use kill_switch::KillSwitch;
pub use live_mcp::LiveMcpClient;
pub use mcp_client::McpClient;
pub use monitor::{AgentMetrics, MetricEventType, MetricsCollector};
pub use orchestrator::Orchestrator;
pub use order_executor::{DryRunOrderExecutor, LiveOrderExecutor, OrderExecutor};
pub use risk_gate::RiskGate;
pub use state_machine::AgentState;
