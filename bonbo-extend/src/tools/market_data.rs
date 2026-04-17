//! Market Data Tool Plugin — fetch crypto market data.

use async_trait::async_trait;
use crate::plugin::*;

/// Plugin that provides market data tools.
pub struct MarketDataPlugin {
    metadata: PluginMetadata,
}

impl MarketDataPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-market-data".to_string(),
                name: "Market Data Tools".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Crypto market data: prices, candles, orderbook".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec!["trading".to_string(), "market".to_string(), "crypto".to_string()],
            },
        }
    }
}

impl Default for MarketDataPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolPlugin for MarketDataPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "get_crypto_price".to_string(),
                description: "Get current price of a cryptocurrency from Binance. Returns bid, ask, last price, 24h change.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol (e.g., BTCUSDT, ETHUSDT)".to_string(),
                        required: true,
                        default: Some(serde_json::Value::String("BTCUSDT".to_string())),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_crypto_candles".to_string(),
                description: "Get candlestick/kline data for a cryptocurrency from Binance. Returns OHLCV data for technical analysis.".to_string(),
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
                        description: "Kline interval".to_string(),
                        required: false,
                        default: Some(serde_json::Value::String("1h".to_string())),
                        r#enum: Some(vec![
                            "1m".to_string(), "5m".to_string(), "15m".to_string(),
                            "1h".to_string(), "4h".to_string(), "1d".to_string(),
                        ]),
                    },
                    ParameterSchema {
                        name: "limit".to_string(),
                        param_type: "integer".to_string(),
                        description: "Number of candles to return (max 100)".to_string(),
                        required: false,
                        default: Some(serde_json::Value::Number(24.into())),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "get_crypto_orderbook".to_string(),
                description: "Get order book depth for a cryptocurrency on Binance. Shows bid/ask levels.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".to_string(),
                        param_type: "string".to_string(),
                        description: "Trading pair symbol".to_string(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "limit".to_string(),
                        param_type: "integer".to_string(),
                        description: "Depth levels (5, 10, 20)".to_string(),
                        required: false,
                        default: Some(serde_json::Value::Number(10.into())),
                        r#enum: Some(vec!["5".to_string(), "10".to_string(), "20".to_string()]),
                    },
                ],
            },
            ToolSchema {
                name: "get_top_crypto".to_string(),
                description: "Get top cryptocurrencies by 24h volume from Binance. Returns ranked list with price, change, volume.".to_string(),
                parameters: vec![
                    ParameterSchema {
                        name: "limit".to_string(),
                        param_type: "integer".to_string(),
                        description: "Number of results (max 50)".to_string(),
                        required: false,
                        default: Some(serde_json::Value::Number(10.into())),
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
            "get_crypto_price" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .unwrap_or("BTCUSDT");
                fetch_ticker(symbol).await
            }
            "get_crypto_candles" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let interval = arguments["interval"]
                    .as_str()
                    .unwrap_or("1h");
                let limit = arguments["limit"]
                    .as_u64()
                    .unwrap_or(24)
                    .min(100) as u32;
                fetch_klines(symbol, interval, limit).await
            }
            "get_crypto_orderbook" => {
                let symbol = arguments["symbol"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
                let limit = arguments["limit"]
                    .as_u64()
                    .unwrap_or(10)
                    .min(20) as u32;
                fetch_orderbook(symbol, limit).await
            }
            "get_top_crypto" => {
                let limit = arguments["limit"]
                    .as_u64()
                    .unwrap_or(10)
                    .min(50) as u32;
                fetch_top_volume(limit).await
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }
}

/// Fetch 24hr ticker from Binance.
async fn fetch_ticker(symbol: &str) -> anyhow::Result<String> {
    let url = format!(
        "https://api.binance.com/api/v3/ticker/24hr?symbol={}",
        symbol.to_uppercase()
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Ok(format!("❌ Binance API error: {} — {}", status, body));
    }

    let data: serde_json::Value = resp.json().await?;

    let last_price = data["lastPrice"].as_str().unwrap_or("N/A");
    let change_pct = data["priceChangePercent"].as_str().unwrap_or("N/A");
    let high = data["highPrice"].as_str().unwrap_or("N/A");
    let low = data["lowPrice"].as_str().unwrap_or("N/A");
    let volume = data["volume"].as_str().unwrap_or("N/A");
    let quote_vol = data["quoteVolume"].as_str().unwrap_or("N/A");

    let emoji = if change_pct.parse::<f64>().unwrap_or(0.0) >= 0.0 { "📈" } else { "📉" };

    Ok(format!(
        "{} **{}** Price: ${}\n\
         {} 24h Change: {}%\n\
         📊 High: ${} | Low: ${}\n\
         📦 Volume: {} {} | ${}",
        emoji, symbol, last_price,
        emoji, change_pct,
        high, low,
        volume, symbol.strip_suffix("USDT").unwrap_or(symbol), quote_vol
    ))
}

/// Fetch klines/candles from Binance.
async fn fetch_klines(symbol: &str, interval: &str, limit: u32) -> anyhow::Result<String> {
    let url = format!(
        "https://api.binance.com/api/v3/klines?symbol={}&interval={}&limit={}",
        symbol.to_uppercase(), interval, limit
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        return Ok(format!("❌ Failed to fetch candles for {}", symbol));
    }

    let candles: Vec<serde_json::Value> = resp.json().await?;
    let mut lines = vec![format!("📊 **{} Candles** ({} interval, last {} candles)\n", symbol, interval, limit)];
    lines.push("| Time | Open | High | Low | Close | Volume |".to_string());
    lines.push("|------|------|------|-----|-------|--------|".to_string());

    for candle in candles.iter().take(limit as usize) {
        let empty = vec![];
        let arr = candle.as_array().unwrap_or(&empty);
        if arr.len() < 6 { continue; }
        let open_time = arr[0].as_u64().unwrap_or(0);
        let time = chrono::DateTime::from_timestamp_millis(open_time as i64)
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let open = arr[1].as_str().unwrap_or("?");
        let high = arr[2].as_str().unwrap_or("?");
        let low = arr[3].as_str().unwrap_or("?");
        let close = arr[4].as_str().unwrap_or("?");
        let vol = arr[5].as_str().unwrap_or("?");

        lines.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            time,
            &open[..open.len().min(10)],
            &high[..high.len().min(10)],
            &low[..low.len().min(10)],
            &close[..close.len().min(10)],
            &vol[..vol.len().min(10)],
        ));
    }

    Ok(lines.join("\n"))
}

/// Fetch order book from Binance.
async fn fetch_orderbook(symbol: &str, limit: u32) -> anyhow::Result<String> {
    let url = format!(
        "https://api.binance.com/api/v3/depth?symbol={}&limit={}",
        symbol.to_uppercase(), limit
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        return Ok(format!("❌ Failed to fetch orderbook for {}", symbol));
    }

    let data: serde_json::Value = resp.json().await?;

    let mut lines = vec![format!("📖 **{} Order Book** (depth: {})\n", symbol, limit)];

    // Bids
    lines.push("### Bids (Buy)\n".to_string());
    lines.push("| Price | Quantity |".to_string());
    lines.push("|-------|----------|".to_string());
    if let Some(bids) = data["bids"].as_array() {
        for bid in bids.iter().take(limit as usize) {
            if let (Some(price), Some(qty)) = (bid[0].as_str(), bid[1].as_str()) {
                lines.push(format!("| {} | {} |", price, qty));
            }
        }
    }

    // Asks
    lines.push("\n### Asks (Sell)\n".to_string());
    lines.push("| Price | Quantity |".to_string());
    lines.push("|-------|----------|".to_string());
    if let Some(asks) = data["asks"].as_array() {
        for ask in asks.iter().take(limit as usize) {
            if let (Some(price), Some(qty)) = (ask[0].as_str(), ask[1].as_str()) {
                lines.push(format!("| {} | {} |", price, qty));
            }
        }
    }

    Ok(lines.join("\n"))
}

/// Fetch top crypto by volume from Binance.
async fn fetch_top_volume(limit: u32) -> anyhow::Result<String> {
    let url = "https://api.binance.com/api/v3/ticker/24hr";
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        return Ok("❌ Failed to fetch market data".to_string());
    }

    let tickers: Vec<serde_json::Value> = resp.json().await?;

    // Filter USDT pairs, sort by quote volume
    let mut usdt_pairs: Vec<&serde_json::Value> = tickers
        .iter()
        .filter(|t| {
            t["symbol"].as_str().map(|s| s.ends_with("USDT")).unwrap_or(false)
        })
        .collect();

    usdt_pairs.sort_by(|a, b| {
        let vol_a = a["quoteVolume"].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let vol_b = b["quoteVolume"].as_str().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        vol_b.partial_cmp(&vol_a).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut lines = vec![format!("🏆 **Top {} Crypto by Volume**\n", limit)];
    lines.push("| # | Symbol | Price | 24h % | Volume (USDT) |".to_string());
    lines.push("|---|--------|-------|-------|---------------|".to_string());

    for (i, ticker) in usdt_pairs.iter().take(limit as usize).enumerate() {
        let symbol = ticker["symbol"].as_str().unwrap_or("?");
        let price = ticker["lastPrice"].as_str().unwrap_or("?");
        let change = ticker["priceChangePercent"].as_str().unwrap_or("?");
        let vol = ticker["quoteVolume"].as_str().unwrap_or("?");
        let emoji = if change.parse::<f64>().unwrap_or(0.0) >= 0.0 { "📈" } else { "📉" };

        lines.push(format!(
            "| {} | {} | ${} | {}{} | ${} |",
            i + 1, symbol, price, emoji, change, vol
        ));
    }

    Ok(lines.join("\n"))
}
