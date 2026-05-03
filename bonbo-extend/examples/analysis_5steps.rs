//! BonBo Trading Analysis Process — Tuân thủ đúng 5 bước từ docs/trading-analysis-process.md
//!
//! ① SCAN: Quét rộng 25 coins → lọc xuống 10-15 coin tiềm năng
//! ② SENTIMENT: Fear/Greed + Whale flow
//! ③ DEEP ANALYSIS: Multi-TF indicators + Hurst regime + Trading signals + S/R
//! ④ BACKTEST/RISK: Backtest SMA crossover + Position sizing
//! ⑤ RA QUYẾT ĐỊNH: Weighted scoring → Trade / Wait / Avoid

use bonbo_data::fetcher::MarketDataFetcher;
use bonbo_data::to_ohlcv;
use bonbo_ta::HurstExponent;
use bonbo_ta::batch::compute_full_analysis;
use std::time::Instant;

fn last_val(v: &[Option<f64>]) -> Option<f64> {
    v.iter().rev().find_map(|&x| x)
}

// ═══════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct CoinScan {
    symbol: String,
    price: f64,
    change_24h_pct: f64,
    volume_24h_usd: f64,
    hurst: Option<f64>,
    market_char: String,
    strategy_hint: String,
    scan_score: f64,
}

#[derive(Debug, Clone)]
struct MultiTfResult {
    timeframe: String,
    hurst: Option<f64>,
    regime: String,
    rsi: Option<f64>,
    macd_signal: String,
    alma_signal: String,
    bb_pos: String,
    atr: Option<f64>,
}

#[derive(Debug, Clone)]
struct CoinDeep {
    symbol: String,
    price: f64,
    change_24h: f64,
    volume_24h: f64,
    // Multi-timeframe
    tf_results: Vec<MultiTfResult>,
    // Hurst consensus
    hurst_consensus: String,
    strategy: String,
    // Signals count
    buy_signals: usize,
    sell_signals: usize,
    neutral_signals: usize,
    // S/R levels
    support: f64,
    resistance: f64,
    // Step 4
    backtest_sharpe: f64,
    backtest_winrate: f64,
    position_size_pct: f64,
    risk_ok: bool,
    // Step 5 — Final
    final_score: f64,
    decision: String,
    entry: f64,
    stop_loss: f64,
    take_profit: f64,
    risk_reward: f64,
    reasoning: String,
}

// ═══════════════════════════════════════════════════════════════
// ① BƯỚC 1: SCAN THỊ TRƯỜNG RỘNG
// ═══════════════════════════════════════════════════════════════

async fn step1_scan_market(client: &reqwest::Client) -> Vec<CoinScan> {
    println!("\n{}", "═".repeat(70));
    println!("① BƯỚC 1: SCAN THỊ TRƯỜNG RỘNG — Lọc từ 25 coins xuống top candidates");
    println!("{}", "─".repeat(70));

    let symbols = vec![
        "BTCUSDT",
        "ETHUSDT",
        "SOLUSDT",
        "BNBUSDT",
        "XRPUSDT",
        "ADAUSDT",
        "AVAXUSDT",
        "DOGEUSDT",
        "LINKUSDT",
        "DOTUSDT",
        "SUIUSDT",
        "PEPEUSDT",
        "AAVEUSDT",
        "NEARUSDT",
        "APTUSDT",
        "TRXUSDT",
        "LTCUSDT",
        "ARUSDT",
        "INJUSDT",
        "ATOMUSDT",
        "FETUSDT",
        "RENDERUSDT",
        "WIFUSDT",
        "TIAUSDT",
        "SEIUSDT",
    ];

    let fetcher = MarketDataFetcher::new();
    let mut scans = Vec::new();

    for symbol in &symbols {
        // Fetch 24h ticker
        let url = format!(
            "https://api.binance.com/api/v3/ticker/24hr?symbol={}",
            symbol
        );
        let (price, change, vol) = match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let t: serde_json::Value = resp.json().await.unwrap_or_default();
                let p = t["lastPrice"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let c = t["priceChangePercent"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let v = t["quoteVolume"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                (p, c, v)
            }
            _ => continue,
        };

        // Filter: volume < $1M → skip (low liquidity)
        if vol < 1_000_000.0 {
            continue;
        }

        // Fetch 1h candles for Hurst
        let (hurst, mc, strat) = match fetcher.fetch_klines(symbol, "1h", Some(200)).await {
            Ok(candles) if candles.len() >= 100 => {
                let ohlcv = to_ohlcv(&candles);
                let closes: Vec<f64> = ohlcv.iter().map(|c| c.close).collect();
                let start = closes.len().saturating_sub(101);
                match HurstExponent::compute(&closes[start..]) {
                    Some(h) => {
                        let (mc, sh) = if h > 0.55 {
                            (
                                "Trending",
                                if change > 0.0 {
                                    "Trend-Follow"
                                } else {
                                    "Trend-Short"
                                },
                            )
                        } else if h < 0.45 {
                            (
                                "Mean-Revert",
                                if change < -2.0 {
                                    "MR-Buy"
                                } else if change > 2.0 {
                                    "MR-Sell"
                                } else {
                                    "Range"
                                },
                            )
                        } else {
                            ("RandomWalk", "CAUTION")
                        };
                        (Some(h), mc, sh)
                    }
                    None => (None, "Unknown", "Standard"),
                }
            }
            _ => (None, "Unknown", "Standard"),
        };

        // Scan score (quick heuristic)
        let mut scan_score = 50.0;
        scan_score += change * 3.0; // momentum
        if let Some(h) = hurst {
            if h > 0.55 {
                scan_score += 10.0;
            } else if h < 0.45 {
                scan_score += 5.0;
            }
            // mean-revert also has edge
            else {
                scan_score -= 8.0;
            } // random walk = no edge
        }
        if change.abs() > 5.0 {
            scan_score += 5.0;
        } // high volatility = attention
        if vol > 100_000_000.0 {
            scan_score += 3.0;
        } // high volume = liquid

        let scan_score = scan_score.clamp(0.0, 100.0);

        let emoji = match scan_score {
            s if s >= 65.0 => "🟢",
            s if s >= 50.0 => "⚪",
            _ => "🔴",
        };
        println!(
            "  {} {:12} ${:<12.2} {:+6.1}% Vol=${:.0}M  H={} {:12} Score={:.0}",
            emoji,
            symbol,
            price,
            change,
            vol / 1e6,
            hurst.map(|h| format!("{:.2}", h)).unwrap_or("—".into()),
            mc,
            scan_score
        );

        scans.push(CoinScan {
            symbol: symbol.to_string(),
            price,
            change_24h_pct: change,
            volume_24h_usd: vol,
            hurst,
            market_char: mc.to_string(),
            strategy_hint: strat.to_string(),
            scan_score,
        });
    }

    // Filter: only Score ≥ 50 for deep analysis, top 5 only (API rate limits)
    scans.sort_by(|a, b| {
        b.scan_score
            .partial_cmp(&a.scan_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let candidates: Vec<CoinScan> = scans
        .into_iter()
        .filter(|s| s.scan_score >= 50.0)
        .take(3)
        .collect();

    println!(
        "\n  ✅ Lọc được {} coins (Score ≥ 50) cho phân tích sâu:",
        candidates.len()
    );
    for (i, c) in candidates.iter().enumerate() {
        println!(
            "     {}. {} (Score: {:.0}, {})",
            i + 1,
            c.symbol,
            c.scan_score,
            c.market_char
        );
    }

    candidates
}

// ═══════════════════════════════════════════════════════════════
// ② BƯỚC 2: SENTIMENT THỊ TRƯỜNG
// ═══════════════════════════════════════════════════════════════

struct SentimentResult {
    fear_greed_value: i32,
    fear_greed_label: String,
    sentiment_bias: String,
    recommendation: String,
}

async fn step2_sentiment(client: &reqwest::Client) -> SentimentResult {
    println!("\n{}", "─".repeat(70));
    println!("② BƯỚC 2: SENTIMENT THỊ TRƯỜNG");
    println!("{}", "─".repeat(70));

    // Fetch Fear & Greed Index
    let (fg_value, fg_label) = match client
        .get("https://api.alternative.me/fng/?limit=1")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().await.unwrap_or_default();
            let val = data["data"][0]["value"]
                .as_str()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(50);
            let label = data["data"][0]["value_classification"]
                .as_str()
                .unwrap_or("Neutral")
                .to_string();
            (val, label)
        }
        _ => (50, "Neutral".to_string()),
    };

    let (bias, rec) = if fg_value < 25 {
        (
            "Extreme Fear 🟢🟢",
            "Cơ hội MUA mạnh — contrarian".to_string(),
        )
    } else if fg_value < 40 {
        ("Fear 🟢", "Tìm cơ hội BUY, nhưng SL chặt".to_string())
    } else if fg_value < 60 {
        ("Neutral ⚪", "Tuân theo technical, không bias".to_string())
    } else if fg_value < 75 {
        ("Greed 🟡", "Thận trọng, giảm position size".to_string())
    } else {
        (
            "Extreme Greed 🔴",
            "Rủi ro cao — counter-trend SHORT hoặc đứng ngoài".to_string(),
        )
    };

    println!("  📊 Fear & Greed Index: {} ({})", fg_value, fg_label);
    println!("  🎯 Bias: {}", bias);
    println!("  💡 Recommendation: {}", rec);

    SentimentResult {
        fear_greed_value: fg_value,
        fear_greed_label: fg_label,
        sentiment_bias: bias.to_string(),
        recommendation: rec,
    }
}

// ═══════════════════════════════════════════════════════════════
// ③ BƯỚC 3: DEEP ANALYSIS — Multi-TF + Hurst + Signals + S/R
// ═══════════════════════════════════════════════════════════════

async fn step3_deep_analysis(
    candidates: &[CoinScan],
    sentiment: &SentimentResult,
) -> Vec<CoinDeep> {
    println!("\n{}", "─".repeat(70));
    println!("③ BƯỚC 3: DEEP ANALYSIS — Multi-TF + Hurst + Signals + S/R");
    println!("{}", "─".repeat(70));

    let fetcher = MarketDataFetcher::new();
    let timeframes = vec![("1d", "1D"), ("4h", "4H"), ("1h", "1H")];
    let mut deep_results = Vec::new();

    for coin in candidates {
        println!(
            "\n  📊 Phân tích sâu: {} (${:.2}, {:+.1}%)",
            coin.symbol, coin.price, coin.change_24h_pct
        );

        let mut tf_results = Vec::new();
        let mut all_closes_1h: Vec<f64> = Vec::new();

        for (tf_api, tf_label) in &timeframes {
            let num_candles = if *tf_api == "1d" {
                200u32
            } else if *tf_api == "4h" {
                200
            } else {
                200
            };

            let result = match fetcher
                .fetch_klines(&coin.symbol, tf_api, Some(num_candles))
                .await
            {
                Ok(candles) if candles.len() >= 50 => {
                    let ohlcv = to_ohlcv(&candles);
                    let closes: Vec<f64> = ohlcv.iter().map(|c| c.close).collect();

                    if *tf_api == "1h" {
                        all_closes_1h = closes.clone();
                    }

                    let analysis = compute_full_analysis(&closes);
                    let rsi = last_val(&analysis.rsi14);

                    // MACD
                    let macd_last = analysis.macd.last().and_then(|v| v.clone());
                    let macd_sig = match macd_last {
                        Some(m) if m.macd_line > m.signal_line => "BULLISH",
                        Some(_) => "BEARISH",
                        None => "N/A",
                    };

                    // BB position
                    let bb_last = analysis.bb.last().and_then(|v| v.clone());
                    let bb_pos = match bb_last {
                        Some(b) => {
                            let range = b.upper - b.lower;
                            if range > 0.0 {
                                let pos = (coin.price - b.lower) / range;
                                if pos > 0.8 {
                                    "UPPER"
                                } else if pos < 0.2 {
                                    "LOWER"
                                } else {
                                    "MID"
                                }
                            } else {
                                "N/A"
                            }
                        }
                        None => "N/A",
                    };

                    // ALMA
                    let alma10 = last_val(&analysis.alma10);
                    let alma30 = last_val(&analysis.alma30);
                    let alma_sig = match (alma10, alma30) {
                        (Some(f), Some(s)) if f > s => "BULLISH",
                        (Some(_), Some(_)) => "BEARISH",
                        _ => "N/A",
                    };

                    // Hurst for this TF
                    let h = {
                        let start = closes.len().saturating_sub(101);
                        HurstExponent::compute(&closes[start..])
                    };

                    // ATR
                    let atr = last_val(&analysis.atr14);

                    let regime = match h {
                        Some(hv) if hv > 0.55 => "📈 Trending",
                        Some(hv) if hv < 0.45 => "🔄 Mean-Revert",
                        Some(_) => "🎲 RandomWalk",
                        None => "❓ Unknown",
                    };

                    Some(MultiTfResult {
                        timeframe: tf_label.to_string(),
                        hurst: h,
                        regime: regime.to_string(),
                        rsi,
                        macd_signal: macd_sig.to_string(),
                        alma_signal: alma_sig.to_string(),
                        bb_pos: bb_pos.to_string(),
                        atr,
                    })
                }
                _ => None,
            };

            if let Some(r) = result {
                let h_str = r.hurst.map(|h| format!("{:.2}", h)).unwrap_or("—".into());
                let rsi_str = r.rsi.map(|r| format!("{:.0}", r)).unwrap_or("—".into());
                println!(
                    "    {} │ H={} │ {} │ RSI={} │ MACD={:7} │ ALMA={:7} │ BB={}",
                    r.timeframe, h_str, r.regime, rsi_str, r.macd_signal, r.alma_signal, r.bb_pos
                );
                tf_results.push(r);
            }
        }

        // ── Multi-TF Confirmation ──
        let tf_confirm_count = {
            let bullish_tfs = tf_results
                .iter()
                .filter(|t| t.alma_signal == "BULLISH")
                .count();
            let bearish_tfs = tf_results
                .iter()
                .filter(|t| t.alma_signal == "BEARISH")
                .count();
            bullish_tfs.max(bearish_tfs) as i32
        };
        let tf_count = tf_results.len().max(1) as i32;
        let tf_consensus = tf_confirm_count >= 2; // At least 2/3 TF agree

        // ── Hurst Consensus ──
        let (hurst_consensus, strategy) = {
            let trending_count = tf_results
                .iter()
                .filter(|t| t.hurst.map_or(false, |h| h > 0.55))
                .count();
            let mr_count = tf_results
                .iter()
                .filter(|t| t.hurst.map_or(false, |h| h < 0.45))
                .count();
            let rw_count = tf_results
                .iter()
                .filter(|t| t.hurst.map_or(false, |h| (0.45..=0.55).contains(&h)))
                .count();

            if trending_count >= 2 {
                (
                    "Trending".to_string(),
                    "Trend-Follow (ALMA + SuperSmoother, SL rộng)".to_string(),
                )
            } else if mr_count >= 2 {
                (
                    "Mean-Reverting".to_string(),
                    "Mean-Revert (BB bounce + RSI, SL chặt)".to_string(),
                )
            } else if rw_count >= 2 {
                (
                    "RandomWalk".to_string(),
                    "⚠️ TRÁNH hoặc giảm size (không predictable)".to_string(),
                )
            } else {
                (
                    "Mixed".to_string(),
                    "Tuân theo timeframe ưu thế".to_string(),
                )
            }
        };

        // ── Trading Signals Count ──
        let mut buy_signals = 0;
        let mut sell_signals = 0;
        let mut neutral_signals = 0;

        for tf in &tf_results {
            if tf.macd_signal == "BULLISH" {
                buy_signals += 1;
            } else if tf.macd_signal == "BEARISH" {
                sell_signals += 1;
            }
            if tf.alma_signal == "BULLISH" {
                buy_signals += 1;
            } else if tf.alma_signal == "BEARISH" {
                sell_signals += 1;
            }
            if tf.bb_pos == "LOWER" {
                buy_signals += 1;
            } else if tf.bb_pos == "UPPER" {
                sell_signals += 1;
            }
            if let Some(rsi) = tf.rsi {
                if rsi < 35.0 {
                    buy_signals += 2;
                }
                // Oversold = strong buy
                else if rsi < 45.0 {
                    buy_signals += 1;
                } else if rsi > 65.0 {
                    sell_signals += 1;
                } else if rsi > 75.0 {
                    sell_signals += 2;
                }
                // Overbought = strong sell
                else {
                    neutral_signals += 1;
                }
            }
        }

        println!(
            "    Signals → 🟢 BUY: {} │ 🔴 SELL: {} │ ⚪ NEUTRAL: {}",
            buy_signals, sell_signals, neutral_signals
        );
        println!("    Hurst Consensus: {} → {}", hurst_consensus, strategy);
        println!(
            "    Multi-TF Confirm: {} ({}/{} TFs đồng thuận)",
            if tf_consensus { "✅" } else { "❌" },
            tf_confirm_count,
            tf_count
        );

        // ── Support/Resistance from price structure ──
        let (support, resistance) = compute_sr(&all_closes_1h, coin.price);
        println!(
            "    S/R → Support: ${:.2} │ Resistance: ${:.2}",
            support, resistance
        );

        // ── Step 4: Quick Backtest ──
        let (bt_sharpe, bt_winrate) = quick_backtest(&all_closes_1h);
        println!(
            "    Backtest → Sharpe: {:.2} │ WinRate: {:.0}%",
            bt_sharpe, bt_winrate
        );

        // ── Step 4: Risk Check ──
        let risk_ok = bt_winrate > 40.0 && bt_sharpe > -0.5;
        let pos_size_pct = if sentiment.fear_greed_value > 70 {
            0.5
        } else if sentiment.fear_greed_value < 30 {
            1.5
        } else {
            1.0
        };

        // ── Step 5: Final Scoring (Weighted) ──
        let mut final_score = 0.0;

        // Factor 1: Hurst Exponent (25%)
        let hurst_score = {
            let avg_h = tf_results.iter().filter_map(|t| t.hurst).sum::<f64>()
                / tf_results.iter().filter_map(|t| t.hurst).count().max(1) as f64;
            if avg_h > 0.55 {
                85.0 + (avg_h - 0.55) * 100.0
            } else if avg_h < 0.45 {
                60.0 + (0.45 - avg_h) * 80.0
            } else {
                30.0
            } // Random walk
        };
        final_score += hurst_score * 0.25;

        // Factor 2: Signals consensus (20%)
        let total_signals = (buy_signals + sell_signals).max(1);
        let signal_score = if buy_signals > sell_signals {
            (buy_signals as f64 / total_signals as f64) * 100.0
        } else {
            (sell_signals as f64 / total_signals as f64) * 100.0
        };
        final_score += signal_score * 0.20;

        // Factor 3: Multi-TF confirmation (20%)
        let mtf_score = if tf_consensus { 80.0 } else { 35.0 };
        final_score += mtf_score * 0.20;

        // Factor 4: Sentiment phù hợp (15%)
        let sent_score = if sentiment.fear_greed_value < 40 {
            if buy_signals > sell_signals {
                80.0
            } else {
                40.0
            } // Fear + BUY signals = contrarian win
        } else if sentiment.fear_greed_value > 70 {
            if buy_signals > sell_signals {
                50.0
            } else {
                65.0
            } // Greed + SELL = contrarian win
        } else {
            60.0 // Neutral
        };
        final_score += sent_score * 0.15;

        // Factor 5: Backtest profitable (10%)
        let bt_score = if bt_sharpe > 1.0 && bt_winrate > 50.0 {
            85.0
        } else if bt_sharpe > 0.0 {
            60.0
        } else {
            25.0
        };
        final_score += bt_score * 0.10;

        // Factor 6: Risk/Reward (10%) — based on S/R
        let atr = tf_results
            .iter()
            .find_map(|t| t.atr)
            .unwrap_or(coin.price * 0.02);
        let side = if buy_signals > sell_signals {
            "LONG"
        } else if sell_signals > buy_signals {
            "SHORT"
        } else {
            "WAIT"
        };

        let (entry, sl, tp, rr) = if side == "LONG" {
            let entry = coin.price;
            let sl = entry - 1.5 * atr;
            let tp = if resistance > entry {
                resistance
            } else {
                entry + 2.5 * atr
            };
            let risk = entry - sl;
            let reward = tp - entry;
            (entry, sl, tp, if risk > 0.0 { reward / risk } else { 0.0 })
        } else if side == "SHORT" {
            let entry = coin.price;
            let sl = entry + 1.5 * atr;
            let tp = if support < entry {
                support
            } else {
                entry - 2.5 * atr
            };
            let risk = sl - entry;
            let reward = entry - tp;
            (entry, sl, tp, if risk > 0.0 { reward / risk } else { 0.0 })
        } else {
            (coin.price, 0.0, 0.0, 0.0)
        };

        let rr_score = if rr >= 2.0 {
            90.0
        } else if rr >= 1.5 {
            70.0
        } else if rr >= 1.0 {
            40.0
        } else {
            15.0
        };
        final_score += rr_score * 0.10;

        let final_score = final_score.clamp(0.0, 100.0);

        // ── Decision ──
        let (decision, reasoning) =
            if final_score >= 65.0 && hurst_consensus != "RandomWalk" && risk_ok && tf_consensus {
                (
                    "🟢 VÀO LỆNH".to_string(),
                    format!(
                        "Score {:.0} ≥ 65, {} regime, {}/{} TF confirm, risk OK",
                        final_score, hurst_consensus, tf_confirm_count, tf_count
                    ),
                )
            } else if final_score >= 50.0 {
                (
                    "🟡 CHỜ".to_string(),
                    format!(
                        "Score {:.0} (50-64), mixed signals — đặt alert, chờ pullback",
                        final_score
                    ),
                )
            } else {
                (
                    "🔴 TRÁNH".to_string(),
                    format!(
                        "Score {:.0} < 50 hoặc {} — không giao dịch",
                        final_score, hurst_consensus
                    ),
                )
            };

        println!("    ─────────────────────────────────");
        println!("    🏆 FINAL SCORE: {:.0}/100 → {}", final_score, decision);
        println!("    📝 {}", reasoning);

        deep_results.push(CoinDeep {
            symbol: coin.symbol.clone(),
            price: coin.price,
            change_24h: coin.change_24h_pct,
            volume_24h: coin.volume_24h_usd,
            tf_results,
            hurst_consensus,
            strategy,
            buy_signals,
            sell_signals,
            neutral_signals,
            support,
            resistance,
            backtest_sharpe: bt_sharpe,
            backtest_winrate: bt_winrate,
            position_size_pct: pos_size_pct,
            risk_ok,
            final_score,
            decision,
            entry,
            stop_loss: sl,
            take_profit: tp,
            risk_reward: rr,
            reasoning,
        });
    }

    deep_results
}

/// Compute Support/Resistance from price structure
fn compute_sr(closes: &[f64], current_price: f64) -> (f64, f64) {
    if closes.len() < 20 {
        return (current_price * 0.97, current_price * 1.03);
    }

    // Find local min/max in recent 100 candles
    let recent = &closes[closes.len().saturating_sub(100)..];
    let mut supports = Vec::new();
    let mut resistances = Vec::new();

    for i in 2..recent.len().saturating_sub(2) {
        if recent[i] < recent[i - 1] && recent[i] < recent[i + 1] && recent[i] < current_price {
            supports.push(recent[i]);
        }
        if recent[i] > recent[i - 1] && recent[i] > recent[i + 1] && recent[i] > current_price {
            resistances.push(recent[i]);
        }
    }

    // Find nearest support (highest below price) and resistance (lowest above price)
    let support = supports
        .iter()
        .copied()
        .filter(|&s| s < current_price)
        .fold(f64::MIN, f64::max)
        .max(current_price * 0.97);
    let resistance = resistances
        .iter()
        .copied()
        .filter(|&r| r > current_price)
        .fold(f64::MAX, f64::min)
        .min(current_price * 1.03);

    (support, resistance)
}

/// Quick SMA crossover backtest
fn quick_backtest(closes: &[f64]) -> (f64, f64) {
    if closes.len() < 50 {
        return (0.0, 50.0);
    }

    let sma_short = 10;
    let sma_long = 30;

    let mut wins = 0usize;
    let mut total = 0usize;
    let mut returns = Vec::new();

    for i in (sma_long + 1)..closes.len().saturating_sub(1) {
        let sma_s: f64 = closes[(i - sma_short)..i].iter().sum::<f64>() / sma_short as f64;
        let sma_l: f64 = closes[(i - sma_long)..i].iter().sum::<f64>() / sma_long as f64;
        let prev_sma_s: f64 =
            closes[(i - sma_short - 1)..(i - 1)].iter().sum::<f64>() / sma_short as f64;
        let prev_sma_l: f64 =
            closes[(i - sma_long - 1)..(i - 1)].iter().sum::<f64>() / sma_long as f64;

        // Crossover detection
        let bullish_cross = prev_sma_s <= prev_sma_l && sma_s > sma_l;
        let bearish_cross = prev_sma_s >= prev_sma_l && sma_s < sma_l;

        if bullish_cross {
            let ret = (closes[i + 1] - closes[i]) / closes[i];
            returns.push(ret);
            total += 1;
            if ret > 0.0 {
                wins += 1;
            }
        }
        if bearish_cross {
            let ret = (closes[i] - closes[i + 1]) / closes[i];
            returns.push(ret);
            total += 1;
            if ret > 0.0 {
                wins += 1;
            }
        }
    }

    if total == 0 {
        return (0.0, 50.0);
    }

    let winrate = (wins as f64 / total as f64) * 100.0;
    let avg_ret = returns.iter().sum::<f64>() / returns.len() as f64;
    let std_ret = {
        let variance =
            returns.iter().map(|r| (r - avg_ret).powi(2)).sum::<f64>() / returns.len() as f64;
        variance.sqrt()
    };
    let sharpe = if std_ret > 0.0 {
        (avg_ret / std_ret) * (252.0_f64).sqrt()
    } else {
        0.0
    };

    (sharpe, winrate)
}

// ═══════════════════════════════════════════════════════════════
// ④ + ⑤ FINAL REPORT
// ═══════════════════════════════════════════════════════════════

fn print_final_report(results: &[CoinDeep], sentiment: &SentimentResult) {
    // Sort by final_score desc
    let mut sorted = results.to_vec();
    sorted.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("\n{}", "═".repeat(80));
    println!("🏆 BONBO FINAL TRADING ANALYSIS REPORT");
    println!(
        "   Fear/Greed: {} ({}) │ Analyzed: {} coins",
        sentiment.fear_greed_value,
        sentiment.fear_greed_label,
        sorted.len()
    );
    println!("{}\n", "═".repeat(80));

    // ── TRADE recommendations ──
    let trades: Vec<_> = sorted
        .iter()
        .filter(|r| r.decision.contains("VÀO LỆNH"))
        .collect();
    let waits: Vec<_> = sorted
        .iter()
        .filter(|r| r.decision.contains("CHỜ"))
        .collect();
    let avoids: Vec<_> = sorted
        .iter()
        .filter(|r| r.decision.contains("TRÁNH"))
        .collect();

    if !trades.is_empty() {
        println!("🟢 ══════════════════════════════════════════════════════");
        println!("🟢   QUYẾT ĐỊNH: VÀO LỆNH ({} coins)", trades.len());
        println!("🟢 ══════════════════════════════════════════════════════\n");

        for (i, t) in trades.iter().enumerate() {
            let side = if t.buy_signals > t.sell_signals {
                "LONG 🟢"
            } else {
                "SHORT 🔴"
            };
            println!(
                "  {} ══════════════════════════════════════════════════",
                if side.contains("LONG") {
                    "🟢"
                } else {
                    "🔴"
                }
            );
            println!("  TRADE #{}: {} {}", i + 1, t.symbol, side);
            println!(
                "  {} ══════════════════════════════════════════════════",
                if side.contains("LONG") {
                    "🟢"
                } else {
                    "🔴"
                }
            );
            println!(
                "  💰 Price:         ${:.2} ({:+.1}%)",
                t.price, t.change_24h
            );
            println!("  🎯 Score:         {:.0}/100", t.final_score);
            println!("  📊 Hurst:         {} → {}", t.hurst_consensus, t.strategy);
            println!(
                "  📉 Signals:       🟢{} / 🔴{} / ⚪{}",
                t.buy_signals, t.sell_signals, t.neutral_signals
            );
            println!(
                "  📈 Backtest:      Sharpe={:.2} WinRate={:.0}%",
                t.backtest_sharpe, t.backtest_winrate
            );
            println!(
                "  📏 S/R:           Support=${:.2} │ Resistance=${:.2}",
                t.support, t.resistance
            );
            println!("  📌 Entry:         ${:.2}", t.entry);
            println!(
                "  🛑 Stop Loss:     ${:.2} ({:.1}%)",
                t.stop_loss,
                ((t.stop_loss - t.entry) / t.entry).abs() * 100.0
            );
            println!(
                "  🏆 Take Profit:   ${:.2} ({:.1}%)",
                t.take_profit,
                ((t.take_profit - t.entry) / t.entry).abs() * 100.0
            );
            println!("  ⚖️  Risk/Reward:   {:.1}:1", t.risk_reward);
            println!("  💼 Position Size: {:.1}% equity", t.position_size_pct);
            println!("  📝 Reason: {}", t.reasoning);
            println!();
        }
    }

    if !waits.is_empty() {
        println!("🟡 ══════════════════════════════════════════════════════");
        println!(
            "🟡   CHỜ XEM ({} coins) — Đặt alert, chờ pullback",
            waits.len()
        );
        println!("🟡 ══════════════════════════════════════════════════════\n");
        for w in &waits {
            println!(
                "  ⏳ {} — Score: {:.0} │ {} │ {}",
                w.symbol, w.final_score, w.hurst_consensus, w.reasoning
            );
        }
        println!();
    }

    if !avoids.is_empty() {
        println!("🔴 ══════════════════════════════════════════════════════");
        println!("🔴   TRÁNH ({} coins) — Không giao dịch", avoids.len());
        println!("🔴 ══════════════════════════════════════════════════════\n");
        for a in &avoids {
            println!(
                "  ❌ {} — Score: {:.0} │ {}",
                a.symbol, a.final_score, a.reasoning
            );
        }
        println!();
    }

    // ── Full Ranking Table ──
    println!("📋 ══════════════════════════════════════════════════════");
    println!("📋   FULL RANKING TABLE");
    println!("📋 ══════════════════════════════════════════════════════\n");

    println!(
        "| #  | Symbol      | Price         | 24h%    | Score | Hurst     | Signals   | R:R  | Decision    |"
    );
    println!(
        "|----|-------------|---------------|---------|-------|-----------|-----------|------|-------------|"
    );

    for (i, r) in sorted.iter().enumerate() {
        let sig = format!("🟢{}🔴{}", r.buy_signals, r.sell_signals);
        let rr = format!("{:.1}:1", r.risk_reward);
        println!(
            "| {:2} | {:11} | ${:<13.2} | {:>+6.1}% | {:5.0} | {:9} | {:9} | {:4} | {:11} |",
            i + 1,
            r.symbol,
            r.price,
            r.change_24h,
            r.final_score,
            r.hurst_consensus,
            sig,
            rr,
            r.decision
                .replace("🟢 ", "")
                .replace("🟡 ", "")
                .replace("🔴 ", "")
        );
    }

    println!("\n✅ Phân tích hoàn tất theo quy trình 5 bước. Trade responsibly! 🎯");
}

// ═══════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let start = Instant::now();
    println!("🚀 BonBo Trading Analysis Process — 5 Bước");
    println!("   Theo docs/trading-analysis-process.md v1.0\n");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    // ① BƯỚC 1: SCAN
    let candidates = step1_scan_market(&client).await;

    // ② BƯỚC 2: SENTIMENT
    let sentiment = step2_sentiment(&client).await;

    // ③ + ④ + ⑤ DEEP ANALYSIS + BACKTEST + DECISION
    let results = step3_deep_analysis(&candidates, &sentiment).await;

    // FINAL REPORT
    print_final_report(&results, &sentiment);

    println!(
        "\n⏱️ Total analysis time: {:.1}s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
