//! Risk Management Plugin — position sizing, circuit breaker, risk checks.

use crate::plugin::{PluginContext, PluginMetadata, ParameterSchema, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct RiskPlugin { metadata: PluginMetadata }
impl RiskPlugin {
    pub fn new() -> Self {
        Self { metadata: PluginMetadata {
            id: "bonbo-risk".into(), name: "Risk Management".into(),
            version: env!("CARGO_PKG_VERSION").into(), description: "Position sizing, circuit breakers, risk metrics".into(),
            author: "BonBo Team".into(), tags: vec!["risk".into(), "position_sizing".into()],
        }}
    }
}

#[async_trait]
impl ToolPlugin for RiskPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.metadata }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema { name: "calculate_position_size".into(), description: "Calculate optimal position size".into(), parameters: vec![
                ParameterSchema { name: "equity".into(), param_type: "number".into(), description: "Portfolio equity USDT".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "entry_price".into(), param_type: "number".into(), description: "Entry price".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "stop_loss".into(), param_type: "number".into(), description: "Stop loss price".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "method".into(), param_type: "string".into(), description: "Sizing method".into(), required: false, default: Some(json!("fixed_percent")), r#enum: Some(vec!["fixed_percent".into(),"kelly".into(),"half_kelly".into()]) },
                ParameterSchema { name: "risk_pct".into(), param_type: "number".into(), description: "Risk % per trade".into(), required: false, default: Some(json!(0.02)), r#enum: None },
            ]},
            ToolSchema { name: "compute_risk_metrics".into(), description: "Compute portfolio risk metrics: VaR, CVaR, Sharpe, Max Drawdown".into(), parameters: vec![
                ParameterSchema { name: "trade_pnls".into(), param_type: "array".into(), description: "Array of trade PnL values".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "equity_curve".into(), param_type: "array".into(), description: "Equity values over time".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "initial_capital".into(), param_type: "number".into(), description: "Initial capital".into(), required: false, default: Some(json!(10000)), r#enum: None },
            ]},
            ToolSchema { name: "check_risk".into(), description: "Check if trading is allowed via circuit breaker".into(), parameters: vec![
                ParameterSchema { name: "equity".into(), param_type: "number".into(), description: "Current equity".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "initial_capital".into(), param_type: "number".into(), description: "Starting capital".into(), required: true, default: None, r#enum: None },
                ParameterSchema { name: "daily_pnl".into(), param_type: "number".into(), description: "Today's P&L".into(), required: false, default: Some(json!(0)), r#enum: None },
                ParameterSchema { name: "peak_equity".into(), param_type: "number".into(), description: "Highest equity".into(), required: false, default: None, r#enum: None },
                ParameterSchema { name: "consecutive_losses".into(), param_type: "integer".into(), description: "Consecutive losses".into(), required: false, default: Some(json!(0)), r#enum: None },
            ]},
        ]
    }
    async fn execute_tool(&self, tool_name: &str, args: &Value, _context: &PluginContext) -> anyhow::Result<String> {
        match tool_name {
            "calculate_position_size" => self.calc_position(args),
            "compute_risk_metrics" => self.calc_metrics(args),
            "check_risk" => self.check_risk(args),
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

impl RiskPlugin {
    fn calc_position(&self, args: &Value) -> anyhow::Result<String> {
        let equity = args["equity"].as_f64().ok_or_else(|| anyhow::anyhow!("equity required"))?;
        let entry = args["entry_price"].as_f64().ok_or_else(|| anyhow::anyhow!("entry_price required"))?;
        let stop = args["stop_loss"].as_f64().ok_or_else(|| anyhow::anyhow!("stop_loss required"))?;
        let method = args["method"].as_str().unwrap_or("fixed_percent");
        let risk_pct = args["risk_pct"].as_f64().unwrap_or(0.02);
        let config = bonbo_risk::models::RiskConfig::default();
        let sizing = match method {
            "kelly" => bonbo_risk::position_sizing::SizingMethod::Kelly { win_rate: 0.55, avg_win: 200.0, avg_loss: 100.0 },
            "half_kelly" => bonbo_risk::position_sizing::SizingMethod::HalfKelly { win_rate: 0.55, avg_win: 200.0, avg_loss: 100.0 },
            _ => bonbo_risk::position_sizing::SizingMethod::FixedPercent { pct: risk_pct },
        };
        let sizer = bonbo_risk::position_sizing::PositionSizer::new(sizing, config);
        let size = sizer.calculate(equity, entry, stop);
        let notional = size * entry;
        let risk_amt = size * (entry - stop).abs();
        Ok(format!("📐 **Position Size**\n\n💰 Equity: ${:.2}\n📈 Entry: ${:.2}\n🛑 Stop: ${:.2}\n📏 Risk/unit: ${:.2}\n\n**{}** → {:.6} units (${:.2})\nRisk: ${:.2} ({:.2}% equity)",
            equity, entry, stop, (entry-stop).abs(), method, size, notional, risk_amt, risk_amt/equity*100.0))
    }

    fn calc_metrics(&self, args: &Value) -> anyhow::Result<String> {
        let pnls: Vec<f64> = match args["trade_pnls"].as_array() {
            Some(a) => a.iter().filter_map(|v| v.as_f64()).collect(),
            None => anyhow::bail!("trade_pnls array required"),
        };
        let eq_curve: Vec<f64> = match args["equity_curve"].as_array() {
            Some(a) => a.iter().filter_map(|v| v.as_f64()).collect(),
            None => anyhow::bail!("equity_curve array required"),
        };
        let initial = args["initial_capital"].as_f64().unwrap_or(10000.0);
        if pnls.is_empty() || eq_curve.is_empty() { return Ok("⚠️ Empty data".to_string()); }
        let m = bonbo_risk::var::compute_portfolio_metrics(&pnls, &eq_curve, initial);
        Ok(format!("📊 **Risk Metrics**\n\n💰 Equity: ${:.2}\n📈 Return: {:.2}%\n\n**Risk:**\n  VaR(95%): {:.2}%\n  CVaR(95%): {:.2}%\n  Max DD: {:.2}%\n\n**Performance:**\n  Sharpe: {:.2}\n  Sortino: {:.2}\n  Win Rate: {:.1}%\n  Profit Factor: {:.2}\n  Avg Trade: ${:.2}",
            m.current_equity, m.total_return_pct*100.0, m.var_95*100.0, m.cvar_95*100.0, m.max_drawdown_pct*100.0,
            m.sharpe_ratio, m.sortino_ratio, m.win_rate*100.0, m.profit_factor, m.avg_trade_pnl))
    }

    fn check_risk(&self, args: &Value) -> anyhow::Result<String> {
        let equity = args["equity"].as_f64().ok_or_else(|| anyhow::anyhow!("equity required"))?;
        let initial = args["initial_capital"].as_f64().ok_or_else(|| anyhow::anyhow!("initial_capital required"))?;
        let daily_pnl = args["daily_pnl"].as_f64().unwrap_or(0.0);
        let peak = args["peak_equity"].as_f64().unwrap_or(initial);
        let consec = args["consecutive_losses"].as_u64().unwrap_or(0) as usize;
        let config = bonbo_risk::models::RiskConfig::default();
        let cb = bonbo_risk::circuit_breaker::CircuitBreaker::new(config);
        let portfolio = bonbo_risk::models::PortfolioState {
            equity, initial_capital: initial, peak_equity: peak, daily_pnl, total_pnl: equity - initial,
            open_positions_count: 0, consecutive_losses: consec, daily_start_equity: equity - daily_pnl, trades_today: 0,
        };
        let level = cb.check(&portfolio);
        let check = cb.can_trade(&portfolio);
        Ok(format!("🛡️ **Risk Check**\n\n💰 Equity: ${:.2}\n📊 Daily P&L: ${:.2}\n📉 DD: {:.2}%\n🔄 Losses: {}\n\n**Status**: {:?}\n**Can Trade**: {}\n**Reason**: {}",
            equity, daily_pnl, (peak-equity)/peak*100.0, consec, level,
            if check.allowed { "✅" } else { "❌" }, check.reason))
    }
}
