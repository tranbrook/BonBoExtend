//! Market Scanner — scans top crypto and generates scored results.

use crate::error::ScannerError;
use crate::models::*;

/// Data point tuple for scanning: (symbol, price, score, regime, signals, sentiment, bt_sharpe)
pub type DataPoint = (String, f64, f64, String, Vec<String>, f64, f64);

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
    ///
    /// For now, it accepts pre-computed data points.
    #[allow(clippy::type_complexity)]
    pub fn generate_report(
        &self,
        data_points: Vec<DataPoint>,
        // (symbol, price, score, regime, signals, sentiment, bt_sharpe)
    ) -> Result<ScanReport, ScannerError> {
        let now = chrono::Utc::now().timestamp();

        let mut results: Vec<ScanResult> = data_points
            .into_iter()
            .map(
                |(symbol, price, score, regime, signals, sentiment, bt_sharpe)| {
                    let recommendation = if score >= 70.0 {
                        "STRONG_BUY"
                    } else if score >= 55.0 {
                        "BUY"
                    } else if score >= 40.0 {
                        "HOLD"
                    } else if score >= 25.0 {
                        "SELL"
                    } else {
                        "STRONG_SELL"
                    };

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
                },
            )
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| {
            b.quant_score
                .partial_cmp(&a.quant_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Generate alerts
        let mut alerts = Vec::new();
        for r in &results {
            if r.quant_score >= self.config.min_score {
                alerts.push(format!(
                    "🎯 {} — Score: {:.0} ({}) | Regime: {} | Signals: {}",
                    r.symbol,
                    r.quant_score,
                    r.recommendation,
                    r.regime,
                    r.top_signals.join(", ")
                ));
            }
        }

        let symbols_scanned = results.len() as u32;

        // Determine overall regime
        let overall_regime = results
            .first()
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

    /// Merge symbols from 3 tiers, deduplicate, and return unique list.
    ///
    /// Research source: trading-process-improvement.md — Enhancement #4.
    ///
    /// # Tiers
    /// 1. Top Volume: top N symbols by 24h volume (passed in)
    /// 2. Watchlist: user-configured symbols (from ScanConfig)
    /// 3. Hot Movers: symbols with |24h change| > threshold and volume > min
    ///
    /// # Arguments
    /// * `top_volume_symbols` — Top N symbols by 24h volume (from exchange API)
    /// * `hot_movers` — Hot movers detected from 24h change data
    pub fn merge_tiers(
        &self,
        top_volume_symbols: &[String],
        hot_movers: &[crate::models::HotMover],
        dynamic_config: &crate::models::DynamicScanConfig,
    ) -> Vec<(String, crate::models::ScanTier)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        // Tier 1: Top volume
        for symbol in top_volume_symbols
            .iter()
            .take(dynamic_config.top_volume_count)
        {
            if seen.insert(symbol.clone()) {
                result.push((symbol.clone(), crate::models::ScanTier::TopVolume));
            }
        }

        // Tier 2: Watchlist
        for symbol in &self.config.symbols {
            if seen.insert(symbol.clone()) {
                result.push((symbol.clone(), crate::models::ScanTier::Watchlist));
            }
        }

        // Tier 3: Hot movers
        let mut hot_sorted: Vec<_> = hot_movers
            .iter()
            .filter(|m| m.volume_24h >= dynamic_config.min_volume_usd)
            .filter(|m| m.change_24h_pct.abs() >= dynamic_config.hot_mover_min_change_pct)
            .collect();
        hot_sorted.sort_by(|a, b| {
            b.change_24h_pct
                .abs()
                .partial_cmp(&a.change_24h_pct.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for mover in hot_sorted.iter().take(dynamic_config.max_hot_movers) {
            if seen.insert(mover.symbol.clone()) {
                result.push((mover.symbol.clone(), crate::models::ScanTier::HotMovers));
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_report() {
        let scanner = MarketScanner::new(ScanConfig::default());
        let data = vec![
            (
                "BTCUSDT".to_string(),
                77000.0,
                72.0,
                "Ranging".to_string(),
                vec!["RSI bullish".to_string()],
                0.3,
                1.5,
            ),
            (
                "ETHUSDT".to_string(),
                2400.0,
                55.0,
                "Ranging".to_string(),
                vec!["MACD crossover".to_string()],
                -0.2,
                0.8,
            ),
            (
                "SOLUSDT".to_string(),
                150.0,
                35.0,
                "Volatile".to_string(),
                vec![],
                -0.5,
                -0.3,
            ),
        ];

        let report = scanner.generate_report(data).unwrap();
        assert_eq!(report.symbols_scanned, 3);
        assert_eq!(report.top_picks.len(), 3); // less than max_results
        assert_eq!(report.top_picks[0].symbol, "BTCUSDT"); // highest score first
        assert_eq!(report.alerts.len(), 2); // BTC + ETH above min_score
    }

    #[test]
    fn test_merge_tiers_basic() {
        let scanner = MarketScanner::new(ScanConfig::default());
        let dynamic_config = crate::models::DynamicScanConfig::default();

        let top_volume = vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "SOLUSDT".to_string(),
        ];
        let hot_movers = vec![
            crate::models::HotMover {
                symbol: "PEPEUSDT".to_string(),
                price: 0.01,
                change_24h_pct: 15.0,
                volume_24h: 5_000_000.0,
                tier: crate::models::ScanTier::HotMovers,
            },
            crate::models::HotMover {
                symbol: "XRPUSDT".to_string(),
                price: 0.5,
                change_24h_pct: -8.0,
                volume_24h: 10_000_000.0,
                tier: crate::models::ScanTier::HotMovers,
            },
        ];

        let merged = scanner.merge_tiers(&top_volume, &hot_movers, &dynamic_config);

        // Should include top volume + watchlist (20 default) + hot movers
        assert!(merged.len() >= 5);
        // BTC should be TopVolume tier
        assert!(
            merged
                .iter()
                .any(|(s, t)| s == "BTCUSDT" && *t == crate::models::ScanTier::TopVolume)
        );
        // PEPE should be HotMovers tier
        assert!(
            merged
                .iter()
                .any(|(s, t)| s == "PEPEUSDT" && *t == crate::models::ScanTier::HotMovers)
        );
        // No duplicates
        let symbols: Vec<&str> = merged.iter().map(|(s, _)| s.as_str()).collect();
        let unique: std::collections::HashSet<&str> = symbols.iter().copied().collect();
        assert_eq!(symbols.len(), unique.len());
    }

    #[test]
    fn test_merge_tiers_no_hot_movers() {
        let scanner = MarketScanner::new(ScanConfig::default());
        let dynamic_config = crate::models::DynamicScanConfig::default();

        // Hot movers below threshold
        let hot_movers = vec![crate::models::HotMover {
            symbol: "LOWVOL".to_string(),
            price: 1.0,
            change_24h_pct: 2.0,   // below 5% threshold
            volume_24h: 100_000.0, // below min volume
            tier: crate::models::ScanTier::HotMovers,
        }];

        let merged = scanner.merge_tiers(&[], &hot_movers, &dynamic_config);
        // Only watchlist symbols (no top volume, no qualifying hot movers)
        assert!(
            merged
                .iter()
                .all(|(_, t)| *t == crate::models::ScanTier::Watchlist)
        );
    }

    #[test]
    fn test_merge_tiers_deduplication() {
        let mut config = ScanConfig::default();
        config.symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        let scanner = MarketScanner::new(config);
        let dynamic_config = crate::models::DynamicScanConfig::default();

        // BTC appears in both top volume and watchlist
        let top_volume = vec!["BTCUSDT".to_string()];
        let hot_movers = vec![];

        let merged = scanner.merge_tiers(&top_volume, &hot_movers, &dynamic_config);
        let btc_count = merged.iter().filter(|(s, _)| s == "BTCUSDT").count();
        assert_eq!(btc_count, 1, "BTC should appear only once (dedup)");
    }
}
