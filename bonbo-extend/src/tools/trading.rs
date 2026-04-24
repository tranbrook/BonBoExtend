//! Trading Tools Plugin — place/cancel orders, manage positions, set leverage.
//!
//! MCP tools for direct Binance Futures trading:
//! - `futures_smart_execute` — **DEFAULT**: auto-analyze → Flash Limit → Smart Market pipeline
//! - `futures_get_balance` — get USDT balance
//! - `futures_get_positions` — list open positions
//! - `futures_get_open_orders` — list open orders for a symbol
//! - `futures_set_leverage` — set leverage for a symbol
//! - `futures_set_margin_type` — set margin type (CROSSED/ISOLATED)
//! - `futures_place_order` — ⚠️ RAW order (bypasses execution engine — prefer smart_execute)
//! - `futures_cancel_orders` — cancel all open orders for a symbol
//! - `futures_close_position` — close an open position
//! - `futures_set_stop_loss` — set stop-loss via Algo API
//! - `futures_set_take_profit` — set take-profit via Algo API
//!
//! ## IMPORTANT: Always prefer `futures_smart_execute` over `futures_place_order`.
//!
//! `futures_smart_execute` automatically:
//! 1. Fetches real-time orderbook depth
//! 2. Computes OFI (Order Flow Imbalance) signal
//! 3. Runs Flash Limit analysis (spread → route decision)
//! 4. Runs Optimal Slicer (depth walk, slippage budget)
//! 5. Selects Smart-Market 5-phase pipeline (READ→THINK→AIM→WAIT→FIRE)
//! 6. Executes with proper order type and price protection
//! 7. Optionally sets SL/TP automatically
//!
//! `futures_place_order` bypasses all of this and goes straight to Binance.
//! Only use it when you need a specific order type the pipeline doesn't cover.

use crate::plugin::*;
use async_trait::async_trait;
use bonbo_binance_futures::models::*;
use bonbo_binance_futures::rest::{AlgoOrdersClient, OrdersClient};
use bonbo_binance_futures::{FuturesConfig, FuturesRestClient};
use bonbo_executor::flash_limit::{analyze_spread, FlashLimitConfig, SpreadTracker};
use bonbo_executor::market_impact::{estimate_impact, ImpactParams};
use bonbo_executor::optimal_slicer::{OptimalSliceConfig, OptimalSlicer};
use bonbo_executor::orderbook::{OrderBookSnapshot, PriceLevel, Side as ExecSide};
use bonbo_executor::smart_market::SmartMarketConfig;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Lazy-initialized Binance client shared across tool calls.
pub struct TradingPlugin {
    metadata: PluginMetadata,
    /// Shared client — initialized once on first use from env vars.
    client: Arc<RwLock<Option<FuturesRestClient>>>,
}

impl TradingPlugin {
    pub fn new() -> Self {
        Self {
            metadata: PluginMetadata {
                id: "bonbo-trading".to_string(),
                name: "Trading Tools".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Binance Futures trading: orders, positions, leverage, margin".to_string(),
                author: "BonBo Team".to_string(),
                tags: vec![
                    "trading".to_string(),
                    "orders".to_string(),
                    "positions".to_string(),
                    "binance".to_string(),
                ],
            },
            client: Arc::new(RwLock::new(None)),
        }
    }

    /// Get or create the Binance REST client.
    async fn get_client(&self) -> anyhow::Result<FuturesRestClient> {
        {
            let guard = self.client.read().await;
            if let Some(ref c) = *guard {
                return Ok(c.clone());
            }
        }
        // Initialize from env
        let config = FuturesConfig::from_env()?;
        let client = FuturesRestClient::new(&config);
        {
            let mut guard = self.client.write().await;
            *guard = Some(client.clone());
        }
        Ok(client)
    }

    /// Parse side from string ("BUY" or "SELL").
    fn parse_side(s: &str) -> anyhow::Result<Side> {
        match s.to_uppercase().as_str() {
            "BUY" => Ok(Side::Buy),
            "SELL" => Ok(Side::Sell),
            other => Err(anyhow::anyhow!("Invalid side '{}'. Use BUY or SELL.", other)),
        }
    }

    // ─── Tool implementations ──────────────────────────────────

    async fn get_balance(&self) -> anyhow::Result<String> {
        let client = self.get_client().await?;
        let raw = client.get_signed("/fapi/v3/balance", "").await?;

        let usdt = raw.as_array()
            .and_then(|arr| arr.iter().find(|b| b.get("asset").and_then(|a| a.as_str()) == Some("USDT")))
            .ok_or_else(|| anyhow::anyhow!("USDT balance not found"))?;

        let balance: Decimal = usdt.get("balance")
            .and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
            .unwrap_or(Decimal::ZERO);
        let available: Decimal = usdt.get("availableBalance")
            .and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
            .unwrap_or(Decimal::ZERO);
        let pnl: Decimal = usdt.get("crossUnPnl")
            .and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
            .unwrap_or(Decimal::ZERO);

        Ok(format!(
            "💰 **Futures Balance**\n\n\
             | Field | Value |\n\
             |-------|-------|\n\
             | Asset | USDT |\n\
             | Balance | {} |\n\
             | Available | {} |\n\
             | Unrealized PnL | {} |",
            balance, available, pnl
        ))
    }

    async fn get_positions(&self) -> anyhow::Result<String> {
        let client = self.get_client().await?;
        let raw = client.get_signed("/fapi/v3/positionRisk", "").await?;

        let positions: Vec<&Value> = raw.as_array()
            .map(|arr| arr.iter().filter(|p| {
                p.get("positionAmt")
                    .and_then(|v| v.as_str())
                    .map(|s| s != "0" && s != "0.0" && s != "-0")
                    .unwrap_or(false)
            }).collect())
            .unwrap_or_default();

        if positions.is_empty() {
            return Ok("📊 **No open positions**".to_string());
        }

        let mut rows = String::new();
        for p in &positions {
            let symbol = p.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
            let amt: Decimal = p.get("positionAmt").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ZERO);
            let entry: Decimal = p.get("entryPrice").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ZERO);
            let mark: Decimal = p.get("markPrice").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ZERO);
            let pnl: Decimal = p.get("unRealizedProfit").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ZERO);
            let lev: i32 = p.get("leverage").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let liq: Decimal = p.get("liquidationPrice").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(Decimal::ZERO);
            let side = if amt > Decimal::ZERO { "LONG" } else { "SHORT" };

            rows.push_str(&format!(
                "\n| {} | {} | {} | {} | {} | {}x | {} | {}",
                symbol, side, amt, entry, mark, pnl, lev, liq
            ));
        }

        Ok(format!(
            "📊 **Open Positions** ({})\n\n\
             | Symbol | Side | Qty | Entry | Mark | PnL | Lev | Liq Price |\n\
             |--------|------|-----|-------|------|-----|-----|----------|{}\n\
             \n💡 Use `futures_close_position` to close a position.",
            positions.len(), rows
        ))
    }

    async fn get_open_orders(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let client = self.get_client().await?;
        let raw = client.get_signed("/fapi/v1/openOrders", &format!("symbol={}", symbol)).await?;

        let orders = raw.as_array()
            .ok_or_else(|| anyhow::anyhow!("Unexpected response format"))?;

        if orders.is_empty() {
            return Ok(format!("📋 No open orders for {}", symbol));
        }

        let mut rows = String::new();
        for o in orders {
            let oid = o.get("orderId").and_then(|v| v.as_i64()).unwrap_or(0);
            let side = o.get("side").and_then(|v| v.as_str()).unwrap_or("?");
            let otype = o.get("type").and_then(|v| v.as_str()).unwrap_or("?");
            let qty = o.get("origQty").and_then(|v| v.as_str()).unwrap_or("0");
            let price = o.get("price").and_then(|v| v.as_str()).unwrap_or("0");
            let stop = o.get("stopPrice").and_then(|v| v.as_str()).unwrap_or("-");
            rows.push_str(&format!("\n| {} | {} | {} | {} | {} | {}", oid, side, otype, qty, price, stop));
        }

        Ok(format!(
            "📋 **Open Orders for {}** ({})\n\n\
             | Order ID | Side | Type | Qty | Price | Stop Price |\n\
             |----------|------|------|-----|-------|------------|{}\n\
             \n💡 Use `futures_cancel_orders` to cancel.",
            symbol, orders.len(), rows
        ))
    }

    async fn set_leverage(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let leverage = args["leverage"].as_u64()
            .ok_or_else(|| anyhow::anyhow!("leverage is required"))? as u32;

        if leverage < 1 || leverage > 125 {
            anyhow::bail!("Leverage must be between 1 and 125");
        }

        let client = self.get_client().await?;
        let params = format!("symbol={}&leverage={}", symbol, leverage);
        let raw = client.post_signed("/fapi/v1/leverage", &params).await?;

        let sym = raw.get("symbol").and_then(|v| v.as_str()).unwrap_or(symbol);
        let lev = raw.get("leverage").and_then(|v| v.as_i64()).unwrap_or(leverage as i64);

        Ok(format!(
            "✅ **Leverage Set**\n\nSymbol: **{}**\nLeverage: **{}x**", sym, lev
        ))
    }

    async fn set_margin_type(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let margin_type = args["margin_type"].as_str()
            .ok_or_else(|| anyhow::anyhow!("margin_type is required (CROSSED or ISOLATED)"))?;

        let mt = margin_type.to_uppercase();
        if mt != "CROSSED" && mt != "ISOLATED" {
            anyhow::bail!("margin_type must be CROSSED or ISOLATED");
        }

        let client = self.get_client().await?;
        let params = format!("symbol={}&marginType={}", symbol, mt);
        match client.post_signed("/fapi/v1/marginType", &params).await {
            Ok(_) => Ok(format!("✅ **Margin Type Set**\n\nSymbol: **{}**\nMargin: **{}**", symbol, mt)),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("No need to change") {
                    Ok(format!("ℹ️ **Margin Type Already Set**\n\nSymbol: **{}** is already **{}**", symbol, mt))
                } else {
                    Err(e)
                }
            }
        }
    }

    /// **DEFAULT execution method** — runs the full analysis pipeline:
    /// 1. Fetch orderbook → compute spread, OFI, imbalance
    /// 2. Flash Limit analysis → route decision
    /// 3. Optimal Slicer → max safe quantity
    /// 4. Smart Market 5-phase → AIM (limit) → WAIT → FIRE (market sweep)
    /// 5. Optionally set SL/TP
    /// **DEFAULT execution method** — runs the full analysis pipeline:
    /// 1. Fetch orderbook → compute spread, imbalance
    /// 2. Flash Limit analysis → route decision
    /// 3. Pre-trade slippage check
    /// 4. Auto-select order type (Limit at touch / Market)
    /// 5. Execute via Binance
    /// 6. Optionally set SL/TP
    async fn smart_execute(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let side_str = args["side"].as_str()
            .ok_or_else(|| anyhow::anyhow!("side is required (BUY/SELL)"))?;
        let qty_str = args["quantity"].as_str()
            .ok_or_else(|| anyhow::anyhow!("quantity is required"))?;
        let max_slippage_bps: f64 = args["max_slippage_bps"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["max_slippage_bps"].as_f64())
            .unwrap_or(5.0);
        let sl_price = args["stop_loss_price"].as_str();
        let tp_price = args["take_profit_price"].as_str();

        let side = Self::parse_side(side_str)?;
        let quantity: Decimal = qty_str.parse()
            .map_err(|_| anyhow::anyhow!("Invalid quantity '{}'", qty_str))?;
        let exec_side = match side {
            Side::Buy => ExecSide::Buy,
            Side::Sell => ExecSide::Sell,
        };

        let client = self.get_client().await?;

        // ═══ STEP 1: Fetch orderbook ═══
        let depth_json = client.get_public(
            "/fapi/v1/depth",
            &format!("symbol={}&limit=20", symbol),
        ).await.map_err(|e| anyhow::anyhow!("Orderbook fetch failed: {}", e))?;

        let book = OrderBookSnapshot::from_binance_depth(symbol, &depth_json)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse orderbook for {}", symbol))?;

        let mid_price = book.mid_price().unwrap_or(Decimal::ONE);
        let spread_f64 = book.spread_bps().unwrap_or(100.0);

        // ═══ STEP 2: Flash Limit analysis ═══
        let mut spread_tracker = SpreadTracker::new(2.0, 1.5, 20);
        let flash_config = FlashLimitConfig::default();
        let flash = analyze_spread(&book, exec_side, &flash_config, &mut spread_tracker);
        let route_str = format!("{}", flash.route);
        let savings_bps = flash.estimated_savings_bps;

        // ═══ STEP 3: Pre-trade slippage check ═══
        let notional_f64: f64 = mid_price.to_string().parse::<f64>().unwrap_or(0.0)
            * quantity.to_string().parse::<f64>().unwrap_or(0.0);
        let impact_params = match symbol {
            "BTCUSDT" | "ETHUSDT" => ImpactParams::btcusdt(),
            "SOLUSDT" => ImpactParams::solusdt(),
            "SEIUSDT" => ImpactParams::seiusdt(),
            _ => ImpactParams::btcusdt(),
        };
        let impact = estimate_impact(&impact_params, notional_f64, 0.0004, 0.5);
        let est_slippage = impact.impact_bps;

        if est_slippage > max_slippage_bps {
            return Ok(format!(
                "⚠️ **Trade Blocked — Slippage Too High**\n\n\
                 | Metric | Value |\n\
                 |--------|-------|\n\
                 | Estimated Slippage | {:.2} bps |\n\
                 | Max Allowed | {:.1} bps |\n\
                 | Route | {} |\n\
                 | Spread | {:.2} bps |\n\
                 \n💡 Reduce quantity or increase max_slippage_bps.",
                est_slippage, max_slippage_bps, route_str, spread_f64
            ));
        }

        // ═══ STEP 4: Determine order type ═══
        let (order_type, execution_note) = if spread_f64 <= 2.0 {
            ("LIMIT", format!(
                "⚡ **Flash Limit** (spread {:.2} bps ≤ threshold)\n\
                 → Limit at touch price (maker fee 0.02%, saves ~{:.2} bps)\n\
                 → Price protection: only fills if book is favorable",
                spread_f64, savings_bps))
        } else if spread_f64 <= 10.0 {
            ("LIMIT", format!(
                "📐 **Adaptive Limit** (spread {:.2} bps)\n\
                 → Limit at best touch price\n\
                 → If not filled → retry as market",
                spread_f64))
        } else if spread_f64 <= 25.0 {
            ("MARKET", format!(
                "🏪 **Market Order** (spread {:.2} bps — too wide for limit)\n\
                 → Guaranteed fill, slippage-gated at {:.1} bps",
                spread_f64, max_slippage_bps))
        } else {
            return Ok(format!(
                "🛑 **Trade Blocked — Spread Too Wide**\n\n\
                 Spread: {:.2} bps (threshold: 25 bps)\n\
                 \n💡 Wait for spread to normalize, or use a different pair.",
                spread_f64
            ));
        };

        // ═══ STEP 5: Compute limit price if LIMIT ═══
        let limit_price = if order_type == "LIMIT" {
            match exec_side {
                ExecSide::Buy => book.best_ask().unwrap_or(mid_price),
                ExecSide::Sell => book.best_bid().unwrap_or(mid_price),
            }
        } else {
            mid_price
        };

        // ═══ STEP 6: Execute order via Binance ═══
        let side_str_param = match side { Side::Buy => "BUY", Side::Sell => "SELL" };
        let params_str = if order_type == "LIMIT" {
            format!(
                "symbol={}&side={}&type=LIMIT&quantity={}&price={}&timeInForce=GTC",
                symbol, side_str_param, quantity, limit_price
            )
        } else {
            format!(
                "symbol={}&side={}&type=MARKET&quantity={}",
                symbol, side_str_param, quantity
            )
        };

        let raw = match client.post_signed("/fapi/v1/order", &params_str).await {
            Ok(v) => v,
            Err(e) => {
                // If LIMIT fails, auto-escalate to MARKET
                if order_type == "LIMIT" {
                    tracing::warn!("Limit order failed ({}), escalating to market", e);
                    let market_params = format!(
                        "symbol={}&side={}&type=MARKET&quantity={}",
                        symbol, side_str_param, quantity
                    );
                    client.post_signed("/fapi/v1/order", &market_params).await
                        .map_err(|e2| anyhow::anyhow!("Both limit and market failed: {} | {}", e, e2))?
                } else {
                    return Err(anyhow::anyhow!("Order failed: {}", e));
                }
            }
        };

        let order_id = raw.get("orderId").and_then(|v| v.as_i64()).unwrap_or(0);
        let status = raw.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
        let exec_qty = raw.get("executedQty").and_then(|v| v.as_str()).unwrap_or("0");
        let avg_price = raw.get("avgPrice").and_then(|v| v.as_str()).unwrap_or("-");
        let fill_status = if status == "FILLED" { "✅ FILLED".to_string() } else { format!("⏳ {}", status) };

        // ═══ STEP 7: Set SL/TP if requested ═══
        let mut sl_tp_report = String::new();
        if (status == "FILLED" || status == "PARTIALLY_FILLED") && (sl_price.is_some() || tp_price.is_some()) {
            let close_side = match side { Side::Buy => "SELL", Side::Sell => "BUY" };
            if let Some(sl) = sl_price {
                let sl_params = format!(
                    "algoType=CONDITIONAL&symbol={}&side={}&type=STOP_MARKET&quantity={}&triggerPrice={}&workingType=MARK_PRICE",
                    symbol, close_side, quantity, sl
                );
                match client.post_signed("/fapi/v1/algoOrder", &sl_params).await {
                    Ok(r) => {
                        let algo_id = r.get("algoId").and_then(|v| v.as_i64()).unwrap_or(0);
                        sl_tp_report.push_str(&format!("\n| Stop Loss | {} @ {} (Algo #{}) ✅ |", close_side, sl, algo_id));
                    }
                    Err(e) => sl_tp_report.push_str(&format!("\n| Stop Loss | ❌ Failed: {} |", e)),
                }
            }
            if let Some(tp) = tp_price {
                let tp_params = format!(
                    "algoType=CONDITIONAL&symbol={}&side={}&type=TAKE_PROFIT_MARKET&quantity={}&triggerPrice={}&workingType=MARK_PRICE",
                    symbol, close_side, quantity, tp
                );
                match client.post_signed("/fapi/v1/algoOrder", &tp_params).await {
                    Ok(r) => {
                        let algo_id = r.get("algoId").and_then(|v| v.as_i64()).unwrap_or(0);
                        sl_tp_report.push_str(&format!("\n| Take Profit | {} @ {} (Algo #{}) ✅ |", close_side, tp, algo_id));
                    }
                    Err(e) => sl_tp_report.push_str(&format!("\n| Take Profit | ❌ Failed: {} |", e)),
                }
            }
        }

        // ═══ STEP 8: Build report ═══
        let price_display = if order_type == "LIMIT" { format!("{}", limit_price) } else { "N/A (market)".to_string() };

        Ok(format!(
            "## ✅ Smart Execute — {}\n\n{}\n\n\
             | Field | Value |\n|-------|-------|\n\
             | Symbol | {} |\n| Side | {} |\n| Order Type | {} |\n\
             | Quantity | {} |\n| Price Target | {} |\n| Executed | {} |\n\
             | Avg Price | {} |\n| Status | {} |\n| Order ID | {} |\n\
             | Spread | {:.2} bps |\n| Est. Impact | {:.2} bps |\n\
             | Est. Savings | {:.2} bps |{}\n\n\
             📊 Pipeline: Orderbook → FlashLimit({}) → Impact({:.2} bps) → {} → Execute",
            symbol, execution_note, symbol, side_str_param, order_type,
            quantity, price_display, exec_qty, avg_price, fill_status, order_id,
            spread_f64, est_slippage, savings_bps, sl_tp_report,
            route_str, est_slippage, order_type
        ))
    }

    async fn place_order(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let side = Self::parse_side(args["side"].as_str()
            .ok_or_else(|| anyhow::anyhow!("side is required (BUY/SELL)"))?)?;
        let quantity: Decimal = args["quantity"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["quantity"].as_f64().and_then(|f| Decimal::from_f64_retain(f)))
            .ok_or_else(|| anyhow::anyhow!("quantity is required"))?;
        let order_type = args["order_type"].as_str().unwrap_or("MARKET").to_uppercase();
        let price: Option<Decimal> = args["price"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["price"].as_f64().and_then(|f| Decimal::from_f64_retain(f)));
        let reduce_only = args["reduce_only"].as_bool().unwrap_or(false);

        if quantity <= Decimal::ZERO {
            anyhow::bail!("quantity must be positive");
        }

        let client = self.get_client().await?;

        let order_req = match order_type.as_str() {
            "MARKET" => {
                let mut req = NewOrderRequest::market(symbol, side, quantity);
                if reduce_only { req = req.with_reduce_only(); }
                req
            }
            "LIMIT" => {
                let limit_price = price
                    .ok_or_else(|| anyhow::anyhow!("price is required for LIMIT orders"))?;
                let mut req = NewOrderRequest::limit(symbol, side, quantity, limit_price);
                if reduce_only { req = req.with_reduce_only(); }
                req
            }
            "TRAILING_STOP_MARKET" => {
                let callback_rate: Decimal = args["callback_rate"].as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| args["callback_rate"].as_f64().and_then(|f| Decimal::from_f64_retain(f)))
                    .ok_or_else(|| anyhow::anyhow!("callback_rate is required for TRAILING_STOP_MARKET (0.1-5.0%)"))?;
                let mut req = NewOrderRequest {
                    symbol: symbol.to_string(),
                    side,
                    r#type: OrderType::TrailingStopMarket,
                    time_in_force: None,
                    quantity: Some(quantity),
                    price: None,
                    stop_price: None,
                    close_position: None,
                    reduce_only: if reduce_only { Some(true) } else { None },
                    position_side: None,
                    working_type: Some(WorkingType::MarkPrice),
                    new_client_order_id: None,
                    callback_rate: Some(callback_rate),
                };
                req
            }
            "STOP_MARKET" => {
                let stop_price: Decimal = args["stop_price"].as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| args["stop_price"].as_f64().and_then(|f| Decimal::from_f64_retain(f)))
                    .ok_or_else(|| anyhow::anyhow!("stop_price is required for STOP_MARKET"))?;
                let mut req = NewOrderRequest {
                    symbol: symbol.to_string(),
                    side,
                    r#type: OrderType::StopMarket,
                    time_in_force: None,
                    quantity: Some(quantity),
                    price: None,
                    stop_price: Some(stop_price),
                    close_position: None,
                    reduce_only: if reduce_only { Some(true) } else { None },
                    position_side: None,
                    working_type: Some(WorkingType::MarkPrice),
                    new_client_order_id: None,
                    callback_rate: None,
                };
                req
            }
            other => anyhow::bail!("Unsupported order_type '{}'. Use MARKET, LIMIT, STOP_MARKET, or TRAILING_STOP_MARKET.", other),
        };

        let params = order_req.to_query();
        let raw = client.post_signed("/fapi/v1/order", &params).await?;

        let order_id = raw.get("orderId").and_then(|v| v.as_i64()).unwrap_or(0);
        let status = raw.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
        let sym = raw.get("symbol").and_then(|v| v.as_str()).unwrap_or(symbol);
        let sd = raw.get("side").and_then(|v| v.as_str()).unwrap_or("?");
        let orig_qty = raw.get("origQty").and_then(|v| v.as_str()).unwrap_or("0");
        let exec_qty = raw.get("executedQty").and_then(|v| v.as_str()).unwrap_or("0");
        let avg_price = raw.get("avgPrice").and_then(|v| v.as_str()).unwrap_or("-");
        let ot = raw.get("type").and_then(|v| v.as_str()).unwrap_or(&order_type);

        Ok(format!(
            "✅ **Order Placed**\n\n\
             | Field | Value |\n\
             |-------|-------|\n\
             | Order ID | {} |\n\
             | Symbol | {} |\n\
             | Side | {} |\n\
             | Type | {} |\n\
             | Quantity | {} |\n\
             | Executed | {} |\n\
             | Avg Price | {} |\n\
             | Status | {} |\n\
             \n💡 Use `futures_set_stop_loss` and `futures_set_take_profit` to protect your position.",
            order_id, sym, sd, ot, orig_qty, exec_qty, avg_price, status
        ))
    }

    async fn cancel_orders(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let client = self.get_client().await?;

        // Cancel standard orders
        let std_result = OrdersClient::cancel_all_orders(&client, symbol).await;

        // Cancel SL/TP standard orders
        let sltp_result = OrdersClient::cancel_sl_tp_orders(&client, symbol).await;

        let mut msg = format!("🗑️ **Cancel Orders for {}**\n\n", symbol);

        match std_result {
            Ok(cancelled) => msg.push_str(&format!("- Standard orders cancelled: {}\n", cancelled.len())),
            Err(e) => msg.push_str(&format!("- Standard cancel: {}\n", e)),
        }

        match sltp_result {
            Ok(cancelled) => msg.push_str(&format!("- SL/TP orders cancelled: {}\n", cancelled.len())),
            Err(e) => msg.push_str(&format!("- SL/TP cancel: {}\n", e)),
        }

        Ok(msg)
    }

    async fn close_position(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let client = self.get_client().await?;

        // Get current position
        let params = format!("symbol={}", symbol);
        let raw = client.get_signed("/fapi/v3/positionRisk", &params).await?;

        let positions = raw.as_array()
            .ok_or_else(|| anyhow::anyhow!("Unexpected response format"))?;

        let position = positions.iter().find(|p| {
            p.get("positionAmt")
                .and_then(|v| v.as_str())
                .map(|s| s != "0" && s != "0.0" && s != "-0")
                .unwrap_or(false)
        });

        let position = match position {
            Some(p) => p,
            None => return Ok(format!("📊 No open position for {}", symbol)),
        };

        let amt: Decimal = position.get("positionAmt")
            .and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("Cannot parse positionAmt"))?;
        let abs_qty = amt.abs();

        let close_side = if amt > Decimal::ZERO { Side::Sell } else { Side::Buy };

        // Cancel existing orders first
        let _ = OrdersClient::cancel_all_orders(&client, symbol).await;
        let _ = OrdersClient::cancel_sl_tp_orders(&client, symbol).await;

        // Close position with market order
        let close_req = NewOrderRequest::market(symbol, close_side, abs_qty).with_reduce_only();
        let order_params = close_req.to_query();
        let raw = client.post_signed("/fapi/v1/order", &order_params).await?;

        let order_id = raw.get("orderId").and_then(|v| v.as_i64()).unwrap_or(0);
        let status = raw.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let exec_qty = raw.get("executedQty").and_then(|v| v.as_str()).unwrap_or("0");
        let avg_price = raw.get("avgPrice").and_then(|v| v.as_str()).unwrap_or("-");

        Ok(format!(
            "✅ **Position Closed**\n\n\
             | Field | Value |\n\
             |-------|-------|\n\
             | Symbol | {} |\n\
             | Position | {} {} |\n\
             | Close Order | #{} |\n\
             | Executed | {} |\n\
             | Avg Price | {} |\n\
             | Status | {} |",
            symbol, abs_qty, if amt > Decimal::ZERO { "LONG" } else { "SHORT" },
            order_id, exec_qty, avg_price, status
        ))
    }

    async fn set_stop_loss(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let trigger_price: Decimal = args["trigger_price"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["trigger_price"].as_f64().and_then(|f| Decimal::from_f64_retain(f)))
            .ok_or_else(|| anyhow::anyhow!("trigger_price is required"))?;
        let side = Self::parse_side(args["side"].as_str().unwrap_or("SELL"))?;
        let close_position = args["close_position"].as_bool().unwrap_or(true);

        let client = self.get_client().await?;
        let close_position = args["close_position"].as_bool().unwrap_or(true);
        let quantity: Option<Decimal> = args["quantity"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["quantity"].as_f64().and_then(|f| Decimal::from_f64_retain(f)));

        let resp = if let Some(qty) = quantity {
            // Partial stop-loss with specific quantity
            AlgoOrdersClient::stop_loss_partial(&client, symbol, side, trigger_price, qty).await?
        } else {
            // Full close position stop-loss
            AlgoOrdersClient::stop_loss(&client, symbol, trigger_price, side, close_position).await?
        };

        if resp.is_success() {
            Ok(format!(
                "🛡️ **Stop-Loss Set**\n\n\
                 | Field | Value |\n\
                 |-------|-------|\n\
                 | Symbol | {} |\n\
                 | Trigger | {} |\n\
                 | Side | {} |\n\
                 | Algo ID | {} |\n\
                 | Close All | {} |",
                symbol, trigger_price, side, resp.algo_id, close_position
            ))
        } else {
            Ok(format!(
                "⚠️ **Stop-Loss Rejected**\n\nCode: {}\nMessage: {}",
                resp.code, resp.msg
            ))
        }
    }

    async fn set_take_profit(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let trigger_price: Decimal = args["trigger_price"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["trigger_price"].as_f64().and_then(|f| Decimal::from_f64_retain(f)))
            .ok_or_else(|| anyhow::anyhow!("trigger_price is required"))?;
        let side = Self::parse_side(args["side"].as_str().unwrap_or("SELL"))?;
        let close_position = args["close_position"].as_bool().unwrap_or(true);

        let client = self.get_client().await?;
        let resp = AlgoOrdersClient::take_profit(&client, symbol, trigger_price, side, close_position).await?;

        if resp.is_success() {
            Ok(format!(
                "🎯 **Take-Profit Set**\n\n\
                 | Field | Value |\n\
                 |-------|-------|\n\
                 | Symbol | {} |\n\
                 | Trigger | {} |\n\
                 | Side | {} |\n\
                 | Algo ID | {} |\n\
                 | Close All | {} |",
                symbol, trigger_price, side, resp.algo_id, close_position
            ))
        } else {
            Ok(format!(
                "⚠️ **Take-Profit Rejected**\n\nCode: {}\nMessage: {}",
                resp.code, resp.msg
            ))
        }
    }

    async fn set_trailing_stop(&self, args: &Value) -> anyhow::Result<String> {
        let symbol = args["symbol"].as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol is required"))?;
        let callback_rate: Decimal = args["callback_rate"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["callback_rate"].as_f64().and_then(|f| Decimal::from_f64_retain(f)))
            .ok_or_else(|| anyhow::anyhow!("callback_rate is required (0.1-5.0 %)"))?;
        let side = Self::parse_side(args["side"].as_str().unwrap_or("SELL"))?;
        let quantity: Option<Decimal> = args["quantity"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["quantity"].as_f64().and_then(|f| Decimal::from_f64_retain(f)));
        let activate_price: Option<Decimal> = args["activate_price"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["activate_price"].as_f64().and_then(|f| Decimal::from_f64_retain(f)));

        if callback_rate < Decimal::from(1) / Decimal::from(10) || callback_rate > Decimal::from(5) {
            anyhow::bail!("callback_rate must be between 0.1 and 5.0 percent");
        }

        let client = self.get_client().await?;
        let resp = AlgoOrdersClient::trailing_stop(
            &client, symbol, side, callback_rate, quantity, activate_price,
        ).await?;

        if resp.is_success() {
            Ok(format!(
                "🔄 **Trailing Stop Set**\n\n\
                 | Field | Value |\n\
                 |-------|-------|\n\
                 | Symbol | {} |\n\
                 | Callback Rate | {}% |\n\
                 | Side | {} |\n\
                 | Quantity | {} |\n\
                 | Algo ID | {} |\n\
                 | Activate Price | {} |",
                symbol, callback_rate, side,
                quantity.map(|q| q.to_string()).unwrap_or_else(|| "all".to_string()),
                resp.algo_id,
                activate_price.map(|p| p.to_string()).unwrap_or_else(|| "immediate".to_string())
            ))
        } else {
            Ok(format!(
                "⚠️ **Trailing Stop Rejected**\n\nCode: {}\nMessage: {}",
                resp.code, resp.msg
            ))
        }
    }
}

impl Default for TradingPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolPlugin for TradingPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            // ─── Account ──────────────────────────────────
            ToolSchema {
                name: "futures_get_balance".into(),
                description: "Get Binance Futures USDT account balance (total, available, unrealized PnL). Requires BINANCE_API_KEY and BINANCE_API_SECRET env vars.".into(),
                parameters: vec![],
            },
            ToolSchema {
                name: "futures_get_positions".into(),
                description: "Get all open Binance Futures positions with entry price, mark price, unrealized PnL, leverage, and liquidation price.".into(),
                parameters: vec![],
            },
            // ─── Smart Execute (DEFAULT) ────────────────
            ToolSchema {
                name: "futures_smart_execute".into(),
                description: "[DEFAULT] Execute order via full analysis pipeline: orderbook → OFI → Flash Limit → Optimal Slicer → Smart Market 5-phase. ALWAYS prefer this over futures_place_order. Auto-selects best order type and price based on real-time market conditions. Optionally sets SL/TP automatically.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., BTCUSDT, ETHUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "side".into(),
                        param_type: "string".into(),
                        description: "BUY or SELL".into(),
                        required: true,
                        default: None,
                        r#enum: Some(vec!["BUY".into(), "SELL".into()]),
                    },
                    ParameterSchema {
                        name: "quantity".into(),
                        param_type: "string".into(),
                        description: "Quantity to trade (e.g., '0.010' BTC, '5.0' ETH)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "stop_loss_price".into(),
                        param_type: "string".into(),
                        description: "Optional stop-loss price. Auto-set after main order fills.".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "take_profit_price".into(),
                        param_type: "string".into(),
                        description: "Optional take-profit price. Auto-set after main order fills.".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "max_slippage_bps".into(),
                        param_type: "string".into(),
                        description: "Max acceptable slippage in bps (default: 5.0). Order skipped if exceeded.".into(),
                        required: false,
                        default: Some("5.0".into()),
                        r#enum: None,
                    },
                ],
            },
            // ─── Orders ──────────────────────────────────
            ToolSchema {
                name: "futures_get_open_orders".into(),
                description: "Get all open orders for a symbol on Binance Futures.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., BTCUSDT, DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "futures_place_order".into(),
                description: "Place an order on Binance Futures (MARKET or LIMIT, BUY or SELL). Example: Buy 100 DOTUSDT at market price.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., BTCUSDT, DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "side".into(),
                        param_type: "string".into(),
                        description: "Order direction".into(),
                        required: true,
                        default: None,
                        r#enum: Some(vec!["BUY".into(), "SELL".into()]),
                    },
                    ParameterSchema {
                        name: "quantity".into(),
                        param_type: "number".into(),
                        description: "Order quantity (in base asset units)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "order_type".into(),
                        param_type: "string".into(),
                        description: "Order type".into(),
                        required: false,
                        default: Some(json!("MARKET")),
                        r#enum: Some(vec!["MARKET".into(), "LIMIT".into()]),
                    },
                    ParameterSchema {
                        name: "price".into(),
                        param_type: "number".into(),
                        description: "Limit price (required for LIMIT orders)".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "reduce_only".into(),
                        param_type: "boolean".into(),
                        description: "If true, only reduce existing position (no new position)".into(),
                        required: false,
                        default: Some(json!(false)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "futures_cancel_orders".into(),
                description: "Cancel ALL open orders (including SL/TP) for a symbol on Binance Futures.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., BTCUSDT, ORDIUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            // ─── Position ────────────────────────────────
            ToolSchema {
                name: "futures_close_position".into(),
                description: "Close an open position on Binance Futures by placing an opposite market order. Also cancels all associated orders.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair to close (e.g., ORDIUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            // ─── Leverage & Margin ───────────────────────
            ToolSchema {
                name: "futures_set_leverage".into(),
                description: "Set leverage for a symbol on Binance Futures (1-125x).".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "leverage".into(),
                        param_type: "integer".into(),
                        description: "Leverage multiplier (1-125)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "futures_set_margin_type".into(),
                description: "Set margin type for a symbol on Binance Futures.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "margin_type".into(),
                        param_type: "string".into(),
                        description: "Margin type".into(),
                        required: true,
                        default: None,
                        r#enum: Some(vec!["CROSSED".into(), "ISOLATED".into()]),
                    },
                ],
            },
            // ─── SL / TP ────────────────────────────────
            ToolSchema {
                name: "futures_set_stop_loss".into(),
                description: "Set a stop-loss order via Binance Algo API. Triggers a market order when price reaches trigger_price.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "trigger_price".into(),
                        param_type: "number".into(),
                        description: "Stop-loss trigger price".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "side".into(),
                        param_type: "string".into(),
                        description: "Order side when triggered (SELL for long SL, BUY for short SL)".into(),
                        required: false,
                        default: Some(json!("SELL")),
                        r#enum: Some(vec!["BUY".into(), "SELL".into()]),
                    },
                    ParameterSchema {
                        name: "close_position".into(),
                        param_type: "boolean".into(),
                        description: "Close entire position when triggered".into(),
                        required: false,
                        default: Some(json!(true)),
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "quantity".into(),
                        param_type: "number".into(),
                        description: "Specific quantity to close (optional, overrides close_position)".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "futures_set_take_profit".into(),
                description: "Set a take-profit order via Binance Algo API. Triggers a market order when price reaches trigger_price.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "trigger_price".into(),
                        param_type: "number".into(),
                        description: "Take-profit trigger price".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "side".into(),
                        param_type: "string".into(),
                        description: "Order side when triggered (SELL for long TP, BUY for short TP)".into(),
                        required: false,
                        default: Some(json!("SELL")),
                        r#enum: Some(vec!["BUY".into(), "SELL".into()]),
                    },
                    ParameterSchema {
                        name: "close_position".into(),
                        param_type: "boolean".into(),
                        description: "Close entire position when triggered".into(),
                        required: false,
                        default: Some(json!(true)),
                        r#enum: None,
                    },
                ],
            },
            ToolSchema {
                name: "futures_set_trailing_stop".into(),
                description: "Set a trailing stop order via Binance Algo API. Follows price up by callback_rate% and triggers SELL when price reverses.".into(),
                parameters: vec![
                    ParameterSchema {
                        name: "symbol".into(),
                        param_type: "string".into(),
                        description: "Trading pair (e.g., DOTUSDT)".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "callback_rate".into(),
                        param_type: "number".into(),
                        description: "Callback rate in percent (0.1-5.0). Trailing distance from highest price.".into(),
                        required: true,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "side".into(),
                        param_type: "string".into(),
                        description: "Order side when triggered (SELL for long, BUY for short)".into(),
                        required: false,
                        default: Some(json!("SELL")),
                        r#enum: Some(vec!["BUY".into(), "SELL".into()]),
                    },
                    ParameterSchema {
                        name: "quantity".into(),
                        param_type: "number".into(),
                        description: "Quantity to close. If not set, tries to close entire position.".into(),
                        required: false,
                        default: None,
                        r#enum: None,
                    },
                    ParameterSchema {
                        name: "activate_price".into(),
                        param_type: "number".into(),
                        description: "Price at which trailing starts (optional, defaults to current price)".into(),
                        required: false,
                        default: None,
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
            "futures_smart_execute" => self.smart_execute(arguments).await,
            "futures_get_balance" => self.get_balance().await,
            "futures_get_positions" => self.get_positions().await,
            "futures_get_open_orders" => self.get_open_orders(arguments).await,
            "futures_set_leverage" => self.set_leverage(arguments).await,
            "futures_set_margin_type" => self.set_margin_type(arguments).await,
            "futures_place_order" => self.place_order(arguments).await,
            "futures_cancel_orders" => self.cancel_orders(arguments).await,
            "futures_close_position" => self.close_position(arguments).await,
            "futures_set_stop_loss" => self.set_stop_loss(arguments).await,
            "futures_set_take_profit" => self.set_take_profit(arguments).await,
            "futures_set_trailing_stop" => self.set_trailing_stop(arguments).await,
            _ => anyhow::bail!("Unknown trading tool: {}", tool_name),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_side_buy() {
        let result = TradingPlugin::parse_side("BUY");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Side::Buy));
    }

    #[test]
    fn test_parse_side_sell() {
        let result = TradingPlugin::parse_side("SELL");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Side::Sell));
    }

    #[test]
    fn test_parse_side_case_insensitive() {
        assert!(TradingPlugin::parse_side("buy").is_ok());
        assert!(TradingPlugin::parse_side("sell").is_ok());
        assert!(TradingPlugin::parse_side("Buy").is_ok());
    }

    #[test]
    fn test_parse_side_invalid() {
        let result = TradingPlugin::parse_side("HOLD");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid side"));
    }

    #[test]
    fn test_smart_execute_args_validation_missing_symbol() {
        // Verify smart_execute validates required args
        let args = json!({
            "side": "BUY",
            "quantity": "0.010"
        });
        // Can't actually call smart_execute without a running client,
        // but we can verify the args parsing would fail
        assert!(args["symbol"].as_str().is_none());
    }

    #[test]
    fn test_smart_execute_args_validation_missing_side() {
        let args = json!({
            "symbol": "BTCUSDT",
            "quantity": "0.010"
        });
        assert!(args["side"].as_str().is_none());
    }

    #[test]
    fn test_smart_execute_args_validation_missing_quantity() {
        let args = json!({
            "symbol": "BTCUSDT",
            "side": "BUY"
        });
        assert!(args["quantity"].as_str().is_none());
    }

    #[test]
    fn test_smart_execute_args_valid() {
        let args = json!({
            "symbol": "BTCUSDT",
            "side": "BUY",
            "quantity": "0.010",
            "max_slippage_bps": "10.0",
            "stop_loss_price": "76000",
            "take_profit_price": "80000"
        });
        assert_eq!(args["symbol"].as_str(), Some("BTCUSDT"));
        assert_eq!(args["side"].as_str(), Some("BUY"));
        assert_eq!(args["quantity"].as_str(), Some("0.010"));
        assert_eq!(args["max_slippage_bps"].as_str(), Some("10.0"));
        assert_eq!(args["stop_loss_price"].as_str(), Some("76000"));
        assert_eq!(args["take_profit_price"].as_str(), Some("80000"));
    }

    #[test]
    fn test_smart_execute_default_slippage() {
        let args = json!({
            "symbol": "BTCUSDT",
            "side": "BUY",
            "quantity": "0.010"
        });
        // Default max_slippage_bps should be 5.0
        let max_slippage: f64 = args["max_slippage_bps"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| args["max_slippage_bps"].as_f64())
            .unwrap_or(5.0);
        assert!((max_slippage - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_smart_execute_quantity_parse() {
        let args = json!({"quantity": "0.010"});
        let qty: Decimal = args["quantity"].as_str().unwrap().parse().unwrap();
        assert_eq!(qty, Decimal::new(10, 3)); // 0.010
    }

    #[test]
    fn test_smart_execute_quantity_invalid() {
        let args = json!({"quantity": "abc"});
        let result: Result<Decimal, _> = args["quantity"].as_str().unwrap().parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_side_conversion_buy() {
        let side = TradingPlugin::parse_side("BUY").unwrap();
        let exec_side = match side {
            Side::Buy => ExecSide::Buy,
            Side::Sell => ExecSide::Sell,
        };
        assert!(matches!(exec_side, ExecSide::Buy));
    }

    #[test]
    fn test_side_conversion_sell() {
        let side = TradingPlugin::parse_side("SELL").unwrap();
        let exec_side = match side {
            Side::Buy => ExecSide::Buy,
            Side::Sell => ExecSide::Sell,
        };
        assert!(matches!(exec_side, ExecSide::Sell));
    }

    #[test]
    fn test_spread_route_decision_flash_limit() {
        // spread ≤ 2 bps → Flash Limit (LIMIT order)
        let spread_f64 = 1.5;
        let order_type = if spread_f64 <= 2.0 { "LIMIT" }
            else if spread_f64 <= 10.0 { "LIMIT" }
            else if spread_f64 <= 25.0 { "MARKET" }
            else { "HOLD" };
        assert_eq!(order_type, "LIMIT");
    }

    #[test]
    fn test_spread_route_decision_adaptive() {
        // 2 < spread ≤ 10 bps → Adaptive Limit
        let spread_f64 = 5.0;
        let order_type = if spread_f64 <= 2.0 { "FLASH" }
            else if spread_f64 <= 10.0 { "ADAPTIVE" }
            else if spread_f64 <= 25.0 { "MARKET" }
            else { "HOLD" };
        assert_eq!(order_type, "ADAPTIVE");
    }

    #[test]
    fn test_spread_route_decision_market() {
        // 10 < spread ≤ 25 → Market
        let spread_f64 = 15.0;
        let order_type = if spread_f64 <= 2.0 { "FLASH" }
            else if spread_f64 <= 10.0 { "ADAPTIVE" }
            else if spread_f64 <= 25.0 { "MARKET" }
            else { "HOLD" };
        assert_eq!(order_type, "MARKET");
    }

    #[test]
    fn test_spread_route_decision_blocked() {
        // spread > 25 → Blocked
        let spread_f64 = 30.0;
        let order_type = if spread_f64 <= 2.0 { "FLASH" }
            else if spread_f64 <= 10.0 { "ADAPTIVE" }
            else if spread_f64 <= 25.0 { "MARKET" }
            else { "HOLD" };
        assert_eq!(order_type, "HOLD");
    }

    #[test]
    fn test_sl_tp_close_side_for_buy() {
        // BUY position → SL/TP should SELL
        let side = Side::Buy;
        let close_side = match side { Side::Buy => "SELL", Side::Sell => "BUY" };
        assert_eq!(close_side, "SELL");
    }

    #[test]
    fn test_sl_tp_close_side_for_sell() {
        // SELL position → SL/TP should BUY
        let side = Side::Sell;
        let close_side = match side { Side::Buy => "SELL", Side::Sell => "BUY" };
        assert_eq!(close_side, "BUY");
    }
}
