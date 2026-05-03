//! Tests for agent config loading and defaults.

use bonbo_agent::config::AgentConfig;

#[test]
fn test_testnet_default_config() {
    let config = AgentConfig::testnet_default();
    assert_eq!(config.account.mode, "testnet");
    assert_eq!(config.account.initial_capital, 1000.0);
    assert_eq!(config.account.currency, "USDT");
}

#[test]
fn test_default_risk_params() {
    let config = AgentConfig::testnet_default();
    assert_eq!(config.risk.max_leverage, 3);
    assert_eq!(config.risk.max_position_pct, 30);
    assert_eq!(config.risk.max_open_positions, 3);
    assert_eq!(config.risk.daily_loss_limit_pct, 3);
    assert_eq!(config.risk.max_drawdown_pct, 10);
    assert!(config.risk.min_risk_reward >= 1.5);
    assert_eq!(config.risk.max_daily_trades, 10);
    assert_eq!(config.risk.consecutive_loss_pause, 5);
}

#[test]
fn test_default_execution_config() {
    let config = AgentConfig::testnet_default();
    assert_eq!(config.execution.order_type, "LIMIT");
    assert!(config.execution.use_trailing_stop);
    assert!(config.execution.partial_close);
}

#[test]
fn test_default_strategy_config() {
    let config = AgentConfig::testnet_default();
    assert!(config.strategy.min_quant_score >= 50);
    assert!(config.strategy.min_hurst > 0.5);
    assert!(!config.strategy.timeframes.is_empty());
}

#[test]
fn test_default_watchlist() {
    let config = AgentConfig::testnet_default();
    assert!(!config.watchlist.symbols.is_empty());
    assert!(config.watchlist.symbols.contains(&"BTCUSDT".to_string()));
    assert!(config.watchlist.symbols.contains(&"ETHUSDT".to_string()));
}

#[test]
fn test_config_serialization_roundtrip() {
    let config = AgentConfig::testnet_default();
    let toml_str = toml::to_string(&config).unwrap();
    let decoded: AgentConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(decoded.account.mode, config.account.mode);
    assert_eq!(decoded.risk.max_leverage, config.risk.max_leverage);
    assert_eq!(
        decoded.watchlist.symbols.len(),
        config.watchlist.symbols.len()
    );
}

#[test]
fn test_config_load_from_toml_string() {
    let config = AgentConfig::testnet_default();
    let toml_str = toml::to_string(&config).unwrap();
    let loaded = AgentConfig::load_from_str(&toml_str).unwrap();
    assert_eq!(loaded.account.mode, "testnet");
}
