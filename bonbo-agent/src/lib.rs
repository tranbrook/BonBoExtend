//! BonBo Agent — 24/7 autonomous trading agent.

pub mod config;
pub mod decision_loop;
pub mod kill_switch;
pub mod mcp_client;
pub mod mock_mcp;
pub mod order_executor;
pub mod orchestrator;
pub mod risk_gate;
pub mod state_machine;

pub use config::AgentConfig;
pub use decision_loop::DecisionLoop;
pub use kill_switch::KillSwitch;
pub use mcp_client::McpClient;
pub use order_executor::{DryRunOrderExecutor, LiveOrderExecutor, OrderExecutor};
pub use orchestrator::Orchestrator;
pub use risk_gate::RiskGate;
pub use state_machine::AgentState;
