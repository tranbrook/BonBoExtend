//! Live MCP Client — real market analysis using Binance API + bonbo-ta indicators.
//!
//! Replaces MockMcpClient for production live trading.
//! Uses:
//! - `bonbo-binance-futures` REST API for market data
//! - `bonbo-ta` for indicator computation (RSI, MACD, EMA, Hurst, Laguerre RSI, etc.)

use crate::mcp_client::*;
use async_trait::async_trait;
use bonbo_binance_futures::rest::{FuturesRestClient, MarketClient};
use bonbo_ta::IncrementalIndicator;
use bonbo_ta::indicators::AdxResult;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Helper: convert Decimal to f64 safely.
fn dec_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

/// Live MCP client — calls real Binance API and computes indicators.
pub struct LiveMcpClient {
    client: FuturesRestClient,
    /// Cache for indicator results (symbol+timeframe → result), TTL-based.
    cache: tokio::sync::RwLock<HashMap<String, (i64, IndicatorResult)>>,
    /// Cache TTL in seconds.
    cache_ttl_secs: i64,
}

impl LiveMcpClient {
    /// Create a new live MCP client.
    pub fn new(client: FuturesRestClient) -> Self {
        Self {
            client,
            cache: tokio::sync::RwLock::new(HashMap::new()),
            cache_ttl_secs: 60, // 1 minute cache
        }
    }

    /// Parse klines JSON response into OHLCV data.
    fn parse_klines(klines: &serde_json::Value) -> Vec<bonbo_ta::OhlcvCandle> {
        let arr = match klines.as_array() {
            Some(a) => a,
            None => return Vec::new(),
        };

        arr.iter()
            .filter_map(|k| {
                let inner = k.as_array()?;
                Some(bonbo_ta::OhlcvCandle {
                    timestamp: inner.first()?.as_i64()?,
                    open: inner.get(1)?.as_str()?.parse().ok()?,
                    high: inner.get(2)?.as_str()?.parse().ok()?,
                    low: inner.get(3)?.as_str()?.parse().ok()?,
                    close: inner.get(4)?.as_str()?.parse().ok()?,
                    volume: inner.get(5)?.as_str()?.parse().ok()?,
                })
            })
            .collect()
    }

    /// Compute composite score from indicators (0-100).
    /// Normalize an indicator value to Z-score in [-1, +1] range.
    ///
    /// Research source: trading-process-improvement.md — Enhancement (Z-Score Normalization).
    /// Standardizes indicator outputs before aggregation so no single
    /// indicator dominates the composite score.
    fn z_score_normalize(value: f64, center: f64, scale: f64) -> f64 {
        ((value - center) / scale).clamp(-1.0, 1.0)
    }

    fn compute_score(
        rsi: Option<f64>,
        macd_signal: &Option<String>,
        ema_cross: &Option<String>,
        hurst: Option<f64>,
        laguerre_rsi: Option<f64>,
        is_long: bool,
    ) -> u32 {
        let mut z_sum: f64 = 0.0;
        let mut weight_sum: f64 = 0.0;

        // RSI Z-score: center=50, scale=25 → [-1, +1]
        if let Some(rsi_val) = rsi {
            let z = Self::z_score_normalize(rsi_val, 50.0, 25.0);
            let adjusted = if is_long { z } else { -z };
            z_sum += adjusted * 15.0; // weight 15
            weight_sum += 15.0;
        }

        // MACD: discrete signal
        if let Some(sig) = macd_signal {
            let z = match sig.as_str() {
                "BUY" if is_long => 1.0,
                "SELL" if !is_long => 1.0,
                "BUY" if !is_long => -0.5,
                "SELL" if is_long => -0.5,
                _ => 0.0,
            };
            z_sum += z * 15.0;
            weight_sum += 15.0;
        }

        // EMA cross: discrete signal
        if let Some(cross) = ema_cross {
            let z = match cross.as_str() {
                "BULLISH" if is_long => 1.0,
                "BEARISH" if !is_long => 1.0,
                _ => 0.0,
            };
            z_sum += z * 15.0;
            weight_sum += 15.0;
        }

        // Hurst Z-score: center=0.5, scale=0.15
        if let Some(h) = hurst {
            let z = Self::z_score_normalize(h, 0.5, 0.15);
            // Trending (high Hurst) → good for trend-following
            z_sum += z * 15.0;
            weight_sum += 15.0;
        }

        // LaguerreRSI Z-score: center=0.5, scale=0.3
        if let Some(lrsi) = laguerre_rsi {
            let z = Self::z_score_normalize(lrsi, 0.5, 0.3);
            let adjusted = if is_long { z } else { -z };
            z_sum += adjusted * 15.0;
            weight_sum += 15.0;
        }

        // Convert weighted Z-score sum to 0-100 scale
        // z_sum range: [-weight_sum, +weight_sum]
        // Normalize: (z_sum / weight_sum + 1) / 2 → [0, 1] → * 100
        let normalized = if weight_sum > 0.0 {
            ((z_sum / weight_sum + 1.0) / 2.0 * 100.0).clamp(0.0, 100.0)
        } else {
            50.0 // neutral
        };

        normalized as u32
    }

    /// Determine MACD signal from values.
    fn macd_signal_text(macd_line: f64, signal_line: f64, histogram: f64) -> String {
        if histogram > 0.0 && macd_line > signal_line {
            "BUY".to_string()
        } else if histogram < 0.0 && macd_line < signal_line {
            "SELL".to_string()
        } else {
            "NEUTRAL".to_string()
        }
    }

    /// Determine EMA cross direction.
    fn ema_cross_text(ema12: f64, ema26: f64) -> String {
        if ema12 > ema26 {
            "BULLISH".to_string()
        } else {
            "BEARISH".to_string()
        }
    }
}

#[async_trait]
impl McpClient for LiveMcpClient {
    async fn scan_market(&self, symbols: &[String]) -> anyhow::Result<Vec<ScanResult>> {
        let mut results = Vec::new();

        for symbol in symbols {
            match MarketClient::get_24h_ticker(&self.client, symbol).await {
                Ok(ticker) => {
                    let price = ticker.last_price;
                    let change_pct = dec_to_f64(ticker.price_change_percent);
                    let volume = dec_to_f64(ticker.quote_volume);

                    let quant_score = if volume > 1_000_000_000.0 && change_pct.abs() > 1.0 {
                        Some(80)
                    } else if volume > 100_000_000.0 && change_pct.abs() > 0.5 {
                        Some(70)
                    } else if volume > 10_000_000.0 {
                        Some(50)
                    } else {
                        Some(30)
                    };

                    results.push(ScanResult {
                        symbol: symbol.clone(),
                        price,
                        change_24h_pct: change_pct,
                        volume_24h_usd: volume,
                        quant_score,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to get ticker for {}: {}", symbol, e);
                }
            }
        }

        results.sort_by(|a, b| {
            b.volume_24h_usd
                .partial_cmp(&a.volume_24h_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    async fn analyze_indicators(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> anyhow::Result<IndicatorResult> {
        // Check cache
        let cache_key = format!("{}:{}", symbol, timeframe);
        {
            let cache = self.cache.read().await;
            if let Some((ts, result)) = cache.get(&cache_key) {
                let now = chrono::Utc::now().timestamp();
                if now - ts < self.cache_ttl_secs {
                    tracing::debug!("Cache hit for {}", cache_key);
                    return Ok(result.clone());
                }
            }
        }

        // Fetch klines (200 candles)
        let klines_json = MarketClient::get_klines(&self.client, symbol, timeframe, 200).await?;
        let candles = Self::parse_klines(&klines_json);

        if candles.len() < 50 {
            return Ok(IndicatorResult {
                symbol: symbol.to_string(),
                timeframe: timeframe.to_string(),
                rsi_14: None,
                macd_signal: None,
                ema_cross: None,
                hurst: None,
                hurst_short: None,
                hurst_divergence: None,
                laguerre_rsi: None,
                laguerre_rsi_fast: None,
                laguerre_divergence: None,
                atr_14: None,
                adx: None,
                buy_signals: 0,
                sell_signals: 0,
                total_signals: 0,
                score: 0,
            });
        }

        // Create indicators — all return Option<Self>
        let mut rsi_ind = bonbo_ta::Rsi::new(14).expect("RSI(14) valid");
        let mut macd_ind = bonbo_ta::Macd::new(12, 26, 9).expect("MACD valid");
        let mut ema12 = bonbo_ta::Ema::new(12).expect("EMA(12) valid");
        let mut ema26 = bonbo_ta::Ema::new(26).expect("EMA(26) valid");
        let mut hurst_ind = bonbo_ta::HurstExponent::new(100).expect("Hurst(100) valid");
        let mut hurst_short_ind = bonbo_ta::HurstExponent::new(50).expect("Hurst(50) valid");
        let mut laguerre_slow = bonbo_ta::LaguerreRsi::new(0.8).expect("LaguerreRSI(0.8) valid");
        let mut laguerre_fast = bonbo_ta::LaguerreRsi::new(0.5).expect("LaguerreRSI(0.5) valid");
        let mut adx_ind = bonbo_ta::Adx::new(14).expect("ADX(14) valid");
        let mut atr_ind = bonbo_ta::Atr::new(14).expect("ATR(14) valid");

        let mut rsi_val = None;
        let mut macd_result = None;
        let mut ema12_val = None;
        let mut ema26_val = None;
        let mut hurst_val = None;
        let mut hurst_short_val = None;
        let mut laguerre_slow_val = None;
        let mut laguerre_fast_val = None;
        let mut adx_val: Option<AdxResult> = None;
        let mut atr_val = None;

        for candle in &candles {
            let close = candle.close;
            rsi_val = rsi_ind.next(close);
            macd_result = macd_ind.next(close);
            ema12_val = ema12.next(close);
            ema26_val = ema26.next(close);
            hurst_val = hurst_ind.next(close);
            hurst_short_val = hurst_short_ind.next(close);
            laguerre_slow_val = laguerre_slow.next(close);
            laguerre_fast_val = laguerre_fast.next(close);
            adx_val = adx_ind.next_hlc(candle.high, candle.low, candle.close);
            atr_val = atr_ind.next_hlc(candle.high, candle.low, candle.close);
        }

        // LaguerreRSI divergence: fast - slow
        let laguerre_divergence = match (laguerre_fast_val, laguerre_slow_val) {
            (Some(f), Some(s)) => Some(f - s),
            _ => None,
        };

        // Hurst divergence detection
        let hurst_divergence = match (hurst_short_val, hurst_val) {
            (Some(h_short), Some(h_long)) => {
                let div = bonbo_regime::HurstDivergenceResult::compute(h_short, h_long);
                if div.is_divergent {
                    Some(div.hint)
                } else {
                    None
                }
            }
            _ => None,
        };

        // Extract signals
        let macd_signal = macd_result
            .as_ref()
            .map(|m| Self::macd_signal_text(m.macd_line, m.signal_line, m.histogram));

        let ema_cross = match (ema12_val, ema26_val) {
            (Some(e12), Some(e26)) => Some(Self::ema_cross_text(e12, e26)),
            _ => None,
        };

        // Count signals
        let mut buy_signals = 0u32;
        let mut sell_signals = 0u32;

        if let Some(sig) = &macd_signal {
            if sig == "BUY" {
                buy_signals += 1;
            }
            if sig == "SELL" {
                sell_signals += 1;
            }
        }
        if let Some(cross) = &ema_cross {
            if cross == "BULLISH" {
                buy_signals += 1;
            }
            if cross == "BEARISH" {
                sell_signals += 1;
            }
        }
        if let Some(rsi_v) = rsi_val {
            if rsi_v < 30.0 {
                buy_signals += 1;
            }
            if rsi_v > 70.0 {
                sell_signals += 1;
            }
        }
        if let Some(lrsi) = laguerre_slow_val {
            if lrsi < 0.2 {
                buy_signals += 1;
            }
            if lrsi > 0.8 {
                sell_signals += 1;
            }
        }
        // LaguerreRSI divergence signals
        if let Some(div) = laguerre_divergence {
            if div > 0.3 {
                buy_signals += 1;
            }
            if div < -0.3 {
                sell_signals += 1;
            }
        }
        if let Some(h) = hurst_val {
            if h > 0.55 {
                buy_signals += 1;
            }
            if h < 0.45 {
                sell_signals += 1;
            }
        }

        let score = Self::compute_score(
            rsi_val,
            &macd_signal,
            &ema_cross,
            hurst_val,
            laguerre_slow_val,
            buy_signals > sell_signals,
        );

        let result = IndicatorResult {
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            rsi_14: rsi_val,
            macd_signal,
            ema_cross,
            hurst: hurst_val,
            hurst_short: hurst_short_val,
            hurst_divergence,
            laguerre_rsi: laguerre_slow_val,
            laguerre_rsi_fast: laguerre_fast_val,
            laguerre_divergence,
            atr_14: atr_val,
            adx: adx_val.map(|a| a.adx),
            buy_signals,
            sell_signals,
            total_signals: buy_signals + sell_signals,
            score,
        };

        tracing::info!(
            "📊 {} indicators: RSI={:?} MACD={:?} EMA={:?} H={:?} LRSI={:?} → score={} (buy={}, sell={})",
            symbol,
            result.rsi_14,
            result.macd_signal,
            result.ema_cross,
            result.hurst,
            result.laguerre_rsi,
            score,
            buy_signals,
            sell_signals
        );

        // Cache result
        {
            let mut cache = self.cache.write().await;
            let now = chrono::Utc::now().timestamp();
            cache.insert(cache_key, (now, result.clone()));
        }

        Ok(result)
    }

    async fn detect_regime(&self, symbol: &str) -> anyhow::Result<RegimeResult> {
        let klines_json = MarketClient::get_klines(&self.client, symbol, "4h", 200).await?;
        let candles = Self::parse_klines(&klines_json);

        if candles.len() < 50 {
            return Ok(RegimeResult {
                symbol: symbol.to_string(),
                regime: "Unknown".to_string(),
                confidence: 0.0,
            });
        }

        let mut hurst_ind = bonbo_ta::HurstExponent::new(100).expect("Hurst(100) valid");
        let mut hurst_val = None;
        for candle in &candles {
            hurst_val = hurst_ind.next(candle.close);
        }

        let mut adx_ind = bonbo_ta::Adx::new(14).expect("ADX(14) valid");
        let mut adx_val: Option<AdxResult> = None;
        for candle in &candles {
            adx_val = adx_ind.next_hlc(candle.high, candle.low, candle.close);
        }

        let adx_f64 = adx_val.map(|r| r.adx);
        let h = hurst_val.unwrap_or(0.5);
        let adx = adx_f64.unwrap_or(20.0);

        let (regime, confidence) = if adx > 35.0 && h > 0.55 {
            ("Trending".to_string(), 0.85)
        } else if adx > 25.0 && h > 0.5 {
            ("Trending".to_string(), 0.65)
        } else if adx < 15.0 && h < 0.45 {
            ("Mean-Reverting".to_string(), 0.6)
        } else {
            ("Ranging".to_string(), 0.4)
        };

        tracing::info!(
            "📐 {} regime: {} (conf={:.2}, hurst={:.3}, adx={:.1})",
            symbol,
            regime,
            confidence,
            h,
            adx
        );

        Ok(RegimeResult {
            symbol: symbol.to_string(),
            regime,
            confidence,
        })
    }

    async fn get_trading_signals(
        &self,
        symbol: &str,
        timeframe: &str,
    ) -> anyhow::Result<Vec<TradingSignal>> {
        let indicator = self.analyze_indicators(symbol, timeframe).await?;
        let regime = self.detect_regime(symbol).await?;

        if indicator.score < 65 {
            tracing::info!(
                "⏭️ No signal for {} — score={} < 65",
                symbol,
                indicator.score
            );
            return Ok(vec![]);
        }

        if regime.regime == "Mean-Reverting" || regime.regime == "Ranging" {
            tracing::info!(
                "⏭️ No signal for {} — regime={} not suitable",
                symbol,
                regime.regime
            );
            return Ok(vec![]);
        }

        let price_info = MarketClient::get_price(&self.client, symbol).await?;
        let current_price = dec_to_f64(price_info.price);

        if current_price <= 0.0 {
            anyhow::bail!("Invalid price for {}", symbol);
        }

        let is_long = indicator.buy_signals > indicator.sell_signals;

        // Calculate ATR for SL/TP
        let klines_json = MarketClient::get_klines(&self.client, symbol, timeframe, 30).await?;
        let candles = Self::parse_klines(&klines_json);
        let mut atr_ind = bonbo_ta::Atr::new(14).expect("ATR(14) valid");
        let mut atr_val = None;
        for candle in &candles {
            atr_val = atr_ind.next_hlc(candle.high, candle.low, candle.close);
        }
        let atr = atr_val.unwrap_or(current_price * 0.02);

        let entry = current_price;
        let (sl, tp) = if is_long {
            ((entry - 2.0 * atr).max(0.0), entry + 3.0 * atr)
        } else {
            (entry + 2.0 * atr, (entry - 3.0 * atr).max(0.0))
        };

        let signal = TradingSignal {
            symbol: symbol.to_string(),
            side: if is_long {
                "BUY".to_string()
            } else {
                "SELL".to_string()
            },
            entry_price: Decimal::from_f64_retain(entry).unwrap_or(Decimal::ZERO),
            stop_loss: Decimal::from_f64_retain(sl).unwrap_or(Decimal::ZERO),
            take_profit: Decimal::from_f64_retain(tp).unwrap_or(Decimal::ZERO),
            score: indicator.score,
            strategy: "BonBo_Composite".to_string(),
        };

        tracing::info!(
            "📡 Signal: {} {} @ {} SL={} TP={} score={}",
            signal.side,
            signal.symbol,
            signal.entry_price,
            signal.stop_loss,
            signal.take_profit,
            signal.score
        );

        Ok(vec![signal])
    }

    async fn get_funding_rate(&self, symbol: &str) -> anyhow::Result<Decimal> {
        match MarketClient::get_funding_rate(&self.client, symbol).await {
            Ok(rate) => Ok(rate.funding_rate),
            Err(e) => {
                tracing::warn!("Failed to get funding rate for {}: {}", symbol, e);
                Ok(Decimal::ZERO)
            }
        }
    }
}
