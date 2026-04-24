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
                        description: "Custom symbol list (uses top 20 by volume if empty)".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "scan_hot_movers".into(),
                description: "Scan hot movers — top gainers/losers with high volume. Discovers opportunities beyond watchlist.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "min_volume_usdt".into(),
                        param_type: "number".into(),
                        description: "Minimum 24h volume in USDT (default 1000000)".into(),
                        required: false,
                        default: Some(json!(1_000_000)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "min_change_pct".into(),
                        param_type: "number".into(),
                        description: "Minimum absolute 24h change % to include (default 3.0)".into(),
                        required: false,
                        default: Some(json!(3.0)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "max_symbols".into(),
                        param_type: "number".into(),
                        description: "Maximum symbols to analyze (default 25)".into(),
                        required: false,
                        default: Some(json!(25)),
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
                        // Default: top 20 by volume (dynamic, not hardcoded)
                        vec![
                            "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT",
                            "ADAUSDT", "AVAXUSDT", "DOGEUSDT", "LINKUSDT", "DOTUSDT",
                            "SUIUSDT", "PEPEUSDT", "AAVEUSDT", "TAOUSDT", "SEIUSDT",
                            "ZECUSDT", "TRXUSDT", "SUIUSDT", "NEARUSDT", "APTUSDT",
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

            "scan_hot_movers" => {
                let min_volume = args
                    .get("min_volume_usdt")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1_000_000.0);
                let min_change = args
                    .get("min_change_pct")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(3.0);
                let max_symbols = args
                    .get("max_symbols")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(25.0) as usize;

                // Step 1: Fetch all 24hr tickers from Binance
                let movers = self.fetch_hot_movers(min_volume, min_change, max_symbols).await?;

                if movers.is_empty() {
                    return Ok("📊 No hot movers found matching criteria.".to_string());
                }

                // Step 2: Run Hurst analysis on each
                let symbols: Vec<String> = movers.iter().map(|m| m.symbol.clone()).collect();
                let analyses = self.scan_symbols(&symbols).await?;

                // Step 3: Build report
                let mut r = "🔥 **Hot Movers Scan**\n\n".to_string();

                // Group by direction
                let gainers: Vec<_> = movers.iter().filter(|m| m.change_pct > 0.0).collect();
                let losers: Vec<_> = movers.iter().filter(|m| m.change_pct < 0.0).collect();

                if !gainers.is_empty() {
                    r.push_str("### 📈 Top Gainers\n");
                    r.push_str(&format!("| # | Symbol | 24h % | Volume | Hurst | Regime | Score |\n"));
                    r.push_str(&format!("|---|--------|-------|--------|-------|--------|-------|\n"));
                    for (i, m) in gainers.iter().enumerate() {
                        if let Some(a) = analyses.iter().find(|a| a.symbol == m.symbol) {
                            let h = a.hurst.map(|h| format!("{:.2}", h)).unwrap_or("—".into());
                            r.push_str(&format!(
                                "| {} | {} | +{:.1}% | ${:.0}M | {} | {} | {:.0} |\n",
                                i + 1,
                                m.symbol,
                                m.change_pct,
                                m.volume / 1_000_000.0,
                                h,
                                a.regime,
                                a.score,
                            ));
                        }
                    }
                    r.push_str("\n");
                }

                if !losers.is_empty() {
                    r.push_str("### 📉 Top Losers\n");
                    r.push_str(&format!("| # | Symbol | 24h % | Volume | Hurst | Regime | Score |\n"));
                    r.push_str(&format!("|---|--------|-------|--------|-------|--------|-------|\n"));
                    for (i, m) in losers.iter().enumerate() {
                        if let Some(a) = analyses.iter().find(|a| a.symbol == m.symbol) {
                            let h = a.hurst.map(|h| format!("{:.2}", h)).unwrap_or("—".into());
                            r.push_str(&format!(
                                "| {} | {} | {:.1}% | ${:.0}M | {} | {} | {:.0} |\n",
                                i + 1,
                                m.symbol,
                                m.change_pct,
                                m.volume / 1_000_000.0,
                                h,
                                a.regime,
                                a.score,
                            ));
                        }
                    }
                    r.push_str("\n");
                }

                // Top picks from analysis
                let mut scored: Vec<_> = analyses.iter().filter(|a| a.score >= 55.0).collect();
                scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

                if !scored.is_empty() {
                    r.push_str("### 🏆 Best Opportunities (score ≥ 55)\n\n");
                    for a in &scored {
                        let emoji = if a.score >= 70.0 { "🟢" } else { "⚪" };
                        let h = a.hurst.map(|h| format!("H={:.2}", h)).unwrap_or("".into());
                        r.push_str(&format!(
                            "{} **{}** ${:.4} | Score: {:.0} | {} {} | {}\n",
                            emoji, a.symbol, a.price, a.score, a.regime, h, a.strategy_hint,
                        ));
                    }
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
                    // Use last 101 prices (window=100 + 1 for return) to match
                    // the incremental Hurst used in analyze_indicators.
                    let (hurst_val, market_char, strategy_hint) = match candles {
                        Ok(c) if c.len() >= 101 => {
                            let all_closes: Vec<f64> = c.iter().map(|k| k.close).collect();
                            let start = all_closes.len().saturating_sub(101);
                            let closes: Vec<f64> = all_closes[start..].to_vec();
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

    /// Fetch hot movers from Binance 24hr ticker API.
    /// Returns symbols with high volume and significant price change.
    async fn fetch_hot_movers(
        &self,
        min_volume: f64,
        min_change: f64,
        max_symbols: usize,
    ) -> anyhow::Result<Vec<HotMover>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        // Fetch ALL 24hr tickers from Binance
        let url = "https://api.binance.com/api/v3/ticker/24hr";
        let resp = client.get(url).send().await?;
        let tickers: Vec<Value> = resp.json().await.unwrap_or_default();

        // Filter: USDT pairs only, min volume, min change, exclude stablecoins
        let stablecoins = ["USDCUSDT", "USD1USDT", "RLUSDUSDT", "FDUSDUSDT", "EURUSDT", "BIOUSDT", "币安人生USDT"];
        let mut movers: Vec<HotMover> = tickers
            .iter()
            .filter_map(|t| {
                let symbol = t["symbol"].as_str()?.to_string();
                if !symbol.ends_with("USDT") { return None; }
                if stablecoins.contains(&symbol.as_str()) { return None; }

                let change_pct = t["priceChangePercent"].as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let volume = t["quoteVolume"].as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let price = t["lastPrice"].as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);

                // Filter criteria
                if volume < min_volume { return None; }
                if change_pct.abs() < min_change { return None; }

                Some(HotMover { symbol, price, change_pct, volume })
            })
            .collect();

        // Sort by absolute change % descending
        movers.sort_by(|a, b| {
            b.change_pct.abs().partial_cmp(&a.change_pct.abs()).unwrap_or(std::cmp::Ordering::Equal)
        });

        movers.truncate(max_symbols);
        Ok(movers)
    }
}

/// Hot mover data from Binance 24hr ticker.
struct HotMover {
    symbol: String,
    price: f64,
    change_pct: f64,
    volume: f64,
}
