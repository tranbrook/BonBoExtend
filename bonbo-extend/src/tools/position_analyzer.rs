//! Position Analyzer Plugin — Automated full position analysis pipeline.
//!
//! Single call replaces 8+ manual tool calls:
//! 1. Fetch multi-timeframe klines (15m, 1H, 4H, 1D)
//! 2. Compute full technical indicators per TF
//! 3. Generate signals per TF
//! 4. Detect market regime (4H)
//! 5. Compute S/R levels
//! 6. Build scoring matrix (0-100)
//! 7. Return recommendation with action items

use async_trait::async_trait;
use bonbo_data::fetcher::MarketDataFetcher;
use bonbo_data::models::{MarketDataCandle, to_ohlcv};
use bonbo_extend_core::{ParameterSchema, PluginContext, PluginMetadata, ToolPlugin, ToolSchema};
use bonbo_ta::batch::{compute_full_analysis, get_support_resistance};
use bonbo_ta::models::{MarketRegime, Signal, SignalType};
use serde_json::{Value, json};

// ── Plugin ────────────────────────────────────────────────────

pub struct PositionAnalyzerPlugin {
    metadata: PluginMetadata,
}

impl Default for PositionAnalyzerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PositionAnalyzerPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-position-analyzer".into(),
                name: "Position Analyzer".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Automated multi-TF analysis pipeline with scoring and recommendation"
                    .into(),
                author: "BonBo Team".into(),
                tags: vec!["analysis".into(), "position".into(), "automation".into()],
            },
        }
    }

    fn fetcher() -> MarketDataFetcher {
        MarketDataFetcher::new()
    }

    fn tf_label(tf: &str) -> &str {
        match tf {
            "15m" => "15m",
            "1h" => "1H",
            "4h" => "4H",
            "1d" => "1D",
            _ => tf,
        }
    }

    // ── Core Analysis ─────────────────────────────────────────

    async fn analyze_position(
        &self,
        symbol: &str,
        entry_price: f64,
        quantity: f64,
        leverage: u32,
        side: &str,
    ) -> anyhow::Result<String> {
        let is_long = side.to_uppercase() == "LONG";
        let timeframes = ["15m", "1h", "4h", "1d"];

        // ═══ STEP 1: Fetch klines for all timeframes ═══
        let mut tf_data: Vec<(&str, Vec<MarketDataCandle>)> = Vec::new();
        for tf_str in &timeframes {
            match Self::fetcher()
                .fetch_klines(symbol, tf_str, Some(200))
                .await
            {
                Ok(candles) => tf_data.push((*tf_str, candles)),
                Err(e) => tracing::warn!("Failed to fetch {} {}: {}", symbol, tf_str, e),
            }
        }

        if tf_data.is_empty() {
            anyhow::bail!("Failed to fetch any data for {}", symbol);
        }

        // Current mark price = last 15m close
        let mark_price = tf_data
            .iter()
            .find(|(tf, _)| *tf == "15m")
            .or_else(|| tf_data.first())
            .and_then(|(_, c)| c.last())
            .map(|c| c.close)
            .unwrap_or(entry_price);

        // ═══ STEP 2: Compute analysis per timeframe ═══
        let mut tf_results: Vec<TfResult> = Vec::new();
        for (tf_label, candles) in &tf_data {
            let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
            let _highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
            let _lows: Vec<f64> = candles.iter().map(|c| c.low).collect();

            let analysis = compute_full_analysis(&closes);
            let _ohlcv = to_ohlcv(candles);
            let signals = bonbo_ta::batch::generate_signals(&analysis, mark_price);

            // Count signals
            let bull_count = signals
                .iter()
                .filter(|s| matches!(s.signal_type, SignalType::StrongBuy | SignalType::Buy))
                .count();
            let bear_count = signals
                .iter()
                .filter(|s| matches!(s.signal_type, SignalType::StrongSell | SignalType::Sell))
                .count();

            // Extract key values (last values)
            let hurst_100 = last_val(&analysis.hurst);
            let hurst_50 = last_val(&analysis.hurst_short);
            let alma10 = last_val(&analysis.alma10);
            let alma30 = last_val(&analysis.alma30);
            let alma_spread = match (alma10, alma30) {
                (Some(a10), Some(a30)) if a30 > 0.0 => Some((a10 - a30) / a30 * 100.0),
                _ => None,
            };
            let ss_val = last_val(&analysis.super_smoother20);
            let ss_slope = match (ss_val, closes.last()) {
                (Some(ss), Some(p)) if *p > 0.0 => Some((ss - p) / p * 100.0),
                _ => None,
            };
            let rsi = last_val(&analysis.rsi14);
            let macd = last_val(&analysis.macd);
            let macd_hist = macd.map(|m| m.histogram);
            let cmo = last_val(&analysis.cmo14);
            let lag_fast = last_val(&analysis.laguerre_rsi_fast);
            let lag_slow = last_val(&analysis.laguerre_rsi);
            let lag_div = match (lag_fast, lag_slow) {
                (Some(f), Some(s)) => Some(f - s),
                _ => None,
            };
            let atr = last_val(&analysis.atr14);
            let _bb = last_val(&analysis.bb);

            // Build signal tags
            let mut tags = Vec::new();
            if let Some(h) = hurst_100 {
                tags.push(if h > 0.55 {
                    format!("H={:.2}📈", h)
                } else if h < 0.45 {
                    format!("H={:.2}🔄", h)
                } else {
                    format!("H={:.2}⚠️", h)
                });
            }
            if let Some(s) = alma_spread {
                tags.push(format!("ALMA={:+.1}%", s));
            }
            if let Some(s) = ss_slope {
                tags.push(format!("SS={:+.1}%", s));
            }
            if let Some(h) = macd_hist {
                tags.push(if h > 0.0 {
                    "MACD🟢".into()
                } else {
                    "MACD🔴".into()
                });
            }
            if let Some(l) = lag_fast {
                if l > 0.8 {
                    tags.push("Lag🔴OB".into());
                } else if l < 0.2 {
                    tags.push("Lag🟢OS".into());
                }
            }
            if let Some(d) = lag_div {
                tags.push(format!("LagΔ={:+.2}", d));
            }

            tf_results.push(TfResult {
                timeframe: tf_label.to_string(),
                bull_count,
                bear_count,
                tags,
                hurst_100,
                hurst_50,
                alma_spread,
                ss_slope,
                rsi,
                macd_hist,
                cmo,
                lag_fast,
                lag_slow,
                lag_div,
                atr,
                signals,
            });
        }

        // ═══ STEP 3: Regime detection (4H) ═══
        let regime_4h = tf_data
            .iter()
            .find(|(tf, _)| *tf == "4h")
            .or_else(|| tf_data.first())
            .map(|(_, c)| {
                let ohlcv = to_ohlcv(c);
                bonbo_ta::batch::detect_market_regime(&ohlcv)
            })
            .unwrap_or(MarketRegime::Ranging);

        // ═══ STEP 4: S/R levels (4H) ═══
        let (resistances, supports) = tf_data
            .iter()
            .find(|(tf, _)| *tf == "4h")
            .or_else(|| tf_data.first())
            .map(|(_, c)| {
                let highs: Vec<f64> = c.iter().map(|c| c.high).collect();
                let lows: Vec<f64> = c.iter().map(|c| c.low).collect();
                get_support_resistance(&highs, &lows)
            })
            .unwrap_or((vec![], vec![]));

        let r1 = resistances.first().copied();
        let r2 = resistances.get(1).copied();
        let s1 = supports.first().copied();
        let s2 = supports.get(2).copied();

        // ═══ STEP 5: ATR stops (4H) ═══
        let tf4h = tf_results.iter().find(|t| t.timeframe == "4h");
        let atr_val = tf4h.and_then(|t| t.atr).unwrap_or(0.0);
        let atr_sl = if atr_val > 0.0 {
            Some(mark_price - atr_val * 2.0)
        } else {
            None
        };
        let atr_tp = if atr_val > 0.0 {
            Some(mark_price + atr_val * 2.0)
        } else {
            None
        };

        // ═══ STEP 6: Scoring Matrix ═══
        let total_bull: usize = tf_results.iter().map(|t| t.bull_count).sum();
        let total_bear: usize = tf_results.iter().map(|t| t.bear_count).sum();

        let signal_score =
            (total_bull as f64 / (total_bull + total_bear).max(1) as f64 * 40.0) as i32;
        let hurst_score = score_hurst(tf4h.and_then(|t| t.hurst_100));
        let momentum_score = score_laguerre(tf4h.and_then(|t| t.lag_div));
        let regime_score = score_regime(regime_4h, is_long);
        let alma_score = score_alma(tf4h.and_then(|t| t.alma_spread));

        let total_score = signal_score + hurst_score + momentum_score + regime_score + alma_score;

        // ═══ STEP 7: PnL ═══
        let pnl_raw = (mark_price - entry_price) * quantity * if is_long { 1.0 } else { -1.0 };
        let pnl_pct = (mark_price / entry_price - 1.0) * if is_long { 1.0 } else { -1.0 } * 100.0;

        // ═══ STEP 8: Build Report ═══
        let pnl_emoji = if pnl_raw >= 0.0 { "🟢" } else { "🔴" };
        let side_emoji = if is_long { "📈" } else { "📉" };
        let mut r = String::new();

        r.push_str(&format!(
            "# {} {} {} — Full Analysis\n\n",
            side_emoji,
            symbol,
            side.to_uppercase()
        ));

        // Position Status
        r.push_str("## 📊 Position Status\n\n");
        r.push_str("| Field | Value |\n|-------|-------|\n");
        r.push_str(&format!("| Symbol | {} |\n", symbol));
        r.push_str(&format!(
            "| Side | {} {} |\n",
            side_emoji,
            side.to_uppercase()
        ));
        r.push_str(&format!("| Quantity | {} |\n", quantity));
        r.push_str(&format!("| Entry | ${:.4} |\n", entry_price));
        r.push_str(&format!("| Mark | ${:.4} |\n", mark_price));
        r.push_str(&format!(
            "| PnL | {} **${:.2}** ({:+.2}%) |\n",
            pnl_emoji, pnl_raw, pnl_pct
        ));
        r.push_str(&format!("| Leverage | {}x |\n", leverage));
        r.push_str(&format!("| Notional | ${:.2} |\n", entry_price * quantity));
        r.push_str(&format!("| Regime (4H) | {} |\n", regime_4h));

        // Multi-TF Signals
        r.push_str("\n## 🔄 Multi-Timeframe Signals\n\n");
        r.push_str("| TF | Bull | Bear | Key Signals | Verdict |\n");
        r.push_str("|-----|------|------|-------------|----------|\n");
        for t in &tf_results {
            let verdict = if t.bull_count > t.bear_count + 1 {
                "🟢 BULL"
            } else if t.bear_count > t.bull_count + 1 {
                "🔴 BEAR"
            } else {
                "🟡 MIXED"
            };
            r.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                Self::tf_label(&t.timeframe),
                t.bull_count,
                t.bear_count,
                t.tags.join(", "),
                verdict
            ));
        }
        let overall = if total_bull > total_bear {
            "🟢 BULLISH"
        } else if total_bear > total_bull {
            "🔴 BEARISH"
        } else {
            "🟡 NEUTRAL"
        };
        r.push_str(&format!(
            "\n**Total: {} Buy / {} Sell → {}**\n",
            total_bull, total_bear, overall
        ));

        // Detailed signals (4H)
        if let Some(t4) = tf4h {
            r.push_str("\n## 📈 Detailed Signals (4H)\n\n");
            r.push_str("| Indicator | Value | Signal |\n|-----------|-------|--------|\n");
            if let Some(h) = t4.hurst_100 {
                let sig = if h > 0.55 {
                    "📈 Trending"
                } else if h < 0.45 {
                    "🔄 Mean-Reverting"
                } else {
                    "⚠️ Random Walk"
                };
                r.push_str(&format!("| Hurst(100) | **{:.3}** | {} |\n", h, sig));
            }
            if let Some(h) = t4.hurst_50 {
                r.push_str(&format!("| Hurst(50) | **{:.3}** | Short-term |\n", h));
            }
            if let (Some(h100), Some(h50)) = (t4.hurst_100, t4.hurst_50) {
                let div = h50 - h100;
                let sig = if div > 0.15 {
                    "🚀 EMERGING"
                } else if div < -0.15 {
                    "⚠️ FADING"
                } else {
                    "⚪ Stable"
                };
                r.push_str(&format!(
                    "| Hurst Divergence | **{:+.3}** | {} |\n",
                    div, sig
                ));
            }
            if let Some(s) = t4.alma_spread {
                let sig = if s > 0.0 {
                    format!("🟢 Bullish +{:.2}%", s)
                } else {
                    format!("🔴 Bearish {:.2}%", s)
                };
                r.push_str(&format!(
                    "| ALMA(10,30) Spread | **{:.2}%** | {} |\n",
                    s, sig
                ));
            }
            if let Some(s) = t4.ss_slope {
                let sig = if s > 0.0 {
                    format!("🟢 Uptrend +{:.2}%", s)
                } else {
                    format!("🔴 Downtrend {:.2}%", s)
                };
                r.push_str(&format!(
                    "| SuperSmoother Slope | **{:+.2}%** | {} |\n",
                    s, sig
                ));
            }
            if let Some(h) = t4.macd_hist {
                let sig: String = if h > 0.0 {
                    "🟢 Bullish".into()
                } else {
                    "🔴 Bearish".into()
                };
                r.push_str(&format!("| MACD Histogram | **{:+.4}** | {} |\n", h, sig));
            }
            if let Some(v) = t4.rsi {
                let sig = if v > 70.0 {
                    format!("🔴 Overbought {:.1}", v)
                } else if v < 30.0 {
                    format!("🟢 Oversold {:.1}", v)
                } else {
                    format!("⚪ Neutral {:.1}", v)
                };
                r.push_str(&format!("| RSI(14) | **{:.1}** | {} |\n", v, sig));
            }
            if let Some(v) = t4.cmo {
                let sig = if v > 0.0 {
                    format!("🟢 Bullish +{:.1}", v)
                } else {
                    format!("🔴 Bearish {:.1}", v)
                };
                r.push_str(&format!("| CMO(14) | **{:.1}** | {} |\n", v, sig));
            }
            if let Some(v) = t4.lag_fast {
                let sig = if v > 0.8 {
                    format!("🔴 Overbought {:.3}", v)
                } else if v < 0.2 {
                    format!("🟢 Oversold {:.3}", v)
                } else {
                    format!("⚪ {:.3}", v)
                };
                r.push_str(&format!(
                    "| LaguerreRSI (fast) | **{:.3}** | {} |\n",
                    v, sig
                ));
            }
            if let Some(v) = t4.lag_slow {
                let sig = if v > 0.8 {
                    format!("🔴 Overbought {:.3}", v)
                } else if v < 0.2 {
                    format!("🟢 Oversold {:.3}", v)
                } else {
                    format!("⚪ {:.3}", v)
                };
                r.push_str(&format!(
                    "| LaguerreRSI (slow) | **{:.3}** | {} |\n",
                    v, sig
                ));
            }
            if let Some(d) = t4.lag_div {
                let sig = if d > 0.3 {
                    format!("🚀 Accelerating +{:.3}", d)
                } else if d > 0.0 {
                    format!("🟢 Slight +{:.3}", d)
                } else if d > -0.3 {
                    format!("⚠️ Decelerating {:.3}", d)
                } else {
                    format!("🔴 Strong decel {:.3}", d)
                };
                r.push_str(&format!(
                    "| Laguerre Divergence | **{:+.3}** | {} |\n",
                    d, sig
                ));
            }
        }

        // Key Levels
        r.push_str("\n## 🎯 Key Levels\n\n```\n");
        if let Some(r2v) = r2 {
            r.push_str(&format!(
                "  R2 ${:.4}  (+{:.1}%)\n",
                r2v,
                (r2v / mark_price - 1.0) * 100.0
            ));
        }
        if let Some(r1v) = r1 {
            r.push_str(&format!(
                "  R1 ${:.4}  (+{:.1}%)\n",
                r1v,
                (r1v / mark_price - 1.0) * 100.0
            ));
        }
        r.push_str(&format!("  → ${:.4}  ← CURRENT\n", mark_price));
        if let Some(s1v) = s1 {
            r.push_str(&format!(
                "  S1 ${:.4}  ({:+.1}%)\n",
                s1v,
                (s1v / mark_price - 1.0) * 100.0
            ));
        }
        if let Some(s2v) = s2 {
            r.push_str(&format!(
                "  S2 ${:.4}  ({:+.1}%)\n",
                s2v,
                (s2v / mark_price - 1.0) * 100.0
            ));
        }
        if let Some(sl) = atr_sl {
            r.push_str(&format!(
                "  ATR SL ${:.4} ({:+.1}%)\n",
                sl,
                (sl / mark_price - 1.0) * 100.0
            ));
        }
        if let Some(tp) = atr_tp {
            r.push_str(&format!(
                "  ATR TP ${:.4} (+{:.1}%)\n",
                tp,
                (tp / mark_price - 1.0) * 100.0
            ));
        }
        r.push_str("```\n");

        // Scoring Matrix
        r.push_str("\n## 🏆 Scoring Matrix\n\n");
        r.push_str("| Component | Score | Max |\n|-----------|-------|-----|\n");
        r.push_str(&format!("| Multi-TF Signals | {} | 40 |\n", signal_score));
        r.push_str(&format!(
            "| Trend Strength (Hurst) | {} | 20 |\n",
            hurst_score
        ));
        r.push_str(&format!(
            "| Momentum (Laguerre Δ) | {} | 15 |\n",
            momentum_score
        ));
        r.push_str(&format!("| ALMA Trend Strength | {} | 10 |\n", alma_score));
        r.push_str(&format!("| Regime Alignment | {} | 15 |\n", regime_score));
        r.push_str(&format!("| **TOTAL** | **{}** | **100** |\n", total_score));

        // Recommendation
        let (rec_emoji, rec_text) = if total_score >= 70 {
            ("✅", "GIỮ VỊ THẾ — Signals tích cực, trend mạnh")
        } else if total_score >= 50 {
            ("🟡", "THẬN TRỌNG — Signals mixed, theo dõi chặt")
        } else if total_score >= 35 {
            ("⚠️", "CÂN NHẮC GIẢM — Signals yếu, bảo vệ vốn")
        } else {
            ("🔴", "ĐÓNG VỊ THẾ — Signals xấu, cắt lỗ")
        };

        r.push_str(&format!(
            "\n## {} Recommendation (Score: {}/100)\n\n",
            rec_emoji, total_score
        ));
        r.push_str(&format!("**{}**\n\n", rec_text));

        // Action Items
        r.push_str("**Action Items:**\n");
        if total_score >= 70 {
            r.push_str(&format!("- ✅ Giữ vị thế {}\n", side.to_uppercase()));
            if let Some(tp) = atr_tp {
                r.push_str(&format!("- 🎯 ATR TP target: ${:.4}\n", tp));
            }
            if let Some(sl) = atr_sl {
                r.push_str(&format!("- 🛡️ ATR SL level: ${:.4}\n", sl));
            }
            r.push_str("- 📈 Trend đang mạnh, hold tối ưu\n");
        } else if total_score >= 50 {
            r.push_str("- 🟡 Giữ nhưng theo dõi chặt\n");
            if let Some(sl) = atr_sl {
                r.push_str(&format!("- 🛡️ Tighten SL về ${:.4}\n", sl));
            }
            r.push_str("- ⚠️ Watch for regime change\n");
        } else {
            r.push_str("- 🔴 Cân nhắc đóng vị thế\n");
            r.push_str("- 🛡️ Đặt SL chặt để bảo vệ vốn\n");
            r.push_str("- ⚠️ Signals đang xấu đi\n");
        }

        // Distance to SL/TP
        r.push_str(&format!("\n**Distance from mark ${:.4}:**\n", mark_price));
        r.push_str("```\n");
        if let Some(r1v) = r1 {
            r.push_str(&format!(
                "  R1 ${:.4}  ({:+.1}%)\n",
                r1v,
                (r1v / mark_price - 1.0) * 100.0
            ));
        }
        if let Some(s1v) = s1 {
            r.push_str(&format!(
                "  S1 ${:.4}  ({:+.1}%)\n",
                s1v,
                (s1v / mark_price - 1.0) * 100.0
            ));
        }
        r.push_str("```\n");

        Ok(r)
    }

    async fn quick_check(&self, symbol: &str, entry_price: f64) -> anyhow::Result<String> {
        tracing::info!("⚡ quick_check: fetching {} 4h", symbol);
        let candles = Self::fetcher()
            .fetch_klines(symbol, "4h", Some(200))
            .await?;
        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let mark_price = closes.last().copied().unwrap_or(entry_price);

        let analysis = compute_full_analysis(&closes);
        let signals = bonbo_ta::batch::generate_signals(&analysis, mark_price);
        let ohlcv = to_ohlcv(&candles);
        let regime = bonbo_ta::batch::detect_market_regime(&ohlcv);

        let bull = signals
            .iter()
            .filter(|s| matches!(s.signal_type, SignalType::StrongBuy | SignalType::Buy))
            .count();
        let bear = signals
            .iter()
            .filter(|s| matches!(s.signal_type, SignalType::StrongSell | SignalType::Sell))
            .count();
        let hurst = last_val(&analysis.hurst);
        let alma_spread = match (last_val(&analysis.alma10), last_val(&analysis.alma30)) {
            (Some(a10), Some(a30)) if a30 > 0.0 => Some((a10 - a30) / a30 * 100.0),
            _ => None,
        };
        let lag_div = match (
            last_val(&analysis.laguerre_rsi_fast),
            last_val(&analysis.laguerre_rsi),
        ) {
            (Some(f), Some(s)) => Some(f - s),
            _ => None,
        };
        let rsi = last_val(&analysis.rsi14);
        let atr = last_val(&analysis.atr14);

        let pnl_pct = (mark_price / entry_price - 1.0) * 100.0;
        let pnl_emoji = if pnl_pct >= 0.0 { "🟢" } else { "🔴" };
        let verdict = if bull > bear + 1 {
            "🟢 BULLISH"
        } else if bear > bull + 1 {
            "🔴 BEARISH"
        } else {
            "🟡 MIXED"
        };

        Ok(format!(
            "## ⚡ Quick Check — {}\n\n\
             | Field | Value |\n|-------|-------|\n\
             | Entry | ${:.4} |\n\
             | Mark | ${:.4} |\n\
             | PnL | {} **{:+.2}%** |\n\
             | Regime | {} |\n\
             | 4H Signals | {} (B{}/S{}) |\n\
             | Hurst(100) | {} |\n\
             | ALMA spread | {} |\n\
             | Laguerre Δ | {} |\n\
             | RSI(14) | {} |\n\
             | ATR(14) | {} |\n\
             | Verdict | {} |",
            symbol,
            entry_price,
            mark_price,
            pnl_emoji,
            pnl_pct,
            regime,
            verdict,
            bull,
            bear,
            hurst.map(|h| format!("{:.3}", h)).unwrap_or("N/A".into()),
            alma_spread
                .map(|s| format!("{:+.2}%", s))
                .unwrap_or("N/A".into()),
            lag_div
                .map(|d| format!("{:+.3}", d))
                .unwrap_or("N/A".into()),
            rsi.map(|r| format!("{:.1}", r)).unwrap_or("N/A".into()),
            atr.map(|a| format!("${:.4}", a)).unwrap_or("N/A".into()),
            verdict,
        ))
    }
}

// ── Internal struct ──────────────────────────────────────────

struct TfResult {
    timeframe: String,
    bull_count: usize,
    bear_count: usize,
    tags: Vec<String>,
    hurst_100: Option<f64>,
    hurst_50: Option<f64>,
    alma_spread: Option<f64>,
    ss_slope: Option<f64>,
    rsi: Option<f64>,
    macd_hist: Option<f64>,
    cmo: Option<f64>,
    lag_fast: Option<f64>,
    lag_slow: Option<f64>,
    lag_div: Option<f64>,
    atr: Option<f64>,
    #[allow(dead_code)]
    signals: Vec<Signal>,
}

// ── Scoring functions ────────────────────────────────────────

fn score_hurst(hurst: Option<f64>) -> i32 {
    match hurst {
        Some(h) if h > 0.65 => 20,
        Some(h) if h > 0.55 => 16,
        Some(h) if h > 0.45 => 10,
        Some(_) => 6,
        None => 5,
    }
}

fn score_laguerre(div: Option<f64>) -> i32 {
    match div {
        Some(d) if d > 0.5 => 15,
        Some(d) if d > 0.2 => 12,
        Some(d) if d > 0.0 => 8,
        Some(d) if d > -0.2 => 5,
        Some(_) => 2,
        None => 5,
    }
}

fn score_regime(regime: MarketRegime, is_long: bool) -> i32 {
    match regime {
        MarketRegime::TrendingUp => {
            if is_long {
                15
            } else {
                3
            }
        }
        MarketRegime::TrendingDown => {
            if is_long {
                3
            } else {
                15
            }
        }
        MarketRegime::Ranging => 8,
        MarketRegime::Volatile => 5,
        MarketRegime::Quiet => 7,
    }
}

fn score_alma(spread: Option<f64>) -> i32 {
    match spread {
        Some(s) if s > 3.0 => 10,
        Some(s) if s > 1.0 => 8,
        Some(s) if s > 0.0 => 5,
        Some(s) if s > -1.0 => 3,
        Some(_) => 1,
        None => 5,
    }
}

// ── ToolPlugin impl ──────────────────────────────────────────

#[async_trait]
impl ToolPlugin for PositionAnalyzerPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "analyze_position_full".into(),
                description: "Full automated position analysis. Fetches multi-TF data, computes all indicators, generates signals, detects regime, builds scoring matrix (0-100), and returns recommendation. Replaces 8+ manual tool calls.".into(),
                parameters: vec![
                    ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair (e.g., ORDIUSDT)".into(), required: true, default: None, r#enum: None },
                    ParameterSchema { name: "entry_price".into(), param_type: "number".into(), description: "Entry price (default: current market price)".into(), required: false, default: None, r#enum: None },
                    ParameterSchema { name: "quantity".into(), param_type: "number".into(), description: "Position quantity (default: 1)".into(), required: false, default: Some(json!(1)), r#enum: None },
                    ParameterSchema { name: "leverage".into(), param_type: "integer".into(), description: "Leverage (default: 1)".into(), required: false, default: Some(json!(1)), r#enum: None },
                    ParameterSchema { name: "side".into(), param_type: "string".into(), description: "LONG or SHORT (default: LONG)".into(), required: false, default: Some(json!("LONG")), r#enum: None },
                ],
            },
            ToolSchema {
                name: "quick_position_check".into(),
                description: "Fast position health check. Only fetches 4H data. Returns PnL, key indicators, and brief verdict. Use for quick monitoring.".into(),
                parameters: vec![
                    ParameterSchema { name: "symbol".into(), param_type: "string".into(), description: "Trading pair (e.g., ORDIUSDT)".into(), required: true, default: None, r#enum: None },
                    ParameterSchema { name: "entry_price".into(), param_type: "number".into(), description: "Entry price".into(), required: true, default: None, r#enum: None },
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
            "analyze_position_full" => {
                let symbol = args["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let quantity = args["quantity"].as_f64().unwrap_or(1.0);
                let leverage = args["leverage"].as_u64().unwrap_or(1) as u32;
                let side = args["side"].as_str().unwrap_or("LONG").to_string();
                // Fetch mark price first for default entry
                let entry_price = args["entry_price"].as_f64().unwrap_or(0.0);
                let entry = if entry_price > 0.0 {
                    entry_price
                } else {
                    Self::fetcher()
                        .fetch_klines(symbol, "15m", Some(1))
                        .await
                        .ok()
                        .and_then(|c| c.last().map(|c| c.close))
                        .unwrap_or(0.0)
                };
                if entry <= 0.0 {
                    anyhow::bail!("Cannot determine price for {}", symbol);
                }
                self.analyze_position(symbol, entry, quantity, leverage, &side)
                    .await
            }
            "quick_position_check" => {
                let symbol = args["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let entry_price = args["entry_price"]
                    .as_f64()
                    .ok_or_else(|| anyhow::anyhow!("entry_price is required"))?;
                tracing::info!("⚡ quick_position_check starting for {}", symbol);
                let result = self.quick_check(symbol, entry_price).await;
                tracing::info!(
                    "⚡ quick_position_check done: {:?}",
                    result.as_ref().map(|s| s.len())
                );
                result
            }
            _ => anyhow::bail!("Unknown tool: {}", tool_name),
        }
    }
}

/// Get last Some value from an Option vector
fn last_val<T: Clone>(v: &[Option<T>]) -> Option<T> {
    v.iter().rev().find_map(|x| x.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_val_empty() {
        let v: Vec<Option<f64>> = vec![];
        assert_eq!(last_val(&v), None);
    }

    #[test]
    fn test_last_val_all_none() {
        let v: Vec<Option<f64>> = vec![None, None, None];
        assert_eq!(last_val(&v), None);
    }

    #[test]
    fn test_last_val_some_values() {
        let v: Vec<Option<f64>> = vec![Some(1.0), None, Some(3.0), Some(5.0)];
        assert_eq!(last_val(&v), Some(5.0));
    }

    #[test]
    fn test_last_val_trailing_nones() {
        let v: Vec<Option<f64>> = vec![Some(1.0), Some(2.0), None, None];
        assert_eq!(last_val(&v), Some(2.0));
    }

    #[test]
    fn test_last_val_single_some() {
        let v: Vec<Option<i32>> = vec![None, None, Some(42), None];
        assert_eq!(last_val(&v), Some(42));
    }
}
