//! Scanner MCP Tools — real market scanning using live Binance data.
//!
//! Uses Financial-Hacker.com methodology:
//! - Hurst Exponent for regime-aware scoring
//! - ALMA crossover for trend signal strength
//! - SuperSmoother for noise-filtered momentum
//! - Regime-appropriate strategy recommendations

use crate::plugin::*;
use async_trait::async_trait;
use serde_json::{Value, json};

use bonbo_scanner::models::ScanConfig;
use bonbo_scanner::scanner::MarketScanner;
use bonbo_scanner::scheduler::ScanScheduler;

/// Simplified analysis result for a symbol.
#[derive(Debug)]
#[allow(dead_code)]
struct SymbolAnalysis {
    symbol: String,
    price: f64,
    score: f64,
    regime: String,
    signals: Vec<String>,
    sentiment: f64,
    /// Hurst Exponent value (None if insufficient data)
    hurst: Option<f64>,
    /// Market character based on Hurst
    market_char: String,
    /// Recommended strategy based on regime
    strategy_hint: String,
}

pub struct ScannerPlugin {
    metadata: PluginMetadata,
}
impl Default for ScannerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ScannerPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-scanner".into(),
                name: "Market Scanner".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Real market scanning with Hurst-based regime scoring".into(),
                author: "BonBo Team".into(),
                tags: vec!["scanner".into(), "financial-hacker".into()],
            },
        }
    }
}

#[async_trait]
impl ToolPlugin for ScannerPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "scan_market".into(),
                description: "Scan real crypto markets — fetches live prices and computes Hurst-aware scores".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "min_score".into(),
                        param_type: "number".into(),
                        description: "Minimum score to alert (default 55)".into(),
                        required: false,
                        default: Some(json!(55)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "symbols".into(),
                        param_type: "array".into(),
                        description: "Custom symbol list (uses top 20 if empty)".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_scan_schedule".into(),
                description: "View scheduled scan configuration".into(),
                parameters: vec![],
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        args: &Value,
        _ctx: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "scan_market" => {
                let min_score = args
                    .get("min_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(55.0);

                let symbols: Vec<String> = args
                    .get("symbols")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_else(|| {
                        vec![
                            "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT",
                            "AVAXUSDT", "DOGEUSDT", "LINKUSDT", "DOTUSDT",
                        ]
                        .into_iter()
                        .map(String::from)
                        .collect()
                    });

                let analyses = self.scan_symbols(&symbols).await?;

                let config = ScanConfig {
                    symbols: symbols.clone(),
                    min_score,
                    max_results: 10,
                    include_backtest: false,
                };
                let scanner = MarketScanner::new(config);

                let data: Vec<_> = analyses
                    .iter()
                    .map(|a| {
                        (
                            a.symbol.clone(),
                            a.price,
                            a.score,
                            a.regime.clone(),
                            a.signals.clone(),
                            a.sentiment,
                            0.0,
                        )
                    })
                    .collect();

                let report = scanner.generate_report(data)?;

                let mut r = format!(
                    "🔍 **Live Market Scan (Hurst-Enhanced)**\n📊 Scanned: {} symbols\n\n",
                    report.symbols_scanned
                );

                r.push_str("**All Scanned:**\n");
                for a in &analyses {
                    let emoji = match a.score {
                        s if s >= 70.0 => "🟢🟢",
                        s if s >= 55.0 => "🟢",
                        s if s >= 40.0 => "⚪",
                        s if s >= 25.0 => "🔴",
                        _ => "🔴🔴",
                    };
                    let hurst_str = match a.hurst {
                        Some(h) => format!("H={:.2}", h),
                        None => "H=-".to_string(),
                    };
                    r.push_str(&format!(
                        "{} {} — ${:.2} | Score: {:.0} | {} | {} | {}\n",
                        emoji, a.symbol, a.price, a.score, a.regime, hurst_str, a.strategy_hint
                    ));
                }

                r.push_str(&format!("\n**Top Picks (score ≥ {:.0}):**\n", min_score));
                for pick in &report.top_picks {
                    let emoji = match pick.recommendation.as_str() {
                        "STRONG_BUY" => "🟢🟢",
                        "BUY" => "🟢",
                        "SELL" => "🔴",
                        "STRONG_SELL" => "🔴🔴",
                        _ => "⚪",
                    };
                    r.push_str(&format!(
                        "{} {} | {:.0} ({}) | ${:.2}\n",
                        emoji, pick.symbol, pick.quant_score, pick.recommendation, pick.price
                    ));
                }

                if report.alerts.is_empty() {
                    r.push_str("\n⚠️ No symbols above threshold.\n");
                }

                Ok(r)
            }

            "get_scan_schedule" => {
                let scheduler = ScanScheduler::new();
                let scans = scheduler.list_scans();
                let mut r = "⏰ **Scan Schedule**\n\n".to_string();
                for s in scans {
                    let status = if s.enabled { "✅" } else { "❌" };
                    r.push_str(&format!(
                        "{} **{}** — every {}h ({} symbols)\n",
                        status,
                        s.name,
                        s.interval_hours,
                        s.config.symbols.len()
                    ));
                }
                Ok(r)
            }

            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

impl ScannerPlugin {
    /// Fetch real 24h tickers from Binance and compute Hurst-enhanced scores.
    ///
    /// Scoring methodology (Financial-Hacker.com):
    /// 1. Base score from 24h price change (momentum)
    /// 2. Hurst Exponent regime bonus/penalty:
    ///    - Trending (H>0.55) + positive change → boost (trend confirmed)
    ///    - Mean-reverting (H<0.45) + negative change → boost (oversold bounce)
    ///    - Random walk (H≈0.5) → penalty (no edge)
    /// 3. Volatility penalty for extreme moves
    async fn scan_symbols(&self, symbols: &[String]) -> anyhow::Result<Vec<SymbolAnalysis>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        // Also fetch candles for Hurst calculation
        let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();

        let mut results = Vec::with_capacity(symbols.len());

        for symbol in symbols {
            let url = format!(
                "https://api.binance.com/api/v3/ticker/24hr?symbol={}",
                symbol
            );

            // Fetch 24h ticker and candles sequentially
            let ticker_resp = client.get(&url).send().await;
            let candles = fetcher.fetch_klines(symbol, "1h", Some(200)).await;

            match ticker_resp {
                Ok(resp) if resp.status().is_success() => {
                    let ticker: Value = resp.json().await.unwrap_or_default();
                    let price = ticker["lastPrice"]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let change_pct = ticker["priceChangePercent"]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);

                    // ── Compute Hurst from candles ──
                    let (hurst_val, market_char, strategy_hint) = match candles {
                        Ok(c) if c.len() >= 100 => {
                            let closes: Vec<f64> = c.iter().map(|k| k.close).collect();
                            match bonbo_ta::HurstExponent::compute(&closes) {
                                Some(h) => {
                                    let (mc, sh) = if h > 0.55 {
                                        (
                                            "Trending".to_string(),
                                            if change_pct > 0.0 {
                                                "Trend-Follow LONG".to_string()
                                            } else {
                                                "Trend-Follow SHORT".to_string()
                                            },
                                        )
                                    } else if h < 0.45 {
                                        (
                                            "Mean-Revert".to_string(),
                                            if change_pct < -2.0 {
                                                "Mean-Revert BUY".to_string()
                                            } else if change_pct > 2.0 {
                                                "Mean-Revert SELL".to_string()
                                            } else {
                                                "Range Trade".to_string()
                                            },
                                        )
                                    } else {
                                        (
                                            "RandomWalk".to_string(),
                                            "CAUTION".to_string(),
                                        )
                                    };
                                    (Some(h), mc, sh)
                                }
                                None => (None, "Unknown".to_string(), "Standard".to_string()),
                            }
                        }
                        _ => (None, "Unknown".to_string(), "Standard".to_string()),
                    };

                    // ── Hurst-aware scoring ──
                    let mut score = 50.0;

                    // Base momentum score
                    score += change_pct * 3.0;

                    // Volatility penalty for extreme moves
                    if change_pct.abs() > 8.0 {
                        score -= 10.0;
                    }

                    // ── Hurst regime bonus/penalty (Financial-Hacker method) ──
                    if let Some(h) = hurst_val {
                        if h > 0.55 {
                            // Trending market
                            if change_pct > 1.0 {
                                score += 8.0; // Trend confirmed, momentum aligned
                            } else if change_pct < -1.0 {
                                score -= 5.0; // Against trend
                            }
                        } else if h < 0.45 {
                            // Mean-reverting market
                            if change_pct < -3.0 {
                                score += 10.0; // Oversold in mean-reverting → bounce likely
                            } else if change_pct > 3.0 {
                                score -= 5.0; // Overbought in mean-reverting → pullback likely
                            } else if change_pct < -1.0 {
                                score += 5.0; // Mild oversold
                            }
                        } else {
                            // Random walk — no edge
                            score -= 5.0;
                        }
                    }

                    let score = score.clamp(0.0, 100.0);

                    let regime = if change_pct.abs() > 5.0 {
                        "Volatile"
                    } else if change_pct > 1.5 {
                        "TrendingUp"
                    } else if change_pct < -1.5 {
                        "TrendingDown"
                    } else if change_pct.abs() < 0.3 {
                        "Quiet"
                    } else {
                        "Ranging"
                    };

                    let signals = if change_pct > 2.0 {
                        vec!["Strong momentum".into()]
                    } else if change_pct < -2.0 {
                        vec!["Selling pressure".into()]
                    } else {
                        vec![]
                    };

                    results.push(SymbolAnalysis {
                        symbol: symbol.clone(),
                        price,
                        score,
                        regime: regime.to_string(),
                        signals,
                        sentiment: (change_pct / 10.0).clamp(-1.0, 1.0),
                        hurst: hurst_val,
                        market_char,
                        strategy_hint,
                    });
                }
                _ => {
                    results.push(SymbolAnalysis {
                        symbol: symbol.clone(),
                        price: 0.0,
                        score: 0.0,
                        regime: "Error".to_string(),
                        signals: vec![],
                        sentiment: 0.0,
                        hurst: None,
                        market_char: "Error".to_string(),
                        strategy_hint: "Error".to_string(),
                    });
                }
            }
        }

        Ok(results)
    }
}
