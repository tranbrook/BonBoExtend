//! Telegram alert integration for BonBo trading signals.
//!
//! Formats trading signals, regime changes, and scan results
//! into Telegram-friendly messages for the BonBo Telegram bot.

use serde::{Deserialize, Serialize};

/// Alert types that can be sent via Telegram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    /// New trading signal.
    Signal {
        symbol: String,
        action: String,
        price: f64,
        confidence: f64,
        reasoning: String,
    },
    /// Market regime change detected.
    RegimeChange {
        from: String,
        to: String,
        confidence: f64,
    },
    /// Price alert triggered.
    PriceAlert {
        symbol: String,
        target: f64,
        current: f64,
        direction: String,
    },
    /// Scan result with top picks.
    ScanResult {
        timestamp: i64,
        top_picks: Vec<String>,
        alerts: Vec<String>,
    },
    /// Risk warning.
    RiskWarning { message: String, level: String },
}

impl AlertType {
    /// Format the alert as a Telegram-friendly HTML message.
    pub fn to_telegram_html(&self) -> String {
        match self {
            AlertType::Signal {
                symbol,
                action,
                price,
                confidence,
                reasoning,
            } => {
                let emoji = match action.as_str() {
                    "STRONG_BUY" | "BUY" => "🟢",
                    "STRONG_SELL" | "SELL" => "🔴",
                    _ => "🟡",
                };
                format!(
                    "{emoji} <b>Trading Signal: {symbol}</b>\n\
                     Action: <b>{action}</b>\n\
                     Price: ${price:.2}\n\
                     Confidence: {confidence:.0}%\n\
                     📝 {reasoning}"
                )
            }
            AlertType::RegimeChange {
                from,
                to,
                confidence,
            } => {
                format!(
                    "🔄 <b>Regime Change Detected</b>\n\
                     {from} → {to}\n\
                     Confidence: {confidence:.0}%"
                )
            }
            AlertType::PriceAlert {
                symbol,
                target,
                current,
                direction,
            } => {
                let arrow = if direction == "above" { "📈" } else { "📉" };
                format!(
                    "{arrow} <b>Price Alert: {symbol}</b>\n\
                     Target: ${target:.2}\n\
                     Current: ${current:.2}\n\
                     Direction: {direction}"
                )
            }
            AlertType::ScanResult {
                timestamp,
                top_picks,
                alerts,
            } => {
                let time = chrono::DateTime::from_timestamp(*timestamp, 0)
                    .map(|t| t.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let picks = top_picks
                    .iter()
                    .take(5)
                    .enumerate()
                    .map(|(i, p)| format!("{}. {}", i + 1, p))
                    .collect::<Vec<_>>()
                    .join("\n");
                let alerts_text = if alerts.is_empty() {
                    "No alerts".to_string()
                } else {
                    alerts
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                format!(
                    "📊 <b>Market Scan Results</b> ({time})\n\n\
                     <b>Top Picks:</b>\n{picks}\n\n\
                     <b>Alerts:</b>\n{alerts_text}"
                )
            }
            AlertType::RiskWarning { message, level } => {
                let emoji = match level.as_str() {
                    "critical" => "🚨",
                    "warning" => "⚠️",
                    _ => "ℹ️",
                };
                format!("{emoji} <b>Risk Warning ({level})</b>\n{message}")
            }
        }
    }
}

/// TradingView PineScript export for strategies.
pub struct PineScriptExporter;

impl PineScriptExporter {
    /// Export an SMA Crossover strategy to PineScript v5.
    pub fn sma_crossover(fast: usize, slow: usize) -> String {
        format!(
            r#"//@version=5
strategy("BonBo SMA Crossover", overlay=true, initial_capital=10000)

// Parameters
fast_len = input.int({fast}, "Fast SMA Period")
slow_len = input.int({slow}, "Slow SMA Period")

// Indicators
fast_sma = ta.sma(close, fast_len)
slow_sma = ta.sma(close, slow_len)

// Plot
plot(fast_sma, "Fast SMA", color=color.blue, linewidth=2)
plot(slow_sma, "Slow SMA", color=color.orange, linewidth=2)

// Signals
bull_cross = ta.crossover(fast_sma, slow_sma)
bear_cross = ta.crossunder(fast_sma, slow_sma)

// Strategy
if bull_cross
    strategy.entry("Long", strategy.long)
if bear_cross
    strategy.close("Long")

// Visual
plotshape(bull_cross, "Buy", shape.triangleup, location.belowbar, color.green, size=size.small)
plotshape(bear_cross, "Sell", shape.triangledown, location.abovebar, color.red, size=size.small)
"#
        )
    }

    /// Export an RSI Mean Reversion strategy to PineScript v5.
    pub fn rsi_mean_reversion(period: usize, oversold: f64, overbought: f64) -> String {
        format!(
            r#"//@version=5
strategy("BonBo RSI Mean Reversion", overlay=false, initial_capital=10000)

// Parameters
rsi_len = input.int({period}, "RSI Period")
oversold_level = input.float({oversold}, "Oversold")
overbought_level = input.float({overbought}, "Overbought")

// Indicator
rsi_val = ta.rsi(close, rsi_len)

// Plot
plot(rsi_val, "RSI", color=color.purple, linewidth=2)
hline(oversold_level, "Oversold", color.green, linestyle=hline.style_dashed)
hline(overbought_level, "Overbought", color.red, linestyle=hline.style_dashed)

// Signals
buy_signal = ta.crossunder(rsi_val, oversold_level)
sell_signal = ta.crossover(rsi_val, overbought_level)

// Strategy
if buy_signal
    strategy.entry("Long", strategy.long)
if sell_signal
    strategy.close("Long")

// Visual
plotshape(buy_signal, "Buy", shape.triangleup, location.bottom, color.green)
plotshape(sell_signal, "Sell", shape.triangledown, location.top, color.red)
"#
        )
    }

    /// Export a MACD strategy to PineScript v5.
    pub fn macd_crossover(fast: usize, slow: usize, signal: usize) -> String {
        format!(
            r#"//@version=5
strategy("BonBo MACD Crossover", overlay=false, initial_capital=10000)

// Parameters
fast_len = input.int({fast}, "Fast EMA")
slow_len = input.int({slow}, "Slow EMA")
signal_len = input.int({signal}, "Signal")

// Indicator
[macd_line, signal_line, hist] = ta.macd(close, fast_len, slow_len, signal_len)

// Plot
plot(macd_line, "MACD", color=color.blue)
plot(signal_line, "Signal", color=color.orange)
plot(hist, "Histogram", style=plot.style_histogram, color=hist >= 0 ? color.green : color.red)

// Signals
bull_cross = ta.crossover(macd_line, signal_line)
bear_cross = ta.crossunder(macd_line, signal_line)

// Strategy
if bull_cross
    strategy.entry("Long", strategy.long)
if bear_cross
    strategy.close("Long")
"#
        )
    }

    /// Export Bollinger Bands strategy to PineScript v5.
    pub fn bollinger_bands(period: usize, std_dev: f64) -> String {
        format!(
            r#"//@version=5
strategy("BonBo Bollinger Bands", overlay=true, initial_capital=10000)

// Parameters
bb_len = input.int({period}, "BB Period")
bb_mult = input.float({std_dev}, "BB StdDev")

// Indicator
[middle, upper, lower] = ta.bb(close, bb_len, bb_mult)

// Plot
plot(middle, "Middle", color=color.gray)
plot(upper, "Upper", color=color.red)
plot(lower, "Lower", color=color.green)

// Strategy
if close <= lower
    strategy.entry("Long", strategy.long)
if close >= upper
    strategy.close("Long")
"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_alert_html() {
        let alert = AlertType::Signal {
            symbol: "BTCUSDT".into(),
            action: "BUY".into(),
            price: 77000.0,
            confidence: 75.0,
            reasoning: "RSI oversold + MACD bullish".into(),
        };
        let html = alert.to_telegram_html();
        assert!(html.contains("BTCUSDT"));
        assert!(html.contains("BUY"));
        assert!(html.contains("77000"));
    }

    #[test]
    fn test_regime_change_alert() {
        let alert = AlertType::RegimeChange {
            from: "Ranging".into(),
            to: "TrendingUp".into(),
            confidence: 0.85,
        };
        let html = alert.to_telegram_html();
        assert!(html.contains("Ranging"));
        assert!(html.contains("TrendingUp"));
    }

    #[test]
    fn test_price_alert() {
        let alert = AlertType::PriceAlert {
            symbol: "ETHUSDT".into(),
            target: 2500.0,
            current: 2510.0,
            direction: "above".into(),
        };
        let html = alert.to_telegram_html();
        assert!(html.contains("ETHUSDT"));
        assert!(html.contains("above"));
    }

    #[test]
    fn test_risk_warning() {
        let alert = AlertType::RiskWarning {
            message: "Drawdown exceeds 10%".into(),
            level: "warning".into(),
        };
        let html = alert.to_telegram_html();
        assert!(html.contains("Drawdown"));
    }

    #[test]
    fn test_scan_result() {
        let alert = AlertType::ScanResult {
            timestamp: 1700000000,
            top_picks: vec!["BTCUSDT: 85".into(), "ETHUSDT: 72".into()],
            alerts: vec!["🎯 BTCUSDT strong buy".into()],
        };
        let html = alert.to_telegram_html();
        assert!(html.contains("BTCUSDT"));
    }

    #[test]
    fn test_pine_script_sma() {
        let script = PineScriptExporter::sma_crossover(10, 30);
        assert!(script.contains("//@version=5"));
        assert!(script.contains("strategy("));
        assert!(script.contains("fast_len = input.int(10"));
    }

    #[test]
    fn test_pine_script_rsi() {
        let script = PineScriptExporter::rsi_mean_reversion(14, 30.0, 70.0);
        assert!(script.contains("//@version=5"));
        assert!(script.contains("rsi_val = ta.rsi"));
    }

    #[test]
    fn test_pine_script_macd() {
        let script = PineScriptExporter::macd_crossover(12, 26, 9);
        assert!(script.contains("ta.macd"));
    }

    #[test]
    fn test_pine_script_bb() {
        let script = PineScriptExporter::bollinger_bands(20, 2.0);
        assert!(script.contains("ta.bb"));
    }
}
