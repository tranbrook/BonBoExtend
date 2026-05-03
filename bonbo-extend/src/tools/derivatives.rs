//! Derivatives Data Tool Plugin — futures market microstructure data.
//!
//! Provides 5 tools for analyzing derivatives market data:
//! - `get_funding_rate` — Funding rate history and trend analysis
//! - `get_open_interest` — Open interest levels and OI/price divergence
//! - `get_trader_sentiment` — Long/Short ratio from top traders and global
//! - `get_taker_volume` — Taker buy/sell ratio and pressure analysis
//! - `get_volume_profile` — Volume profile with POC and value area

use async_trait::async_trait;
use bonbo_extend_core::*;

/// Plugin that provides derivatives market microstructure tools.
pub struct DerivativesPlugin {
    metadata: PluginMetadata,
}

impl DerivativesPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-derivatives".to_string(),
                name: "Derivatives Market Data".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Futures derivatives: funding rate, open interest, L/S ratio, taker volume, volume profile".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec![
                    "trading".to_string(),
                    "derivatives".to_string(),
                    "futures".to_string(),
                    "market-microstructure".to_string(),
                ],
            },
        }
    }
}

impl Default for DerivativesPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolPlugin for DerivativesPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "get_funding_rate".to_string(),
                description: "Get funding rate for a symbol from Binance Futures. Shows current rate, trend, annualized cost, and whether longs or shorts are paying. Negative funding = shorts pay longs (bullish signal).".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol (e.g., BTCUSDT, ETHUSDT)".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "limit".to_string(),
                        param_type: "number".to_string(),
                        description: "Number of historical funding periods to fetch (default: 30, max: 100)".to_string(),
                        required: false,
                        default: Some(serde_json::json!(30)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_open_interest".to_string(),
                description: "Get open interest data for a symbol. Shows current OI, 24h OI change, OI trend, and OI/price divergence analysis.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol (e.g., BTCUSDT)".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_trader_sentiment".to_string(),
                description: "Get long/short ratios from Binance Futures. Shows top trader positions, global account ratio, and sentiment classification.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol (e.g., BTCUSDT)".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_taker_volume".to_string(),
                description: "Get taker buy/sell volume ratio. Shows whether aggressive buyers or sellers dominate. Rising taker buy ratio = buying pressure.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol (e.g., BTCUSDT)".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_volume_profile".to_string(),
                description: "Get volume profile analysis for a symbol and timeframe. Shows POC (Point of Control), Value Area (70% volume zone), and price histogram.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol (e.g., BTCUSDT)".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "interval".to_string(),
                        param_type: "string".to_string(),
                        description: "Kline interval for volume analysis (default: 4h)".to_string(),
                        required: false,
                        default: Some(serde_json::json!("4h")),
                        r#enum: Some(vec!["15m".into(), "1h".into(), "4h".into(), "1d".into()]),
                    },
                    ParameterSchema {
                        name: "buckets".to_string(),
                        param_type: "number".to_string(),
                        description: "Number of price buckets for histogram (default: 12, max: 20)".to_string(),
                        required: false,
                        default: Some(serde_json::json!(12)),
                        r#enum: None,
                    },
                ],
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        _context: &PluginContext,
    ) -> anyhow::Result<String> {
        match tool_name {
            "get_funding_rate" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let limit = arguments["limit"].as_u64().unwrap_or(30).min(100) as u32;
                fetch_funding_rate(symbol, limit).await
            }
            "get_open_interest" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                fetch_open_interest(symbol).await
            }
            "get_trader_sentiment" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                fetch_trader_sentiment(symbol).await
            }
            "get_taker_volume" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                fetch_taker_volume(symbol).await
            }
            "get_volume_profile" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let interval = arguments["interval"].as_str().unwrap_or("4h");
                let buckets = arguments["buckets"].as_u64().unwrap_or(12).min(20) as usize;
                fetch_volume_profile(symbol, interval, buckets).await
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════
//  HELPERS
// ══════════════════════════════════════════════════════════════════════

/// Make a GET request to Binance API and return JSON.
async fn binance_get(url: &str) -> anyhow::Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header("User-Agent", "BonBoExtend/1.0")
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Binance API error: {} — {}", status, body));
    }

    Ok(resp.json().await?)
}

/// Make a GET request and return as Vec<Value>.
async fn binance_get_array(url: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    binance_get(url)
        .await
        .map(|v| v.as_array().cloned().unwrap_or_default())
}

// ══════════════════════════════════════════════════════════════════════
//  TOOL 1: FUNDING RATE
// ══════════════════════════════════════════════════════════════════════

async fn fetch_funding_rate(symbol: &str, limit: u32) -> anyhow::Result<String> {
    let sym = symbol.to_uppercase();
    let url = format!(
        "https://fapi.binance.com/fapi/v1/fundingRate?symbol={}&limit={}",
        sym, limit
    );

    let data = binance_get_array(&url).await?;
    if data.is_empty() {
        return Ok(format!("No funding rate data for {}", sym));
    }

    let rates: Vec<f64> = data
        .iter()
        .filter_map(|d| {
            d["fundingRate"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
        })
        .collect();

    if rates.is_empty() {
        return Ok(format!("No valid funding rates for {}", sym));
    }

    let current = *rates.last().unwrap_or(&0.0);
    let avg = rates.iter().sum::<f64>() / rates.len() as f64;
    let max_r = rates.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_r = rates.iter().cloned().fold(f64::INFINITY, f64::min);
    let annualized = current * 3.0 * 365.0 * 100.0;

    // Trend: compare recent 5 vs old 5
    let recent_avg = rates.iter().rev().take(5).sum::<f64>() / 5.0_f64.min(rates.len() as f64);
    let old_avg = rates.iter().take(5).sum::<f64>() / 5.0_f64.min(rates.len() as f64);
    let trend = if (recent_avg - old_avg).abs() < 1e-6 {
        "FLAT"
    } else if recent_avg > old_avg {
        "INCREASING"
    } else {
        "DECREASING"
    };

    // Consecutive same-sign
    let mut consecutive = 0;
    for r in rates.iter().rev() {
        if (current >= 0.0 && *r >= 0.0) || (current < 0.0 && *r < 0.0) {
            consecutive += 1;
        } else {
            break;
        }
    }

    let payer = if current >= 0.0 {
        "LONG pays SHORT"
    } else {
        "SHORT pays LONG"
    };

    let signal = if current < -0.0005 {
        "STRONGLY BULLISH - shorts are paying heavily"
    } else if current < 0.0 {
        "BULLISH - shorts are paying"
    } else if current > 0.0005 {
        "BEARISH - longs are paying heavily"
    } else {
        "NEUTRAL - balanced funding"
    };

    let mut out = format!(
        "Funding Rate - {}\n\n\
         Current: {:.4}% ({})\n\
         {}-period avg: {:.4}%\n\
         Min/Max: {:.4}% / {:.4}%\n\
         Annualized: {:.1}%\n\
         Trend: {}\n\
         Consecutive {} periods: {}\n\n\
         Signal: {}\n\n",
        sym,
        current * 100.0,
        payer,
        limit,
        avg * 100.0,
        min_r * 100.0,
        max_r * 100.0,
        annualized,
        trend,
        if current >= 0.0 {
            "positive"
        } else {
            "negative"
        },
        consecutive,
        signal,
    );

    // Last 6 periods
    out.push_str("Recent Funding Periods:\n");
    for d in data.iter().rev().take(8) {
        let ts = d["fundingTime"].as_u64().unwrap_or(0);
        let rate = d["fundingRate"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let time_str = chrono::DateTime::from_timestamp_millis(ts as i64)
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let emoji = if rate >= 0.0 { "L" } else { "S" };
        out.push_str(&format!(
            "  [{}] {}  {:.4}%\n",
            emoji,
            time_str,
            rate * 100.0
        ));
    }

    Ok(out)
}

// ══════════════════════════════════════════════════════════════════════
//  TOOL 2: OPEN INTEREST
// ══════════════════════════════════════════════════════════════════════

async fn fetch_open_interest(symbol: &str) -> anyhow::Result<String> {
    let sym = symbol.to_uppercase();

    // Current OI
    let oi_data = binance_get(&format!(
        "https://fapi.binance.com/fapi/v1/openInterest?symbol={}",
        sym
    ))
    .await?;
    let oi_qty = oi_data["openInterest"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    // Current price
    let price_data = binance_get(&format!(
        "https://fapi.binance.com/fapi/v1/ticker/price?symbol={}",
        sym
    ))
    .await?;
    let price = price_data["price"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let oi_usd = oi_qty * price;
    let base = sym.strip_suffix("USDT").unwrap_or(&sym);

    // OI history for trend
    let oih_data = binance_get_array(&format!(
        "https://fapi.binance.com/futures/data/openInterestHist?symbol={}&period=4h&limit=30",
        sym
    ))
    .await?;

    let (oi_24h_change, oi_trend) = if oih_data.len() >= 12 {
        let recent: Vec<f64> = oih_data
            .iter()
            .rev()
            .take(6)
            .filter_map(|d| {
                d["sumOpenInterest"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .collect();
        let old: Vec<f64> = oih_data
            .iter()
            .take(6)
            .filter_map(|d| {
                d["sumOpenInterest"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .collect();
        if !recent.is_empty() && !old.is_empty() {
            let r_avg = recent.iter().sum::<f64>() / recent.len() as f64;
            let o_avg = old.iter().sum::<f64>() / old.len() as f64;
            let pct = if o_avg > 0.0 {
                (r_avg - o_avg) / o_avg * 100.0
            } else {
                0.0
            };
            let t = if pct > 5.0 {
                "STRONGLY INCREASING"
            } else if pct > 0.0 {
                "Increasing"
            } else if pct < -5.0 {
                "STRONGLY DECREASING"
            } else if pct < 0.0 {
                "Decreasing"
            } else {
                "Flat"
            };
            (pct, t.to_string())
        } else {
            (0.0, "Insufficient data".to_string())
        }
    } else {
        (0.0, "Insufficient data".to_string())
    };

    // Price trend for divergence
    let klines = binance_get_array(&format!(
        "https://fapi.binance.com/fapi/v1/klines?symbol={}&interval=4h&limit=30",
        sym
    ))
    .await?;
    let price_trend = if klines.len() >= 6 {
        let first = klines[0][4]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let last = klines[klines.len() - 1][4]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        if first > 0.0 {
            (last - first) / first * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let divergence = if oi_24h_change > 5.0 && price_trend > 2.0 {
        "LONG BUILDUP - OI up + Price up = genuine bullish breakout"
    } else if oi_24h_change > 5.0 && price_trend < -2.0 {
        "SHORT BUILDUP - OI up + Price down = aggressive shorting"
    } else if oi_24h_change < -5.0 && price_trend > 2.0 {
        "SHORT SQUEEZE - OI down + Price up = shorts covering"
    } else if oi_24h_change < -5.0 && price_trend < -2.0 {
        "LONG LIQUIDATION - OI down + Price down = longs capitulating"
    } else {
        "NORMAL - No significant divergence"
    };

    Ok(format!(
        "Open Interest - {}\n\n\
         Current OI: {:.0} {} (~${:.0})\n\
         Current Price: ${:.4}\n\
         OI 24h Change: {:+.1}%\n\
         OI Trend: {}\n\
         Price 5d Trend: {:+.1}%\n\n\
         OI/Price Divergence:\n\
         {}",
        sym, oi_qty, base, oi_usd, price, oi_24h_change, oi_trend, price_trend, divergence,
    ))
}

// ══════════════════════════════════════════════════════════════════════
//  TOOL 3: TRADER SENTIMENT (L/S Ratio)
// ══════════════════════════════════════════════════════════════════════

async fn fetch_trader_sentiment(symbol: &str) -> anyhow::Result<String> {
    let sym = symbol.to_uppercase();

    let top_data = binance_get_array(&format!(
        "https://fapi.binance.com/futures/data/topLongShortAccountRatio?symbol={}&period=4h&limit=10",
        sym
    ))
    .await?;

    let global_data = binance_get_array(&format!(
        "https://fapi.binance.com/futures/data/globalLongShortAccountRatio?symbol={}&period=4h&limit=10",
        sym
    ))
    .await?;

    if top_data.is_empty() && global_data.is_empty() {
        return Ok(format!("No sentiment data for {}", sym));
    }

    let mut out = format!("Trader Sentiment - {}\n\n", sym);

    // Top Traders
    if !top_data.is_empty() {
        let latest = &top_data[top_data.len() - 1];
        let long_pct = latest["longAccount"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(50.0)
            * 100.0;
        let short_pct = 100.0 - long_pct;
        let ratio = latest["longShortRatio"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);
        let old_ratio = top_data[0]["longShortRatio"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);

        let signal = if ratio > 2.0 {
            "EXTREME LONG - contrarian beware"
        } else if ratio > 1.5 {
            "LONG BIAS"
        } else if ratio < 0.5 {
            "EXTREME SHORT - contrarian opportunity"
        } else if ratio < 0.7 {
            "SHORT BIAS"
        } else {
            "BALANCED"
        };

        let trend = if ratio > old_ratio * 1.05 {
            "Shifting LONG"
        } else if ratio < old_ratio * 0.95 {
            "Shifting SHORT"
        } else {
            "Stable"
        };

        out.push_str(&format!(
            "Top Traders (Whales):\n\
             Long: {:.1}% | Short: {:.1}%\n\
             L/S Ratio: {:.3}\n\
             Trend: {} ({:.3} -> {:.3})\n\
             Signal: {}\n\n",
            long_pct, short_pct, ratio, trend, old_ratio, ratio, signal,
        ));
    }

    // Global Traders
    if !global_data.is_empty() {
        let latest = &global_data[global_data.len() - 1];
        let long_pct = latest["longAccount"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(50.0)
            * 100.0;
        let short_pct = 100.0 - long_pct;
        let ratio = latest["longShortRatio"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);

        let signal = if long_pct > 65.0 {
            "CROWDED LONG - reversal risk"
        } else if long_pct < 35.0 {
            "CROWDED SHORT - squeeze potential"
        } else {
            "NORMAL DISTRIBUTION"
        };

        out.push_str(&format!(
            "Global Traders (Retail):\n\
             Long: {:.1}% | Short: {:.1}%\n\
             L/S Ratio: {:.3}\n\
             Signal: {}",
            long_pct, short_pct, ratio, signal,
        ));
    }

    Ok(out)
}

// ══════════════════════════════════════════════════════════════════════
//  TOOL 4: TAKER BUY/SELL VOLUME
// ══════════════════════════════════════════════════════════════════════

async fn fetch_taker_volume(symbol: &str) -> anyhow::Result<String> {
    let sym = symbol.to_uppercase();

    let data = binance_get_array(&format!(
        "https://fapi.binance.com/futures/data/takerlongshortRatio?symbol={}&period=4h&limit=20",
        sym
    ))
    .await?;

    if data.is_empty() {
        return Ok(format!("No taker volume data for {}", sym));
    }

    let latest = &data[data.len() - 1];
    let ratio = latest["buySellRatio"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);
    let buy_pct = ratio / (ratio + 1.0) * 100.0;
    let sell_pct = 100.0 - buy_pct;
    let old_ratio = data[0]["buySellRatio"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);

    let signal = if ratio > 1.5 {
        "STRONG BUYING PRESSURE"
    } else if ratio > 1.1 {
        "Buying pressure"
    } else if ratio < 0.7 {
        "STRONG SELLING PRESSURE"
    } else if ratio < 0.9 {
        "Selling pressure"
    } else {
        "NEUTRAL"
    };

    let trend = if ratio > old_ratio * 1.05 {
        "Increasing buying"
    } else if ratio < old_ratio * 0.95 {
        "Increasing selling"
    } else {
        "Stable"
    };

    let mut out = format!(
        "Taker Volume - {}\n\n\
         Taker Buy: {:.1}% | Sell: {:.1}%\n\
         Buy/Sell Ratio: {:.3}\n\
         Trend: {} ({:.3} -> {:.3})\n\
         Signal: {}\n\n\
         Recent Taker Ratios:\n",
        sym, buy_pct, sell_pct, ratio, trend, old_ratio, ratio, signal,
    );

    for d in data.iter().rev().take(8) {
        let r = d["buySellRatio"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let ts = d["timestamp"].as_u64().unwrap_or(0);
        let time_str = chrono::DateTime::from_timestamp_millis(ts as i64)
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let side = if r > 1.0 { "BUY" } else { "SELL" };
        out.push_str(&format!("  [{}] {}  ratio={:.3}\n", side, time_str, r));
    }

    Ok(out)
}

// ══════════════════════════════════════════════════════════════════════
//  TOOL 5: VOLUME PROFILE
// ══════════════════════════════════════════════════════════════════════

async fn fetch_volume_profile(
    symbol: &str,
    interval: &str,
    bucket_count: usize,
) -> anyhow::Result<String> {
    let sym = symbol.to_uppercase();

    let data = binance_get_array(&format!(
        "https://fapi.binance.com/fapi/v1/klines?symbol={}&interval={}&limit=200",
        sym, interval
    ))
    .await?;

    if data.len() < 10 {
        return Ok(format!(
            "Insufficient candle data for {} ({})",
            sym, interval
        ));
    }

    let prices: Vec<f64> = data
        .iter()
        .filter_map(|k| k[4].as_str().and_then(|s| s.parse::<f64>().ok()))
        .collect();
    let volumes: Vec<f64> = data
        .iter()
        .filter_map(|k| k[5].as_str().and_then(|s| s.parse::<f64>().ok()))
        .collect();

    if prices.is_empty() {
        return Ok(format!("No valid price data for {} ({})", sym, interval));
    }

    let p_min = prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let p_max = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let current_price = *prices.last().unwrap_or(&0.0);

    let bucket_size = (p_max - p_min) / bucket_count as f64;
    if bucket_size <= 0.0 {
        return Ok(format!("Price range too small for {} ({})", sym, interval));
    }

    // Build volume histogram
    let mut buckets = vec![0.0f64; bucket_count];
    for (p, v) in prices.iter().zip(volumes.iter()) {
        let idx = ((p - p_min) / bucket_size).floor() as usize;
        let idx = idx.min(bucket_count - 1);
        buckets[idx] += v;
    }

    let total_vol: f64 = buckets.iter().sum();

    // POC: bucket with highest volume
    let poc_idx = buckets
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let poc_price = p_min + (poc_idx as f64 + 0.5) * bucket_size;

    // Value Area: 70% of volume around POC
    let mut sorted_buckets: Vec<(usize, f64)> = buckets.iter().copied().enumerate().collect();
    sorted_buckets.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut va_indices = Vec::new();
    let mut va_vol = 0.0;
    for (idx, vol) in &sorted_buckets {
        va_indices.push(*idx);
        va_vol += vol;
        if va_vol >= total_vol * 0.7 {
            break;
        }
    }
    let va_lo_idx = *va_indices.iter().min().unwrap_or(&0);
    let va_hi_idx = *va_indices.iter().max().unwrap_or(&(bucket_count - 1));
    let va_lo = p_min + va_lo_idx as f64 * bucket_size;
    let va_hi = p_min + (va_hi_idx as f64 + 1.0) * bucket_size;

    let va_position = if current_price > va_hi {
        "ABOVE Value Area"
    } else if current_price < va_lo {
        "BELOW Value Area"
    } else {
        "IN Value Area"
    };

    let poc_signal = if current_price > poc_price {
        "Price above POC - bullish gravity (POC acts as support)"
    } else {
        "Price below POC - bearish gravity (POC acts as resistance)"
    };

    let mut out = format!(
        "Volume Profile - {} ({})\n\n\
         POC (Point of Control): ${:.4}\n\
         Value Area (70%): ${:.4} - ${:.4}\n\
         Current Price: ${:.4} ({})\n\n\
         POC Signal: {}\n\n\
         Price-Volume Histogram ({} periods, {} buckets):\n",
        sym,
        interval,
        poc_price,
        va_lo,
        va_hi,
        current_price,
        va_position,
        poc_signal,
        prices.len(),
        bucket_count,
    );

    // Visual histogram
    let max_bucket_vol = buckets.iter().cloned().fold(0.0f64, f64::max);
    let max_bar_width = 30usize;
    for (i, bucket_vol) in buckets.iter().enumerate().take(bucket_count) {
        let lo = p_min + i as f64 * bucket_size;
        let hi = lo + bucket_size;
        let pct = if total_vol > 0.0 {
            *bucket_vol / total_vol * 100.0
        } else {
            0.0
        };
        let bar_len = if max_bucket_vol > 0.0 {
            ((*bucket_vol / max_bucket_vol) * max_bar_width as f64).round() as usize
        } else {
            0
        };
        let bar: String = "#".repeat(bar_len);
        let marker = if lo <= current_price && current_price < hi {
            " << NOW"
        } else if i == poc_idx {
            " << POC"
        } else {
            ""
        };
        out.push_str(&format!(
            "  ${:.3}-${:.3} |{} {:.1}%{}\n",
            lo, hi, bar, pct, marker,
        ));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derivatives_plugin_metadata() {
        let plugin = DerivativesPlugin::new();
        assert_eq!(plugin.metadata().id, "bonbo-derivatives");
        assert_eq!(plugin.metadata().name, "Derivatives Market Data");
    }

    #[test]
    fn test_derivatives_plugin_tools_count() {
        let plugin = DerivativesPlugin::new();
        let tools = plugin.tools();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"get_funding_rate"));
        assert!(names.contains(&"get_open_interest"));
        assert!(names.contains(&"get_trader_sentiment"));
        assert!(names.contains(&"get_taker_volume"));
        assert!(names.contains(&"get_volume_profile"));
    }

    #[test]
    fn test_derivatives_plugin_default() {
        let plugin = DerivativesPlugin::default();
        assert_eq!(plugin.metadata().id, "bonbo-derivatives");
    }

    #[tokio::test]
    async fn test_execute_unknown_tool() {
        let plugin = DerivativesPlugin::new();
        let args = serde_json::json!({});
        let result = plugin
            .execute_tool(
                "nonexistent_tool",
                &args,
                &PluginContext::new(std::path::PathBuf::from("/tmp/bonbo-test"), "test-plugin"),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_funding_rate_missing_symbol() {
        let plugin = DerivativesPlugin::new();
        let args = serde_json::json!({});
        let result = plugin
            .execute_tool(
                "get_funding_rate",
                &args,
                &PluginContext::new(std::path::PathBuf::from("/tmp/bonbo-test"), "test-plugin"),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_open_interest_missing_symbol() {
        let plugin = DerivativesPlugin::new();
        let args = serde_json::json!({});
        let result = plugin
            .execute_tool(
                "get_open_interest",
                &args,
                &PluginContext::new(std::path::PathBuf::from("/tmp/bonbo-test"), "test-plugin"),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_trader_sentiment_missing_symbol() {
        let plugin = DerivativesPlugin::new();
        let args = serde_json::json!({});
        let result = plugin
            .execute_tool(
                "get_trader_sentiment",
                &args,
                &PluginContext::new(std::path::PathBuf::from("/tmp/bonbo-test"), "test-plugin"),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_taker_volume_missing_symbol() {
        let plugin = DerivativesPlugin::new();
        let args = serde_json::json!({});
        let result = plugin
            .execute_tool(
                "get_taker_volume",
                &args,
                &PluginContext::new(std::path::PathBuf::from("/tmp/bonbo-test"), "test-plugin"),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_volume_profile_missing_symbol() {
        let plugin = DerivativesPlugin::new();
        let args = serde_json::json!({});
        let result = plugin
            .execute_tool(
                "get_volume_profile",
                &args,
                &PluginContext::new(std::path::PathBuf::from("/tmp/bonbo-test"), "test-plugin"),
            )
            .await;
        assert!(result.is_err());
    }
}
