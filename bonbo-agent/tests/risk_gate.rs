//! Tests for risk gate — pre-trade risk validation.

use bonbo_agent::config::AgentConfig;
use bonbo_agent::risk_gate::RiskGate;
use bonbo_executor::saga::TradeParams;
use bonbo_position_manager::PositionTracker;
use rust_decimal::Decimal;

fn make_risk_gate() -> RiskGate {
    RiskGate::new(AgentConfig::testnet_default(), Decimal::new(10000, 0)) // 10000 USDT
}

fn make_trade(quantity: f64, entry: f64, sl: f64, tp: f64) -> TradeParams {
    TradeParams::long(
        "BTCUSDT",
        Decimal::from_f64_retain(quantity).unwrap(),
        Decimal::from_f64_retain(entry).unwrap(),
        Decimal::from_f64_retain(sl).unwrap(),
        Decimal::from_f64_retain(tp).unwrap(),
    )
}

#[tokio::test]
async fn test_risk_gate_approves_valid_trade() {
    let gate = make_risk_gate();
    let tracker = PositionTracker::new();
    // R:R = (51000-50000)/(50000-49800) = 1000/200 = 5.0 > 1.5 ✓
    let trade = make_trade(0.001, 50000.0, 49800.0, 51000.0);

    let result = gate.validate(&trade, &tracker).await;
    assert!(result.approved, "Expected approval, got: {}", result.reason);
}

#[tokio::test]
async fn test_risk_gate_rejects_oversized_position() {
    let gate = make_risk_gate();
    let tracker = PositionTracker::new();
    // 30% of 10000 = 3000 max. Trade notional = 0.5 * 50000 = 25000 >> 3000
    let trade = make_trade(0.5, 50000.0, 49800.0, 51000.0);

    let result = gate.validate(&trade, &tracker).await;
    assert!(result.approved, "Should be approved with adjusted qty");
    assert!(
        result.adjusted_quantity.is_some(),
        "Should have adjusted qty"
    );
}

#[tokio::test]
async fn test_risk_gate_equity_tracking() {
    let mut gate = make_risk_gate();
    assert_eq!(gate.equity(), Decimal::new(10000, 0));

    gate.update_equity(Decimal::new(10500, 0));
    assert_eq!(gate.equity(), Decimal::new(10500, 0));

    gate.update_equity(Decimal::new(10200, 0));
    assert_eq!(gate.equity(), Decimal::new(10200, 0));
}

#[tokio::test]
async fn test_risk_gate_consecutive_losses() {
    let mut gate = make_risk_gate();
    // Config has consecutive_loss_pause = 5
    for _ in 0..4 {
        gate.record_trade(Decimal::new(-10, 0)); // 4 consecutive losses
    }
    // Should still allow trading at 4 losses (< 5 threshold)
    let tracker = PositionTracker::new();
    let trade = make_trade(0.001, 50000.0, 49800.0, 51000.0);
    let result = gate.validate(&trade, &tracker).await;
    assert!(result.approved, "Should still approve with 4 losses");

    // 5th loss
    gate.record_trade(Decimal::new(-10, 0));
    let result = gate.validate(&trade, &tracker).await;
    assert!(!result.approved, "Should reject after 5 consecutive losses");
}

#[tokio::test]
async fn test_risk_gate_win_resets_consecutive() {
    let mut gate = make_risk_gate();
    for _ in 0..4 {
        gate.record_trade(Decimal::new(-10, 0));
    }
    // Win resets counter
    gate.record_trade(Decimal::new(50, 0));
    // Should be back to 0 consecutive losses, allow again
    let tracker = PositionTracker::new();
    let trade = make_trade(0.001, 50000.0, 49800.0, 51000.0);
    let result = gate.validate(&trade, &tracker).await;
    assert!(result.approved, "Should approve after win reset");
}

#[tokio::test]
async fn test_risk_gate_daily_reset() {
    let mut gate = make_risk_gate();
    for _ in 0..5 {
        gate.record_trade(Decimal::new(-10, 0));
    }
    gate.reset_daily();
    // After reset, consecutive losses are preserved
    let tracker = PositionTracker::new();
    let trade = make_trade(0.001, 50000.0, 49800.0, 51000.0);
    let result = gate.validate(&trade, &tracker).await;
    // Consecutive losses is still 5 from before, should reject
    assert!(
        !result.approved,
        "Consecutive losses persist after daily reset"
    );
}
