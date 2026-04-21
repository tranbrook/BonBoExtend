//! Integration tests for Decision Loop with trait-based architecture.

use bonbo_agent::config::AgentConfig;
use bonbo_agent::decision_loop::DecisionLoop;
use bonbo_agent::mock_mcp::MockMcpClient;
use bonbo_agent::mcp_client::*;
use bonbo_agent::{DryRunOrderExecutor, OrderExecutor};
use bonbo_agent::state_machine::AgentState;
use rust_decimal::Decimal;

fn test_config() -> AgentConfig {
    let mut config = AgentConfig::testnet_default();
    config.watchlist.symbols = vec![
        "BTCUSDT".to_string(),
        "ETHUSDT".to_string(),
    ];
    config.risk.max_leverage = 3;
    config.strategy.min_quant_score = 50;
    config
}

#[tokio::test]
async fn test_full_decision_cycle_dry_run() {
    let config = test_config();
    let equity = Decimal::new(190, 2);
    let decision_loop = DecisionLoop::new(config, equity);

    let mcp = MockMcpClient::default();
    let executor = DryRunOrderExecutor::new();

    let result = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;

    assert!(result.is_ok(), "Decision cycle should succeed");
}

#[tokio::test]
async fn test_scan_candidates_with_mock() {
    let mcp = MockMcpClient::default();
    let results = mcp.scan_market(&["BTCUSDT".to_string()]).await;
    assert!(results.is_ok());
    let candidates = results.unwrap();
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].symbol, "BTCUSDT");
}

#[tokio::test]
async fn test_analyze_indicators_with_mock() {
    let mcp = MockMcpClient::default();
    let result = mcp.analyze_indicators("BTCUSDT", "4h").await;
    assert!(result.is_ok());

    let indicator = result.unwrap();
    assert_eq!(indicator.symbol, "BTCUSDT");
    assert_eq!(indicator.timeframe, "4h");
    assert!(indicator.buy_signals > 0);
    assert!(indicator.score > 0);
}

#[tokio::test]
async fn test_detect_regime_with_mock() {
    let mcp = MockMcpClient::default();
    let result = mcp.detect_regime("BTCUSDT").await;
    assert!(result.is_ok());
    let regime = result.unwrap();
    assert_eq!(regime.regime, "Trending");
    assert!(regime.confidence > 0.0);
}

#[tokio::test]
async fn test_funding_rate_filter() {
    let mcp = MockMcpClient::default();
    let rate = mcp.get_funding_rate("BTCUSDT").await;
    assert!(rate.is_ok());
    assert!(rate.unwrap().abs() < Decimal::new(1, 2));
}

#[tokio::test]
async fn test_dry_run_executor() {
    let executor = DryRunOrderExecutor::new();
    assert!(executor.is_dry_run());

    let result = executor.cancel_all("BTCUSDT").await;
    assert!(result.is_ok());

    let orders: std::vec::Vec<bonbo_binance_futures::models::OrderResponse> =
        executor.get_open_orders("BTCUSDT").await.unwrap();
    assert!(orders.is_empty());
}

#[tokio::test]
async fn test_state_starts_idle() {
    let config = test_config();
    let equity = Decimal::new(190, 2);
    let decision_loop = DecisionLoop::new(config, equity);
    let state = decision_loop.state().await;
    assert!(matches!(state, AgentState::Idle));
}

#[tokio::test]
async fn test_kill_switch_blocks_cycle() {
    let config = test_config();
    let equity = Decimal::new(190, 2);
    let decision_loop = DecisionLoop::new(config, equity);

    decision_loop.activate_kill_switch().await;

    let mcp = MockMcpClient::default();
    let executor = DryRunOrderExecutor::new();

    let result = decision_loop
        .run_cycle(&mcp as &dyn McpClient, &executor as &dyn OrderExecutor)
        .await;
    assert!(result.is_ok());

    let state = decision_loop.state().await;
    assert!(matches!(state, AgentState::Stopped));
}
