//! Backtest Plugin — exposes bonbo-quant backtesting via MCP tools.

use crate::plugin::{ParameterSchema, PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct BacktestPlugin {
    metadata: PluginMetadata,
}
impl Default for BacktestPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl BacktestPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-backtest".into(),
                name: "Backtesting Engine".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Backtest trading strategies on historical data".into(),
                author: "BonBo Team".into(),
                tags: vec!["backtest".into(), "strategy".into()],
            },
        }
    }
}

#[async_trait]
impl ToolPlugin for BacktestPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "run_backtest".into(),
                description: "Run a backtest of a trading strategy on historical crypto data"
                    .into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "interval".into(),
                        param_type: "string".into(),
                        description: "Candle interval".into(),
                        required: false,
                        default: Some(json!("1d")),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "strategy".into(),
                        param_type: "string".into(),
                        description: "Strategy name".into(),
                        required: true,
                        default: None,
                        r#enum: Some(vec![
                            "sma_crossover".into(),
                            "rsi_mean_reversion".into(),
                            "bollinger_bands".into(),
                            "momentum".into(),
                            "breakout".into(),
                            "macd_crossover".into(),
                            "grid_trading".into(),
                            "dca".into(),
                        ]),
                    },
                    ParameterSchema {
                        name: "initial_capital".into(),
                        param_type: "number".into(),
                        description: "Starting capital USDT".into(),
                        required: false,
                        default: Some(json!(10000)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "fast_period".into(),
                        param_type: "integer".into(),
                        description: "Fast period".into(),
                        required: false,
                        default: Some(json!(10)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "slow_period".into(),
                        param_type: "integer".into(),
                        description: "Slow period".into(),
                        required: false,
                        default: Some(json!(30)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "list_strategies".into(),
                description: "List all available trading strategies with metadata".into(),
                parameters: vec![],
            },
            ToolSchema {
                name: "compare_strategies".into(),
                description: "Compare multiple strategies on the same data".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "interval".into(),
                        param_type: "string".into(),
                        description: "Candle interval".into(),
                        required: false,
                        default: Some(json!("1d")),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "strategies".into(),
                        param_type: "string".into(),
                        description: "Comma-separated strategy names".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "export_pinescript".into(),
                description: "Export a strategy as TradingView PineScript v5 code".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "strategy".into(),
                        param_type: "string".into(),
                        description: "Strategy to export".into(),
                        required: true,
                        default: None,
                        r#enum: Some(vec![
                            "sma_crossover".into(),
                            "rsi_mean_reversion".into(),
                            "macd_crossover".into(),
                            "bollinger_bands".into(),
                        ]),
                    },
                    ParameterSchema {
                        name: "fast_period".into(),
                        param_type: "integer".into(),
                        description: "Fast period".into(),
                        required: false,
                        default: Some(json!(10)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "slow_period".into(),
                        param_type: "integer".into(),
                        description: "Slow period".into(),
                        required: false,
                        default: Some(json!(30)),
                        r#enum: None,
                    },
                ],
            },
        ]
    }
    async fn execute_tool(
        &self,
        tool_name: &str,
        args: &Value,
        _context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "run_backtest" => self.run_backtest(args).await,
            "list_strategies" => self.list_strategies(),
            "compare_strategies" => self.compare_strategies(args).await,
            "export_pinescript" => self.export_pinescript(args),
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

impl BacktestPlugin {
    async fn run_backtest(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let strategy_name = args["strategy"].as_str().unwrap_or("sma_crossover");
        let initial_capital = args["initial_capital"].as_f64().unwrap_or(10000.0);
        let fast = args["fast_period"].as_u64().unwrap_or(10) as usize;
        let slow = args["slow_period"].as_u64().unwrap_or(30) as usize;

        let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();
        let raw = fetcher.fetch_klines(symbol, interval, Some(200)).await?;
        let candles: Vec<bonbo_ta::models::OhlcvCandle> = raw
            .into_iter()
            .map(|c| bonbo_ta::models::OhlcvCandle {
                timestamp: c.timestamp,
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect();
        if candles.len() < 50 {
            return Ok("⚠️ Not enough data (need 50+)".to_string());
        }

        let config = bonbo_quant::models::BacktestConfig {
            initial_capital,
            ..Default::default()
        };
        let report = match strategy_name {
            "sma_crossover" => {
                let s = bonbo_quant::strategy::SmaCrossoverStrategy::new(fast, slow);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "rsi_mean_reversion" => {
                let s =
                    bonbo_quant::strategy::RsiMeanReversionStrategy::new(fast.max(14), 30.0, 70.0);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            // ── Financial-Hacker Strategies ──
            "alma_crossover" => {
                let s = bonbo_quant::AlmaCrossoverStrategy::new();
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "laguerre_rsi" => {
                let s = bonbo_quant::LaguerreRsiStrategy::new(0.8)
                    .ok_or_else(|| anyhow::anyhow!("Invalid gamma for LaguerreRSI"))?;
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "cmo_momentum" => {
                let s = bonbo_quant::CmoMomentumStrategy::new(fast.max(14))
                    .ok_or_else(|| anyhow::anyhow!("Invalid period for CMO"))?;
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "fh_composite" => {
                let s = bonbo_quant::FhCompositeStrategy::new()
                    .ok_or_else(|| anyhow::anyhow!("FH Composite init failed"))?;
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "ehlers_trend" => {
                let s = bonbo_quant::EhlersTrendStrategy::new();
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "enhanced_mean_reversion" => {
                let s = bonbo_quant::EnhancedMeanReversionStrategy::new();
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "bollinger_bands" => {
                let s = bonbo_quant::strategy::BollingerBandsStrategy::new(20, 2.0);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "macd_crossover" => {
                let s = bonbo_quant::strategy::MacdCrossoverStrategy::new(12, 26, 9);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "momentum" => {
                let s = bonbo_quant::strategy::MomentumStrategy::new(10, 5.0);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "breakout" => {
                let s = bonbo_quant::strategy::BreakoutStrategy::new(20);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            "ema_crossover" => {
                let s = bonbo_quant::strategy::EmaCrossoverStrategy::new(12, 26);
                let mut eng = bonbo_quant::engine::BacktestEngine::new(config, s);
                eng.run(&candles)?
            }
            _ => anyhow::bail!("Unknown strategy: {}", strategy_name),
        };
        Ok(report.format_report())
    }

    fn list_strategies(&self) -> anyhow::Result<String> {
        let strategies = bonbo_quant::strategies::list_strategies();
        let mut result = String::from("📊 **Available Trading Strategies**\n\n");
        for s in &strategies {
            result.push_str(&format!(
                "• **{}** ({}) — {}\n  Best regime: {} | Params: {}\n\n",
                s.name,
                s.category,
                s.description,
                s.best_regime,
                s.parameters.join(", ")
            ));
        }
        result.push_str(&format!("\nTotal: {} strategies", strategies.len()));
        Ok(result)
    }

    async fn compare_strategies(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let strategies_str = args["strategies"]
            .as_str()
            .unwrap_or("sma_crossover,rsi_mean_reversion");

        let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();
        let raw = fetcher.fetch_klines(symbol, interval, Some(200)).await?;
        let candles: Vec<bonbo_ta::models::OhlcvCandle> = raw
            .into_iter()
            .map(|c| bonbo_ta::models::OhlcvCandle {
                timestamp: c.timestamp,
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect();
        if candles.len() < 50 {
            return Ok("⚠️ Not enough data (need 50+)".to_string());
        }

        let config = bonbo_quant::models::BacktestConfig::default();
        let mut results = Vec::new();

        for name in strategies_str.split(',') {
            let name = name.trim();
            let report = match name {
                "sma_crossover" => {
                    let s = bonbo_quant::strategy::SmaCrossoverStrategy::new(10, 30);
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "rsi_mean_reversion" => {
                    let s = bonbo_quant::strategy::RsiMeanReversionStrategy::new(14, 30.0, 70.0);
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "bollinger_bands" => {
                    let s = bonbo_quant::strategies::BollingerBandsStrategy::new(20, 2.0);
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "momentum" => {
                    let s = bonbo_quant::strategies::MomentumStrategy::new(10, 0.02);
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "breakout" => {
                    let s = bonbo_quant::strategies::BreakoutStrategy::new(20);
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "macd_crossover" => {
                    let s = bonbo_quant::strategies::MacdStrategy::new(12, 26, 9);
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                // ── Financial-Hacker Strategies ──
                "alma_crossover" => {
                    let s = bonbo_quant::AlmaCrossoverStrategy::new();
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "laguerre_rsi" => {
                    let s = match bonbo_quant::LaguerreRsiStrategy::new(0.8) {
                        Some(s) => s,
                        None => { continue; }
                    };
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "cmo_momentum" => {
                    let s = match bonbo_quant::CmoMomentumStrategy::new(14) {
                        Some(s) => s,
                        None => { continue; }
                    };
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "fh_composite" => {
                    let s = match bonbo_quant::FhCompositeStrategy::new() {
                        Some(s) => s,
                        None => { continue; }
                    };
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "ehlers_trend" => {
                    let s = bonbo_quant::EhlersTrendStrategy::new();
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                "enhanced_mean_reversion" => {
                    let s = bonbo_quant::EnhancedMeanReversionStrategy::new();
                    let mut eng = bonbo_quant::engine::BacktestEngine::new(config.clone(), s);
                    eng.run(&candles).ok()
                }
                _ => None,
            };
            if let Some(r) = report {
                results.push((
                    name.to_string(),
                    r.total_return_pct,
                    r.win_rate,
                    r.total_trades,
                ));
            }
        }

        if results.is_empty() {
            return Ok("⚠️ No valid strategies to compare".to_string());
        }

        // Sort by total return descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut output = format!("📊 **Strategy Comparison: {} ({})**\n\n", symbol, interval);
        output.push_str("| Strategy | Return% | Win Rate | Trades |\n");
        output.push_str("|----------|---------|----------|--------|\n");
        for (name, ret, wr, trades) in &results {
            output.push_str(&format!(
                "| {} | {:.2}% | {:.0}% | {} |\n",
                name,
                ret,
                wr * 100.0,
                trades
            ));
        }

        let best = &results[0];
        output.push_str(&format!("\n🏆 **Best**: {} ({:.2}%)", best.0, best.1));
        Ok(output)
    }

    fn export_pinescript(&self, args: &Value) -> anyhow::Result<String> {
        let strategy = args["strategy"].as_str().unwrap_or("sma_crossover");
        let fast = args["fast_period"].as_u64().unwrap_or(10) as usize;
        let slow = args["slow_period"].as_u64().unwrap_or(30) as usize;

        let script = match strategy {
            "sma_crossover" => crate::integration::PineScriptExporter::sma_crossover(fast, slow),
            "rsi_mean_reversion" => {
                crate::integration::PineScriptExporter::rsi_mean_reversion(fast, 30.0, 70.0)
            }
            "macd_crossover" => {
                crate::integration::PineScriptExporter::macd_crossover(fast, slow, 9)
            }
            "bollinger_bands" => crate::integration::PineScriptExporter::bollinger_bands(fast, 2.0),
            _ => {
                return Ok(format!(
                    "⚠️ Unknown strategy for PineScript export: {}",
                    strategy
                ));
            }
        };

        Ok(format!(
            "📝 **PineScript v5 Code ({})**\n\n```pine\n{}\n```",
            strategy, script
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plugin() -> BacktestPlugin {
        BacktestPlugin::new()
    }

    #[tokio::test]
    async fn test_run_backtest_sma_crossover() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "sma_crossover"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "SMA crossover should succeed: {:?}", result);
        let text = result.unwrap();
        assert!(text.contains("Total Return"), "Should contain Total Return");
        assert!(text.contains("Win Rate"), "Should contain Win Rate");
    }

    #[tokio::test]
    async fn test_run_backtest_alma_crossover() {
        let p = plugin();
        let args = json!({
            "symbol": "ETHUSDT",
            "interval": "1h",
            "strategy": "alma_crossover"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "ALMA crossover should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_backtest_laguerre_rsi() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "laguerre_rsi"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "LaguerreRSI should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_backtest_cmo_momentum() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "cmo_momentum"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "CMO Momentum should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_backtest_fh_composite() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "fh_composite"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "FH Composite should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_backtest_ehlers_trend() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "ehlers_trend"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "Ehlers Trend should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_backtest_enhanced_mean_reversion() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "enhanced_mean_reversion"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_ok(), "Enhanced Mean Reversion should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_backtest_unknown_strategy() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategy": "nonexistent_strategy"
        });
        let result = p.run_backtest(&args).await;
        assert!(result.is_err(), "Unknown strategy should return error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unknown strategy"),
            "Error should mention unknown strategy: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_compare_strategies() {
        let p = plugin();
        let args = json!({
            "symbol": "BTCUSDT",
            "interval": "1h",
            "strategies": "sma_crossover,alma_crossover,laguerre_rsi,fh_composite"
        });
        let result = p.compare_strategies(&args).await;
        assert!(result.is_ok(), "compare_strategies should succeed: {:?}", result);
        let text = result.unwrap();
        // Should contain all specified strategies
        assert!(text.contains("sma_crossover"), "Should list sma_crossover");
        assert!(text.contains("alma_crossover"), "Should list alma_crossover (FH)");
        assert!(text.contains("laguerre_rsi"), "Should list laguerre_rsi (FH)");
    }

    #[tokio::test]
    async fn test_all_fh_strategies_registered() {
        // Verify all FH strategy names are accepted
        let p = plugin();
        let fh_strategies = [
            "alma_crossover",
            "laguerre_rsi",
            "cmo_momentum",
            "fh_composite",
            "ehlers_trend",
            "enhanced_mean_reversion",
        ];

        for strat in &fh_strategies {
            let args = json!({
                "symbol": "BTCUSDT",
                "interval": "1h",
                "strategy": strat
            });
            let result = p.run_backtest(&args).await;
            assert!(
                result.is_ok(),
                "Strategy '{}' should be registered and work: {:?}",
                strat,
                result
            );
        }
    }
}
