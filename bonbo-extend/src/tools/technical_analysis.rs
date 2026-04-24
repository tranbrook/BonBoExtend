//! Technical Analysis Plugin — exposes bonbo-ta indicators via MCP tools.
//!
//! Includes Financial-Hacker.com indicators:
//! - ALMA (Arnaud Legoux Moving Average) — best smoothing
//! - SuperSmoother (Ehlers 2-pole Butterworth) — DSP noise filter
//! - Hurst Exponent — regime detection (trending vs mean-reverting)
//! - CMO (Chande Momentum Oscillator) — fast momentum
//! - Laguerre RSI (Ehlers) — adaptive oscillator

use crate::plugin::{ParameterSchema, PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use async_trait::async_trait;
use bonbo_ta::MarketCharacter;
use serde_json::{Value, json};

pub struct TechnicalAnalysisPlugin {
    metadata: PluginMetadata,
}

impl Default for TechnicalAnalysisPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl TechnicalAnalysisPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-technical-analysis".to_string(),
                name: "Technical Analysis".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Technical analysis with Financial-Hacker indicators (ALMA, Hurst, SuperSmoother, CMO, Laguerre RSI)".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec![
                    "analysis".to_string(),
                    "indicators".to_string(),
                    "ta".to_string(),
                    "financial-hacker".to_string(),
                ],
            },
        }
    }

    async fn fetch_candles(
        &self,
        symbol: &str,
        interval: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<bonbo_ta::models::OhlcvCandle>> {
        let fetcher = bonbo_data::fetcher::MarketDataFetcher::new();
        let raw = fetcher.fetch_klines(symbol, interval, Some(limit)).await?;
        Ok(raw
            .into_iter()
            .map(|c| bonbo_ta::models::OhlcvCandle {
                timestamp: c.timestamp,
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect())
    }

    async fn do_analyze_indicators(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        // Default 200 candles to ensure Hurst (needs ≥100) has enough data
        let limit = args["limit"].as_u64().unwrap_or(200) as u32;
        let candles = self.fetch_candles(symbol, interval, limit).await?;
        if candles.len() < 30 {
            return Ok("⚠️ Not enough candles for analysis (need at least 30)".to_string());
        }
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let analysis = bonbo_ta::batch::compute_full_analysis(&closes);
        let mut result = format!("📊 **Technical Analysis — {} ({})**\n\n", symbol, interval);

        // ── Traditional Indicators ──
        result.push_str("━━━ **Traditional Indicators** ━━━\n\n");

        if let Some(Some(v)) = analysis.sma20.last() {
            result.push_str(&format!("📈 **SMA(20)**: ${:.2}\n", v));
        }
        if let Some(Some(v)) = analysis.ema12.last() {
            result.push_str(&format!("📈 **EMA(12)**: ${:.2}\n", v));
        }
        if let Some(Some(v)) = analysis.ema26.last() {
            result.push_str(&format!("📈 **EMA(26)**: ${:.2}\n", v));
        }
        if let Some(Some(v)) = analysis.rsi14.last() {
            let label = if *v > 70.0 {
                "🔴 Overbought"
            } else if *v < 30.0 {
                "🟢 Oversold"
            } else {
                "⚪ Neutral"
            };
            result.push_str(&format!("\n📉 **RSI(14)**: {:.1} {}\n", v, label));
        }
        if let Some(Some(m)) = analysis.macd.last() {
            result.push_str(&format!(
                "\n📊 **MACD**: line={:.4} signal={:.4} hist={:.4} {}\n",
                m.macd_line,
                m.signal_line,
                m.histogram,
                if m.histogram > 0.0 { "🟢" } else { "🔴" }
            ));
        }
        if let Some(Some(bb)) = analysis.bb.last() {
            result.push_str(&format!(
                "\n🎯 **BB(20,2)**: upper=${:.2} mid=${:.2} lower=${:.2} %B={:.2}\n",
                bb.upper, bb.middle, bb.lower, bb.percent_b
            ));
        }

        // ── Financial-Hacker.com Indicators ──
        result.push_str("\n\n━━━ **Financial-Hacker Indicators** ━━━\n\n");

        // ALMA crossover
        if let (Some(alma_fast), Some(alma_slow)) = (
            analysis.alma10.last().and_then(|v| *v),
            analysis.alma30.last().and_then(|v| *v),
        ) {
            let diff_pct = if alma_slow > 0.0 {
                (alma_fast - alma_slow) / alma_slow * 100.0
            } else {
                0.0
            };
            let cross = if diff_pct > 0.0 {
                "🟢 Bullish"
            } else {
                "🔴 Bearish"
            };
            result.push_str(&format!(
                "🔮 **ALMA(10)**: ${:.2} | **ALMA(30)**: ${:.2} → {} ({:+.2}%)\n",
                alma_fast, alma_slow, cross, diff_pct
            ));
        } else if let Some(Some(v)) = analysis.alma10.last() {
            result.push_str(&format!("🔮 **ALMA(10)**: ${:.2}\n", v));
        }

        // SuperSmoother slope
        if analysis.super_smoother20.len() >= 2 {
            let curr = analysis.super_smoother20.last().and_then(|v| *v);
            let prev = analysis
                .super_smoother20
                .get(analysis.super_smoother20.len() - 2)
                .and_then(|v| *v);
            if let (Some(c), Some(p)) = (curr, prev) {
                let slope = if p > 0.0 { (c - p) / p * 100.0 } else { 0.0 };
                let arrow = if slope > 0.0 { "📈" } else { "📉" };
                result.push_str(&format!(
                    "{} **SuperSmoother(20)**: ${:.2} (slope: {:+.4}%)\n",
                    arrow, c, slope
                ));
            }
        }

        // Hurst Exponent
        if let Some(Some(h)) = analysis.hurst.last() {
            let regime_str = if *h > 0.55 {
                "📈 Trending → use trend-following"
            } else if *h < 0.45 {
                "🔄 Mean-Reverting → use mean-reversion"
            } else {
                "⚠️ Random Walk → caution advised"
            };
            result.push_str(&format!(
                "\n🧬 **Hurst(100)**: {:.3} — {}\n",
                h, regime_str
            ));

            // QW2: Hurst divergence detection
            if let Some(Some(h_short)) = analysis.hurst_short.last() {
                let divergence = (h - h_short).abs();
                if divergence > 0.15 {
                    let direction = if h_short > h { "emerging trend" } else { "fading trend" };
                    result.push_str(&format!(
                        "    ⚡ **Hurst Divergence**: short={:.3} vs long={:.3} (Δ={:.3}) → regime transition likely ({})\n",
                        h_short, h, divergence, direction
                    ));
                } else {
                    result.push_str(&format!(
                        "    ℹ️ **Hurst(50)**: {:.3} — aligned with long-term\n",
                        h_short
                    ));
                }
            }
        } else {
            result.push_str("\n🧬 **Hurst(100)**: ⏳ Need 100+ candles\n");
        }

        // CMO
        if let Some(Some(cmo)) = analysis.cmo14.last() {
            let label = if *cmo > 50.0 {
                "🔴 Overbought"
            } else if *cmo < -50.0 {
                "🟢 Oversold"
            } else if *cmo > 20.0 {
                "📈 Bullish momentum"
            } else if *cmo < -20.0 {
                "📉 Bearish momentum"
            } else {
                "⚪ Neutral"
            };
            result.push_str(&format!("⚡ **CMO(14)**: {:.1} {}\n", cmo, label));
        }

        // Laguerre RSI (dual gamma — QW3)
        if let Some(Some(lrsi)) = analysis.laguerre_rsi.last() {
            let label = if *lrsi > 0.8 {
                "🔴 Overbought"
            } else if *lrsi < 0.2 {
                "🟢 Oversold"
            } else {
                "⚪ Neutral"
            };
            result.push_str(&format!(
                "🌀 **LaguerreRSI(γ=0.8)**: {:.3} {}\n",
                lrsi, label
            ));
        }
        // QW3: Fast LaguerreRSI (gamma=0.5) — more responsive, avoids flat-line at 1.0
        if let Some(Some(lrsi_fast)) = analysis.laguerre_rsi_fast.last() {
            let label = if *lrsi_fast > 0.8 {
                "🔴 Overbought"
            } else if *lrsi_fast < 0.2 {
                "🟢 Oversold"
            } else {
                "⚪ Neutral"
            };
            result.push_str(&format!(
                "🌀 **LaguerreRSI(γ=0.5)**: {:.3} {} (responsive)\n",
                lrsi_fast, label
            ));
            // Show divergence between fast and slow
            if let Some(Some(lrsi_slow)) = analysis.laguerre_rsi.last() {
                let diff = lrsi_fast - lrsi_slow;
                if diff.abs() > 0.2 {
                    let hint = if diff > 0.0 { "momentum accelerating" } else { "momentum decelerating" };
                    result.push_str(&format!(
                        "    ⚡ **LaguerreRSI Divergence**: fast-slow={:+.3} → {}\n",
                        diff, hint
                    ));
                }
            }
        }

        if let Some(p) = closes.last() {
            result.push_str(&format!("\n💰 **Price**: ${:.2}\n", p));

            // QW1: ATR-based stop loss (regime-adaptive)
            if candles.len() >= 14 {
                let atr = compute_atr_from_candles(&candles, 14);
                if let Some(atr_val) = atr {
                    let hurst_val = analysis.hurst.last().and_then(|v| *v);
                    let (mult, regime_hint) = match hurst_val {
                        Some(h) if h > 0.55 => (2.0, "Trending → wider SL (2.0×ATR)"),
                        Some(h) if h < 0.45 => (1.5, "Mean-Reverting → tight SL (1.5×ATR)"),
                        Some(_) => (2.5, "Random Walk → widest SL (2.5×ATR)"),
                        None => (2.0, "Default SL (2.0×ATR)"),
                    };
                    let sl_long = p - atr_val * mult;
                    let sl_short = p + atr_val * mult;
                    let tp_r1 = p + atr_val * mult; // R:R 1:1
                    result.push_str(&format!(
                        "\n🛡️ **ATR Stops** (ATR(14)={:.4}, {})\n",
                        atr_val, regime_hint
                    ));
                    result.push_str(&format!(
                        "    LONG  → SL: ${:.2} ({:.1}%) | TP: ${:.2} (+{:.1}%)\n",
                        sl_long,
                        (sl_long - p) / p * 100.0,
                        tp_r1,
                        (tp_r1 - p) / p * 100.0,
                    ));
                    result.push_str(&format!(
                        "    SHORT → SL: ${:.2} (+{:.1}%)\n",
                        sl_short,
                        (sl_short - p) / p * 100.0,
                    ));
                }
            }
        }
        Ok(result)
    }

    async fn do_get_trading_signals(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        // 200 candles for Hurst
        let candles = self.fetch_candles(symbol, interval, 200).await?;
        if candles.len() < 30 {
            return Ok("⚠️ Not enough data".to_string());
        }
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let analysis = bonbo_ta::batch::compute_full_analysis(&closes);
        let price = closes.last().copied().unwrap_or(0.0);
        let signals = bonbo_ta::batch::generate_signals(&analysis, price);

        // Determine market character from Hurst for header
        let hurst_val = analysis.hurst.last().and_then(|v| *v);
        let char_str = match hurst_val {
            Some(h) if h > 0.55 => "📈 TRENDING (Hurst > 0.55)",
            Some(h) if h < 0.45 => "🔄 MEAN-REVERTING (Hurst < 0.45)",
            Some(h) => format!("⚠️ RANDOM WALK (Hurst = {:.2})", h).leak(),
            None => "❓ UNKNOWN",
        };

        let mut result = format!(
            "🎯 **Trading Signals — {} ({})**\n💰 Price: ${:.2}\n🧬 Market: {}\n\n",
            symbol, interval, price, char_str
        );

        if signals.is_empty() {
            result.push_str("⚪ No strong signals detected\n");
        } else {
            for sig in &signals {
                let icon = match sig.signal_type {
                    bonbo_ta::models::SignalType::StrongBuy => "🟢🟢",
                    bonbo_ta::models::SignalType::Buy => "🟢",
                    bonbo_ta::models::SignalType::Neutral => "⚪",
                    bonbo_ta::models::SignalType::Sell => "🔴",
                    bonbo_ta::models::SignalType::StrongSell => "🔴🔴",
                };
                result.push_str(&format!(
                    "{} **{:?}** [{}] ({:.0}%)\n   {}\n",
                    icon,
                    sig.signal_type,
                    sig.source,
                    sig.confidence * 100.0,
                    sig.reason
                ));
            }
        }
        Ok(result)
    }

    async fn do_detect_market_regime(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        // 200 candles for full analysis
        let candles = self.fetch_candles(symbol, interval, 200).await?;
        if candles.len() < 20 {
            return Ok("⚠️ Not enough data".to_string());
        }

        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let regime = bonbo_ta::batch::detect_market_regime(&candles);
        let price = candles.last().map(|c| c.close).unwrap_or(0.0);

        // Get Hurst value for display — use last 101 prices to match analyze_indicators
        // (which uses incremental HurstExponent with window=100, keeping window+1=101 prices)
        let hurst_window = 101;
        let start = closes.len().saturating_sub(hurst_window);
        let hurst_closes: Vec<f64> = closes[start..].to_vec();
        let hurst_val = bonbo_ta::HurstExponent::compute(&hurst_closes);
        let hurst_str = match hurst_val {
            Some(h) => format!("{:.3}", h),
            None => "N/A (need 100+ candles)".to_string(),
        };

        // Also compute Hurst on ALL prices for comparison
        let hurst_full = bonbo_ta::HurstExponent::compute(&closes);
        let hurst_full_str = match hurst_full {
            Some(h) => format!("{:.3}", h),
            None => "N/A".to_string(),
        };

        // Get Hurst character (use windowed Hurst for consistency with analyze_indicators)
        let char_str = match hurst_val {
            Some(h) if h > 0.55 => MarketCharacter::Trending.to_string(),
            Some(h) if h < 0.45 => MarketCharacter::MeanReverting.to_string(),
            Some(_) => MarketCharacter::RandomWalk.to_string(),
            None => MarketCharacter::Unknown.to_string(),
        };

        let desc = match regime {
            bonbo_ta::models::MarketRegime::TrendingUp => "📈 Uptrend — use trend-following (ALMA crossover, SuperSmoother slope)",
            bonbo_ta::models::MarketRegime::TrendingDown => "📉 Downtrend — consider shorts or exit longs",
            bonbo_ta::models::MarketRegime::Ranging => "↔️ Sideways — use mean-reversion (BB bounce, RSI extreme)",
            bonbo_ta::models::MarketRegime::Volatile => "⚡ High volatility — use wider stops, reduce position size",
            bonbo_ta::models::MarketRegime::Quiet => "🔇 Low volatility — breakout incoming, prepare entry",
        };

        // Strategy recommendation based on windowed Hurst (consistent with analyze_indicators)
        let strategy = match hurst_val {
            Some(h) if h > 0.55 => "→ Strategy: Ehlers Trend Following (SuperSmoother + ALMA crossover)",
            Some(h) if h < 0.45 => "→ Strategy: Mean Reversion (BB + RSI extreme + Hurst filter)",
            Some(_) => "→ Strategy: AVOID or reduce size (random walk market)",
            None => "→ Strategy: Insufficient data for Hurst, use standard approach",
        };

        // Detect if there's a divergence between short-term and long-term Hurst
        let hurst_divergence = match (hurst_val, hurst_full) {
            (Some(h_short), Some(h_long)) => {
                let diff = (h_short - h_long).abs();
                if diff > 0.15 {
                    Some(format!(
                        "⚠️ Hurst divergence: short-term({:.3}) vs long-term({:.3}) differ by {:.3} — regime transition likely",
                        h_short, h_long, diff
                    ))
                } else {
                    None
                }
            }
            _ => None,
        };

        let mut output = format!(
            "{}\n\n💰 **{}** @ ${:.2}\n🧬 Hurst(100): {} ({})\n🧬 Hurst(full): {}\n📝 {}\n\n{}",
            regime, symbol, price, hurst_str, char_str, hurst_full_str, desc, strategy
        );

        if let Some(divergence) = hurst_divergence {
            output.push_str(&format!("\n{}", divergence));
        }

        Ok(output)
    }

    async fn do_get_support_resistance(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str().unwrap_or("BTCUSDT");
        let interval = args["interval"].as_str().unwrap_or("1d");
        let lookback = args["lookback"].as_u64().unwrap_or(60) as u32;
        let candles = self.fetch_candles(symbol, interval, lookback).await?;
        if candles.len() < 10 {
            return Ok("⚠️ Not enough data".to_string());
        }
        let highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let lows: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let (supports, resistances) = bonbo_ta::batch::get_support_resistance(&highs, &lows);
        let price = candles.last().map(|c| c.close).unwrap_or(0.0);
        let mut result = format!("🎯 **S/R — {} ({})** @ ${:.2}\n\n", symbol, interval, price);
        result.push_str("🔴 **Resistance**:\n");
        for (i, r) in resistances.iter().enumerate() {
            result.push_str(&format!(
                "  R{}: ${:.2} (+{:.1}%)\n",
                i + 1,
                r,
                (r - price) / price * 100.0
            ));
        }
        result.push_str("\n🟢 **Support**:\n");
        for (i, s) in supports.iter().enumerate() {
            result.push_str(&format!(
                "  S{}: ${:.2} (-{:.1}%)\n",
                i + 1,
                s,
                (price - s) / price * 100.0
            ));
        }
        Ok(result)
    }
}

#[async_trait]
impl ToolPlugin for TechnicalAnalysisPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "analyze_indicators".into(),
                description: "Compute all technical indicators including Financial-Hacker (ALMA, Hurst, SuperSmoother, CMO, LaguerreRSI) for a crypto symbol".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g. BTCUSDT)".into(),
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
                        r#enum: Some(vec![
                            "1m".into(),
                            "5m".into(),
                            "15m".into(),
                            "1h".into(),
                            "4h".into(),
                            "1d".into(),
                        ]),
                    },
                    ParameterSchema {
                        name: "limit".into(),
                        param_type: "integer".into(),
                        description: "Number of candles (200 recommended for Hurst)".into(),
                        required: false,
                        default: Some(json!(200)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_trading_signals".into(),
                description: "Generate buy/sell signals using Financial-Hacker methodology (Hurst regime filter, ALMA crossover, SuperSmoother, CMO, LaguerreRSI)".into(),
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
                ],
            },
            ToolSchema {
                name: "detect_market_regime".into(),
                description: "Detect market regime using Hurst Exponent (trending vs mean-reverting vs random walk) with strategy recommendations".into(),
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
                ],
            },
            ToolSchema {
                name: "get_support_resistance".into(),
                description: "Identify support and resistance levels".into(),
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
                        name: "lookback".into(),
                        param_type: "integer".into(),
                        description: "Lookback period".into(),
                        required: false,
                        default: Some(json!(60)),
                        r#enum: None,
                    },
                ],
            },
        ]
    }
    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &Value,
        _context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "analyze_indicators" => self.do_analyze_indicators(arguments).await,
            "get_trading_signals" => self.do_get_trading_signals(arguments).await,
            "detect_market_regime" => self.do_detect_market_regime(arguments).await,
            "get_support_resistance" => self.do_get_support_resistance(arguments).await,
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

/// QW1: Compute ATR from candle data using Wilder's smoothing.
/// Returns None if not enough candles.
fn compute_atr_from_candles(candles: &[bonbo_ta::models::OhlcvCandle], period: usize) -> Option<f64> {
    if candles.len() < period + 1 {
        return None;
    }

    // True Range for each candle
    let mut tr_values: Vec<f64> = Vec::with_capacity(candles.len() - 1);
    for i in 1..candles.len() {
        let high = candles[i].high;
        let low = candles[i].low;
        let prev_close = candles[i - 1].close;
        let tr = (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs());
        tr_values.push(tr);
    }

    if tr_values.len() < period {
        return None;
    }

    // Wilder's smoothing: first value = SMA, then EMA with alpha = 1/period
    let first_sma: f64 = tr_values[..period].iter().sum::<f64>() / period as f64;
    let mut atr = first_sma;
    let alpha = 1.0 / period as f64;
    for tr in &tr_values[period..] {
        atr = atr * (1.0 - alpha) + tr * alpha;
    }
    Some(atr)
}
