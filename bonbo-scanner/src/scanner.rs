//! Market Scanner — scans top crypto and generates scored results.

use crate::error::ScannerError;
use crate::models::*;

/// Market scanner that scores symbols based on combined analysis.
pub struct MarketScanner {
    config: ScanConfig,
}

impl MarketScanner {
    pub fn new(config: ScanConfig) -> Self {
        Self { config }
    }

    /// Generate scan report from pre-fetched analysis data.
    /// In production, this would call the MCP tools internally.
    /// For now, it accepts pre-computed data points.
    pub fn generate_report(
        &self,
        data_points: Vec<(String, f64, f64, String, Vec<String>, f64, f64)>,
        // (symbol, price, score, regime, signals, sentiment, bt_sharpe)
    ) -> Result<ScanReport, ScannerError> {
        let now = chrono::Utc::now().timestamp();

        let mut results: Vec<ScanResult> = data_points.into_iter().map(|(symbol, price, score, regime, signals, sentiment, bt_sharpe)| {
            let recommendation = if score >= 70.0 { "STRONG_BUY" }
                else if score >= 55.0 { "BUY" }
                else if score >= 40.0 { "HOLD" }
                else if score >= 25.0 { "SELL" }
                else { "STRONG_SELL" };

            ScanResult {
                symbol,
                price,
                regime,
                quant_score: score,
                recommendation: recommendation.to_string(),
                top_signals: signals,
                sentiment,
                backtest_sharpe: bt_sharpe,
                scan_timestamp: now,
            }
        }).collect();

        // Sort by score descending
        results.sort_by(|a, b| b.quant_score.partial_cmp(&a.quant_score).unwrap_or(std::cmp::Ordering::Equal));

        // Generate alerts
        let mut alerts = Vec::new();
        for r in &results {
            if r.quant_score >= self.config.min_score {
                alerts.push(format!(
                    "🎯 {} — Score: {:.0} ({}) | Regime: {} | Signals: {}",
                    r.symbol, r.quant_score, r.recommendation, r.regime,
                    r.top_signals.join(", ")
                ));
            }
        }

        let symbols_scanned = results.len() as u32;

        // Determine overall regime
        let overall_regime = results.first()
            .map(|r| r.regime.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        // Take top N
        results.truncate(self.config.max_results);

        Ok(ScanReport {
            timestamp: now,
            symbols_scanned,
            regime: overall_regime,
            top_picks: results,
            alerts,
        })
    }

    pub fn config(&self) -> &ScanConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_report() {
        let scanner = MarketScanner::new(ScanConfig::default());
        let data = vec![
            ("BTCUSDT".to_string(), 77000.0, 72.0, "Ranging".to_string(), vec!["RSI bullish".to_string()], 0.3, 1.5),
            ("ETHUSDT".to_string(), 2400.0, 55.0, "Ranging".to_string(), vec!["MACD crossover".to_string()], -0.2, 0.8),
            ("SOLUSDT".to_string(), 150.0, 35.0, "Volatile".to_string(), vec![], -0.5, -0.3),
        ];

        let report = scanner.generate_report(data).unwrap();
        assert_eq!(report.symbols_scanned, 3);
        assert_eq!(report.top_picks.len(), 3); // less than max_results
        assert_eq!(report.top_picks[0].symbol, "BTCUSDT"); // highest score first
        assert_eq!(report.alerts.len(), 2); // BTC + ETH above min_score
    }
}
