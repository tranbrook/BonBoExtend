//! Performance tracking — compute learning metrics from journal data.

use crate::error::JournalError;
use crate::journal::JournalStore;
use crate::models::*;
use std::collections::HashMap;

/// Tracks and computes learning metrics from journal entries.
pub struct PerformanceTracker<'a> {
    store: &'a JournalStore,
}

impl<'a> PerformanceTracker<'a> {
    pub fn new(store: &'a JournalStore) -> Self {
        Self { store }
    }

    /// Compute comprehensive learning metrics from all entries with outcomes.
    pub fn compute_metrics(&self) -> Result<LearningMetrics, JournalError> {
        let entries = self.store.get_entries_with_outcome(None)?;
        let with_outcome: Vec<_> = entries
            .into_iter()
            .filter(|e| e.outcome.is_some())
            .collect();

        if with_outcome.is_empty() {
            return Ok(LearningMetrics::default());
        }

        let total_with_outcome = with_outcome.len() as u32;
        let total_predictions = self.store.count_entries(&JournalQuery::default())?;

        // Direction accuracy
        let direction_correct = with_outcome
            .iter()
            .filter(|e| e.outcome.as_ref().unwrap().direction_correct)
            .count() as u32;
        let direction_accuracy = direction_correct as f64 / total_with_outcome as f64;

        // Score error
        let avg_score_error = with_outcome
            .iter()
            .map(|e| e.outcome.as_ref().unwrap().score_accuracy)
            .sum::<f64>()
            / total_with_outcome as f64;

        // Win rate (positive return)
        let winners = with_outcome
            .iter()
            .filter(|e| e.outcome.as_ref().unwrap().actual_return_pct > 0.0)
            .count() as u32;
        let win_rate = winners as f64 / total_with_outcome as f64;

        // Average return
        let avg_return_pct = with_outcome
            .iter()
            .map(|e| e.outcome.as_ref().unwrap().actual_return_pct)
            .sum::<f64>()
            / total_with_outcome as f64;

        // Sharpe of predictions (simplified annualized)
        let returns: Vec<f64> = with_outcome
            .iter()
            .map(|e| e.outcome.as_ref().unwrap().actual_return_pct / 100.0)
            .collect();
        let sharpe = compute_sharpe(&returns);

        // Profit factor
        let gross_profit: f64 = returns.iter().filter(|&&r| r > 0.0).sum();
        let gross_loss: f64 = returns.iter().filter(|&&r| r < 0.0).map(|r| r.abs()).sum();
        let profit_factor = if gross_loss > 0.0 {
            gross_profit / gross_loss
        } else if gross_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        // Per-indicator accuracy
        let mut indicator_stats: HashMap<String, (u32, u32)> = HashMap::new();
        for entry in &with_outcome {
            let outcome = entry.outcome.as_ref().unwrap();
            for (indicator, &correct) in &outcome.indicator_accuracy {
                let (total, correct_count) =
                    indicator_stats.entry(indicator.clone()).or_insert((0, 0));
                *total += 1;
                if correct {
                    *correct_count += 1;
                }
            }
        }
        let per_indicator_accuracy: HashMap<String, IndicatorAccuracy> = indicator_stats
            .into_iter()
            .map(|(name, (total, correct))| {
                (
                    name.clone(),
                    IndicatorAccuracy {
                        name,
                        total_signals: total,
                        correct_signals: correct,
                        accuracy: if total > 0 {
                            correct as f64 / total as f64
                        } else {
                            0.0
                        },
                    },
                )
            })
            .collect();

        // Per-regime accuracy
        let mut regime_stats: HashMap<String, (u32, u32, f64)> = HashMap::new();
        for entry in &with_outcome {
            let regime_str = format!("{:?}", entry.snapshot.market_regime);
            let outcome = entry.outcome.as_ref().unwrap();
            let (total, correct, sum_return) = regime_stats
                .entry(regime_str.clone())
                .or_insert((0, 0, 0.0));
            *total += 1;
            if outcome.direction_correct {
                *correct += 1;
            }
            *sum_return += outcome.actual_return_pct;
        }
        let per_regime_accuracy: HashMap<String, RegimeAccuracy> = regime_stats
            .into_iter()
            .map(|(regime, (total, correct, sum_return))| {
                (
                    regime.clone(),
                    RegimeAccuracy {
                        regime,
                        total_predictions: total,
                        correct_direction: correct,
                        accuracy: if total > 0 {
                            correct as f64 / total as f64
                        } else {
                            0.0
                        },
                        avg_return: if total > 0 {
                            sum_return / total as f64
                        } else {
                            0.0
                        },
                    },
                )
            })
            .collect();

        // Recent 10 accuracy
        let recent_10_accuracy = if with_outcome.len() >= 10 {
            let recent: Vec<_> = with_outcome.iter().rev().take(10).collect();
            let recent_correct = recent
                .iter()
                .filter(|e| e.outcome.as_ref().unwrap().direction_correct)
                .count();
            recent_correct as f64 / 10.0
        } else {
            direction_accuracy
        };

        Ok(LearningMetrics {
            total_predictions,
            total_with_outcome,
            direction_accuracy,
            avg_score_error,
            win_rate,
            avg_return_pct,
            sharpe_of_predictions: sharpe,
            profit_factor,
            per_indicator_accuracy,
            per_regime_accuracy,
            recent_10_accuracy,
        })
    }
}

/// Compute annualized Sharpe ratio from a slice of returns.
fn compute_sharpe(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    if std_dev < 1e-10 {
        return if mean > 0.0 { f64::INFINITY } else { 0.0 };
    }
    // Annualize assuming ~252 trading days, ~6 4h-candles per day
    let annualization_factor = (252.0_f64 * 6.0).sqrt();
    (mean / std_dev) * annualization_factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::JournalStore;

    fn make_entry_with_outcome(
        symbol: &str,
        return_pct: f64,
        direction_correct: bool,
    ) -> TradeJournalEntry {
        let mut entry = TradeJournalEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            snapshot: AnalysisSnapshot::default(),
            recommendation: if return_pct > 0.0 {
                Recommendation::Buy
            } else {
                Recommendation::Sell
            },
            entry_price: 50_000.0,
            stop_loss: 48_000.0,
            target_price: 54_000.0,
            risk_reward_ratio: 2.0,
            position_size_usd: 1000.0,
            outcome: None,
        };
        entry.snapshot.symbol = symbol.to_string();
        entry.snapshot.price = 50_000.0;
        entry.snapshot.market_regime = MarketRegime::Ranging;

        let outcome = TradeOutcome {
            close_timestamp: entry.timestamp + 86400,
            exit_price: 50_000.0 * (1.0 + return_pct / 100.0),
            actual_return_pct: return_pct,
            hit_target: return_pct >= 8.0,
            hit_stoploss: return_pct <= -4.0,
            holding_period_hours: 24,
            max_favorable_excursion: return_pct.max(0.0) * 1.2,
            max_adverse_excursion: return_pct.min(0.0) * 1.2,
            direction_correct,
            score_accuracy: return_pct.abs(),
            indicator_accuracy: {
                let mut m = HashMap::new();
                m.insert("RSI".to_string(), return_pct > 0.0);
                m.insert("MACD".to_string(), return_pct > 0.0);
                m.insert("BB".to_string(), return_pct < 0.0);
                m
            },
        };
        entry.outcome = Some(outcome);
        entry
    }

    #[test]
    fn test_compute_metrics() {
        let store = JournalStore::open_in_memory().unwrap();

        // Insert entries with pre-set outcomes
        let entries = vec![
            make_entry_with_outcome("BTCUSDT", 5.0, true),
            make_entry_with_outcome("ETHUSDT", -2.0, false),
            make_entry_with_outcome("SOLUSDT", 3.0, true),
            make_entry_with_outcome("BTCUSDT", -1.0, true), // short correct
            make_entry_with_outcome("ETHUSDT", 7.0, true),
        ];

        for e in &entries {
            store.insert_entry(e).unwrap();
        }

        let tracker = PerformanceTracker::new(&store);
        let metrics = tracker.compute_metrics().unwrap();

        assert_eq!(metrics.total_with_outcome, 5);
        assert_eq!(metrics.total_predictions, 5);
        assert!(metrics.direction_accuracy > 0.0);
        assert!(metrics.win_rate > 0.0);
        assert!(metrics.per_indicator_accuracy.contains_key("RSI"));
        assert!(metrics.per_regime_accuracy.contains_key("Ranging"));
    }

    #[test]
    fn test_compute_metrics_empty() {
        let store = JournalStore::open_in_memory().unwrap();
        let tracker = PerformanceTracker::new(&store);
        let metrics = tracker.compute_metrics().unwrap();
        assert_eq!(metrics.total_predictions, 0);
    }

    #[test]
    fn test_sharpe_calculation() {
        // All positive returns → positive Sharpe
        let returns = vec![0.01, 0.02, 0.015, 0.03, 0.005];
        let sharpe = compute_sharpe(&returns);
        assert!(sharpe > 0.0);

        // Empty → 0
        assert_eq!(compute_sharpe(&[]), 0.0);
    }
}
