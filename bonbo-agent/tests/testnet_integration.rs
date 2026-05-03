//! Testnet Integration Tests — End-to-end agent flow with Binance Testnet.
//!
//! These tests verify the complete trading pipeline:
//! 1. Config loading (testnet mode)
//! 2. MCP analysis (mock)
//! 3. Decision loop → Risk gate → Order execution (dry-run)
//! 4. Position tracking
//!
//! Run with: cargo test -p bonbo-agent --test testnet_integration
//!
//! NOTE: These tests use DRY-RUN mode by default.
//! For live testnet testing, set environment variables:
//!   BINANCE_API_KEY=testnet_key
//!   BINANCE_API_SECRET=testnet_secret
//!   BONBO_AGENT_MODE=testnet

use bonbo_agent::config::AgentConfig;
use bonbo_agent::decision_loop::DecisionLoop;
use bonbo_agent::mock_mcp::MockMcpClient;
use bonbo_agent::order_executor::DryRunOrderExecutor;
use bonbo_agent::state_machine::AgentState;
use bonbo_agent::{McpClient, OrderExecutor};
use rust_decimal::Decimal;

/// Test that the full agent pipeline works in dry-run mode.
#[tokio::test]
async fn test_full_pipeline_dry_run() {
    let config = AgentConfig::testnet_default();
    let equity = Decimal::ONE_HUNDRED;
    let decision_loop = DecisionLoop::new(config.clone(), equity);
    let executor = DryRunOrderExecutor::new();
    let mcp = MockMcpClient::default();

    // Run one decision cycle
    let result = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;

    // Should complete without error (even if no signal generated)
    assert!(
        result.is_ok(),
        "Decision cycle should not error: {:?}",
        result
    );

    // State should be back to Idle or Monitoring (not stuck in executing)
    let state = decision_loop.state().await;
    assert!(
        matches!(state, AgentState::Idle | AgentState::Monitoring),
        "State should not be stuck: {:?}",
        state
    );
}

/// Test that kill switch stops the agent.
#[tokio::test]
async fn test_kill_switch_stops_cycle() {
    let config = AgentConfig::testnet_default();
    let equity = Decimal::ONE_HUNDRED;
    let decision_loop = DecisionLoop::new(config.clone(), equity);
    let executor = DryRunOrderExecutor::new();
    let mcp = MockMcpClient::default();

    // Activate kill switch via decision loop
    decision_loop.activate_kill_switch().await;

    // Decision cycle should detect kill switch and stop
    let result = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;

    // Should return Ok (graceful stop)
    assert!(result.is_ok(), "Should handle kill switch gracefully");

    // State should be Stopped
    let state = decision_loop.state().await;
    assert_eq!(
        state,
        AgentState::Stopped,
        "State should be Stopped after kill switch"
    );

    // Cleanup
    decision_loop.deactivate_kill_switch().await;
}

/// Test multiple consecutive decision cycles.
#[tokio::test]
async fn test_multiple_cycles() {
    let config = AgentConfig::testnet_default();
    let equity = Decimal::ONE_THOUSAND;
    let decision_loop = DecisionLoop::new(config.clone(), equity);
    let executor = DryRunOrderExecutor::new();
    let mcp = MockMcpClient::default();

    // Run 5 cycles rapidly
    for i in 0..5 {
        let result = decision_loop
            .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
            .await;
        assert!(result.is_ok(), "Cycle {} should succeed: {:?}", i, result);
    }
}

/// Test that the agent handles MCP errors gracefully.
#[tokio::test]
async fn test_mcp_error_handling() {
    let config = AgentConfig::testnet_default();
    let equity = Decimal::ONE_THOUSAND;
    let decision_loop = DecisionLoop::new(config.clone(), equity);
    let executor = DryRunOrderExecutor::new();

    // Create a failing MCP client
    struct FailingMcpClient;
    #[async_trait::async_trait]
    impl McpClient for FailingMcpClient {
        async fn scan_market(
            &self,
            _symbols: &[String],
        ) -> anyhow::Result<Vec<bonbo_agent::mcp_client::ScanResult>> {
            Err(anyhow::anyhow!("MCP server unreachable"))
        }
        async fn analyze_indicators(
            &self,
            _symbol: &str,
            _timeframe: &str,
        ) -> anyhow::Result<bonbo_agent::mcp_client::IndicatorResult> {
            Err(anyhow::anyhow!("MCP server unreachable"))
        }
        async fn detect_regime(
            &self,
            _symbol: &str,
        ) -> anyhow::Result<bonbo_agent::mcp_client::RegimeResult> {
            Err(anyhow::anyhow!("MCP server unreachable"))
        }
        async fn get_trading_signals(
            &self,
            _symbol: &str,
            _timeframe: &str,
        ) -> anyhow::Result<Vec<bonbo_agent::mcp_client::TradingSignal>> {
            Err(anyhow::anyhow!("MCP server unreachable"))
        }
        async fn get_funding_rate(&self, _symbol: &str) -> anyhow::Result<Decimal> {
            Err(anyhow::anyhow!("MCP server unreachable"))
        }
    }

    let mcp = FailingMcpClient;

    // Agent should handle MCP failure gracefully (not panic)
    let result = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;

    // Should either succeed (no action taken) or return a non-fatal error
    // The key is: it should NOT panic
    match &result {
        Ok(()) => { /* Agent handled it gracefully by skipping */ }
        Err(e) => {
            // Error should be informational, not a panic
            assert!(!e.to_string().is_empty(), "Error should have a message");
        }
    }
}

/// Test config loading for testnet mode.
#[test]
fn test_testnet_config() {
    let config = AgentConfig::testnet_default();
    assert_eq!(config.account.mode, "testnet");
    assert!(!config.watchlist.symbols.is_empty());
    assert!(config.strategy.scan_interval_minutes > 0);
    assert!(config.risk.max_leverage > 0);
    assert!(config.risk.max_position_pct > 0);
}

/// Test that risk gate works through decision loop.
#[tokio::test]
async fn test_risk_gate_integrated() {
    // Use a very small equity to trigger risk limits
    let config = AgentConfig::testnet_default();
    let tiny_equity = Decimal::ONE; // Only 1 USDT equity

    let decision_loop = DecisionLoop::new(config.clone(), tiny_equity);
    let executor = DryRunOrderExecutor::new();
    let mcp = MockMcpClient::default();

    // With only 1 USDT, the signal (entry at 75500) should be rejected
    // by risk gate (notional >> equity)
    let result = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;

    // Should still succeed (risk gate rejects, but no crash)
    assert!(result.is_ok(), "Should handle risk rejection gracefully");
}

/// Test state machine transitions during a cycle.
#[tokio::test]
async fn test_state_transitions() {
    let config = AgentConfig::testnet_default();
    let equity = Decimal::ONE_THOUSAND;
    let decision_loop = DecisionLoop::new(config.clone(), equity);
    let executor = DryRunOrderExecutor::new();
    let mcp = MockMcpClient::default();

    // Initial state should be Idle
    let initial = decision_loop.state().await;
    assert_eq!(initial, AgentState::Idle);

    // Run a cycle
    let _ = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;

    // After cycle, should be back to Idle or Monitoring
    let final_state = decision_loop.state().await;
    assert!(
        matches!(final_state, AgentState::Idle | AgentState::Monitoring),
        "Final state should be Idle or Monitoring, got: {:?}",
        final_state
    );
}

/// Test that mock MCP returns consistent data.
#[tokio::test]
async fn test_mock_mcp_consistency() {
    let mcp = MockMcpClient::default();

    let scan = mcp.scan_market(&["BTCUSDT".to_string()]).await;
    assert!(scan.is_ok());
    assert!(!scan.unwrap().is_empty());

    let indicators = mcp.analyze_indicators("BTCUSDT", "4h").await;
    assert!(indicators.is_ok());
    let ind = indicators.unwrap();
    assert_eq!(ind.symbol, "BTCUSDT");
    assert_eq!(ind.timeframe, "4h");

    let regime = mcp.detect_regime("BTCUSDT").await;
    assert!(regime.is_ok());

    let signals = mcp.get_trading_signals("BTCUSDT", "4h").await;
    assert!(signals.is_ok());
    assert!(!signals.unwrap().is_empty());

    let funding = mcp.get_funding_rate("BTCUSDT").await;
    assert!(funding.is_ok());
}
