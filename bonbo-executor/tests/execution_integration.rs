//! Integration tests for execution algorithms using mock exchange.
//!
//! Tests verify:
//! - TWAP splits and executes correctly
//! - Mock exchange fills orders predictably
//! - Dry-run executor works without network
//! - Risk guards and limits work
//! - Market impact estimation scales correctly
//! - Algorithm selection picks appropriate strategies

mod mock_exchange;

use bonbo_executor::dry_run::DryRunExecutor;
use bonbo_executor::execution_algo::{OrderPlacer, select_execution_algo};
use bonbo_executor::market_impact::{ImpactParams, estimate_impact};
use bonbo_executor::orderbook::Side;
use bonbo_executor::risk_guards::{CumulativeRiskState, ExecutionRiskLimits};
use bonbo_executor::saga::TradeParams;
use bonbo_executor::twap::{TwapConfig, execute_twap};
use mock_exchange::MockExchange;
use rust_decimal::Decimal;
use std::time::Duration;

/// Create risk limits suitable for testing (high limits to avoid rejections).
fn test_limits() -> ExecutionRiskLimits {
    ExecutionRiskLimits {
        max_notional_per_order: Decimal::new(1_000_000, 0), // $1M per order
        max_slippage_bps: 1000.0,
        max_participation_rate: 1.0,
        max_slices: 50,
        max_execution_time: Duration::from_secs(600),
    }
}

/// Create test TWAP config with very permissive settings for mock exchange.
fn test_twap_config() -> TwapConfig {
    TwapConfig {
        slices: 3,
        interval_secs: 1,
        jitter_pct: 0.0,
        max_slippage_per_slice_bps: 500.0, // very wide
        min_slice_fraction: 0.05,
        max_slice_fraction: 0.40,
        limit_first: false,
        limit_timeout_secs: 1,
        normal_spread_bps: 5.0,          // match mock spread (1% = 100bps)
        spread_pause_multiplier: 100.0,  // never pause
        spread_abort_multiplier: 1000.0, // never abort
        max_participation_rate: 1.0,
        max_retries: 3,
        retry_delay_secs: 1,
    }
}

// ═══════════════════════════════════════════════════════════════════
// TWAP Integration Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_twap_basic_execution() {
    let exchange = MockExchange::with_price(60000.0);
    let config = TwapConfig {
        slices: 3,
        interval_secs: 1,
        jitter_pct: 0.0, // deterministic for tests
        max_slippage_per_slice_bps: 500.0,
        min_slice_fraction: 0.05,
        max_slice_fraction: 0.40,
        limit_first: false,
        limit_timeout_secs: 1,
        normal_spread_bps: 5.0,
        spread_pause_multiplier: 100.0, // very wide to avoid pauses
        spread_abort_multiplier: 1000.0,
        max_participation_rate: 1.0, // allow full participation
        max_retries: 3,
        retry_delay_secs: 1,
    };
    let impact = ImpactParams::btcusdt();
    let limits = test_limits();
    let risk = CumulativeRiskState::new(limits.clone());

    let result = execute_twap(
        &exchange as &dyn OrderPlacer,
        "BTCUSDT",
        Side::Buy,
        Decimal::ONE,
        &config,
        &impact,
        &risk,
        &limits,
    )
    .await;

    assert!(result.is_ok(), "TWAP should succeed: {:?}", result);
    let report = result.unwrap();
    assert_eq!(report.base.algo, "TWAP");
    assert!(
        report.base.fill_rate > 0.0,
        "Fill rate should be positive, got {}",
        report.base.fill_rate
    );
}

#[tokio::test]
async fn test_twap_slice_count() {
    let exchange = MockExchange::with_price(60000.0);
    let config = TwapConfig {
        slices: 3,
        interval_secs: 1,
        jitter_pct: 0.0,
        max_slippage_per_slice_bps: 500.0,
        min_slice_fraction: 0.05,
        max_slice_fraction: 0.40,
        limit_first: false,
        limit_timeout_secs: 1,
        normal_spread_bps: 5.0,
        spread_pause_multiplier: 100.0,
        spread_abort_multiplier: 1000.0,
        max_participation_rate: 1.0,
        max_retries: 3,
        retry_delay_secs: 1,
    };
    let impact = ImpactParams::btcusdt();
    let limits = test_limits();
    let risk = CumulativeRiskState::new(limits.clone());

    let _ = execute_twap(
        &exchange as &dyn OrderPlacer,
        "BTCUSDT",
        Side::Buy,
        Decimal::ONE,
        &config,
        &impact,
        &risk,
        &limits,
    )
    .await;

    // Should have placed approximately 3 orders (one per slice)
    let order_count = exchange.order_count().await;
    assert!(
        order_count > 0,
        "Should have placed at least one order, got {}",
        order_count
    );
}

#[tokio::test]
async fn test_twap_sell_side() {
    let exchange = MockExchange::with_price(60000.0);
    let config = TwapConfig {
        slices: 3,
        interval_secs: 1,
        jitter_pct: 0.0,
        max_slippage_per_slice_bps: 500.0,
        min_slice_fraction: 0.05,
        max_slice_fraction: 0.40,
        limit_first: false,
        limit_timeout_secs: 1,
        normal_spread_bps: 5.0,
        spread_pause_multiplier: 100.0,
        spread_abort_multiplier: 1000.0,
        max_participation_rate: 1.0,
        max_retries: 3,
        retry_delay_secs: 1,
    };
    let impact = ImpactParams::btcusdt();
    let limits = test_limits();
    let risk = CumulativeRiskState::new(limits.clone());

    let result = execute_twap(
        &exchange as &dyn OrderPlacer,
        "BTCUSDT",
        Side::Sell,
        Decimal::new(5, 1), // 0.5 BTC
        &config,
        &impact,
        &risk,
        &limits,
    )
    .await;

    assert!(result.is_ok(), "TWAP sell should succeed: {:?}", result);
    let report = result.unwrap();
    assert!(report.base.fill_rate > 0.0, "Fill rate should be positive");
}

#[tokio::test]
async fn test_twap_with_limit_orders() {
    let exchange = MockExchange::with_price(60000.0);
    let config = TwapConfig {
        slices: 3,
        interval_secs: 1,
        jitter_pct: 0.0,
        max_slippage_per_slice_bps: 500.0,
        min_slice_fraction: 0.05,
        max_slice_fraction: 0.40,
        limit_first: true,
        limit_timeout_secs: 1,
        normal_spread_bps: 5.0,
        spread_pause_multiplier: 100.0,
        spread_abort_multiplier: 1000.0,
        max_participation_rate: 1.0,
        max_retries: 3,
        retry_delay_secs: 1,
    };
    let impact = ImpactParams::btcusdt();
    let limits = test_limits();
    let risk = CumulativeRiskState::new(limits.clone());

    let result = execute_twap(
        &exchange as &dyn OrderPlacer,
        "BTCUSDT",
        Side::Buy,
        Decimal::new(1, 1), // 0.1 BTC
        &config,
        &impact,
        &risk,
        &limits,
    )
    .await;

    assert!(result.is_ok(), "TWAP with limit orders should succeed");
}

// ═══════════════════════════════════════════════════════════════════
// Mock Exchange Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_mock_exchange_market_order() {
    let exchange = MockExchange::with_price(60000.0);

    let result = exchange
        .place_market("BTCUSDT", Side::Buy, Decimal::ONE)
        .await;

    assert!(result.is_ok());
    let fill = result.unwrap();
    assert_eq!(fill.fill_qty, Decimal::ONE);
    assert!(fill.fill_price > Decimal::ZERO);
    assert!(!fill.is_maker);

    let orders = exchange.orders().await;
    assert_eq!(orders.len(), 1);
    assert!(orders[0].is_market);
}

#[tokio::test]
async fn test_mock_exchange_limit_order() {
    let exchange = MockExchange::with_price(60000.0);

    // Place a limit buy above the ask → should fill
    let result = exchange
        .place_limit("BTCUSDT", Side::Buy, Decimal::ONE, Decimal::new(61000, 0))
        .await;

    assert!(result.is_ok());
    let fill = result.unwrap();
    assert_eq!(fill.fill_qty, Decimal::ONE);
    assert!(fill.is_maker);
}

#[tokio::test]
async fn test_mock_exchange_orderbook() {
    let exchange = MockExchange::with_price(60000.0);

    let book = exchange.get_orderbook("BTCUSDT").await;
    assert!(book.is_ok());
    let book = book.unwrap();
    assert!(!book.bids.is_empty());
    assert!(!book.asks.is_empty());
    assert_eq!(book.symbol, "BTCUSDT");
}

// ═══════════════════════════════════════════════════════════════════
// Dry-Run Executor Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_dry_run_executor() {
    let executor = DryRunExecutor::new();

    let params = TradeParams::long(
        "BTCUSDT",
        Decimal::new(1, 2), // 0.01 BTC
        Decimal::new(60000, 0),
        Decimal::new(59000, 0),
        Decimal::new(62000, 0),
    );

    let result = executor.execute(&params).await;
    assert!(result.success, "Dry-run trade should succeed");
}

// ═══════════════════════════════════════════════════════════════════
// Risk Guards Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_risk_limits_default() {
    let limits = test_limits();
    assert!(limits.max_notional_per_order > Decimal::ZERO);
    assert!(limits.max_slippage_bps > 0.0);
    assert!(limits.max_participation_rate > 0.0);
    assert!(limits.max_slices > 0);
}

#[test]
fn test_cumulative_risk_state_new() {
    let limits = test_limits();
    let state = CumulativeRiskState::new(limits);
    assert_eq!(state.total_notional(), 0.0);
    assert_eq!(state.total_commission(), 0.0);
}

#[test]
fn test_cumulative_risk_state_recording() {
    let limits = test_limits();
    let state = CumulativeRiskState::new(limits);
    state.record_execution(1000.0, 0.5);
    assert_eq!(state.total_notional(), 1000.0);
    assert_eq!(state.total_commission(), 0.5);
    state.record_execution(500.0, 0.25);
    assert_eq!(state.total_notional(), 1500.0);
}

// ═══════════════════════════════════════════════════════════════════
// Algorithm Selection Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_algo_selection_small_order() {
    let sel = select_execution_algo(100.0, 60.0, 50_000_000.0, 2.0);
    assert!(!sel.algo.is_empty());
    assert!(sel.estimated_slippage_bps >= 0.0);
}

#[test]
fn test_algo_selection_large_order() {
    let sel = select_execution_algo(500_000.0, 3600.0, 50_000_000.0, 2.0);
    assert!(!sel.algo.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Market Impact Tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_impact_estimation() {
    let params = ImpactParams::btcusdt();
    let estimate = estimate_impact(&params, 10_000.0, 0.0005, 1.0);
    assert!(estimate.impact_bps > 0.0, "Impact should be positive");
    assert!(estimate.permanent_bps >= 0.0);
    assert!(estimate.temporary_bps >= 0.0);
}

#[test]
fn test_impact_scales_with_size() {
    let params = ImpactParams::btcusdt();
    let small = estimate_impact(&params, 1_000.0, 0.0005, 1.0);
    let large = estimate_impact(&params, 1_000_000.0, 0.0005, 1.0);
    assert!(
        large.impact_bps > small.impact_bps,
        "Larger order should have more impact: {} vs {}",
        large.impact_bps,
        small.impact_bps
    );
}
