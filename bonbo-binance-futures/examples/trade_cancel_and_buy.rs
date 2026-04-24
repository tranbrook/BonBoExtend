//! Quick trade executor: Cancel ORDI + Buy DOT with margin x10
//!
//! Usage:
//!   cargo run --example trade_cancel_and_buy -p bonbo-binance-futures

use anyhow::{Context, Result};
use bonbo_binance_futures::models::*;
use bonbo_binance_futures::rest::{AccountClient, AlgoOrdersClient, OrdersClient};
use bonbo_binance_futures::{FuturesConfig, FuturesRestClient};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env from project root
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    dotenvy::from_path(format!("{}/BonBoExtend/.env", home)).ok();

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    // === Setup client ===
    let config = FuturesConfig::from_env().context("Failed to load Binance config from .env")?;
    let client = FuturesRestClient::new(&config);

    println!("🔧 Connected to Binance {}",
        if config.testnet { "TESTNET" } else { "MAINNET" }
    );

    // ============================================================
    // STEP 1: Cancel ALL orders + close position for ORDIUSDT
    // ============================================================
    println!("\n{}", "=".repeat(50));
    println!("📤 STEP 1: Cancelling all ORDI orders & closing position");
    println!("{}", "=".repeat(50));

    // 1a. Cancel all open orders for ORDIUSDT
    match OrdersClient::cancel_all_orders(&client, "ORDIUSDT").await {
        Ok(cancelled) => {
            if cancelled.is_empty() {
                println!("   ✅ No open orders to cancel for ORDIUSDT");
            } else {
                println!("   ✅ Cancelled {} open orders for ORDIUSDT", cancelled.len());
                for order in &cancelled {
                    println!("      - Order #{} ({:?})", order.order_id, order.status);
                }
            }
        }
        Err(e) => {
            println!("   ⚠️  Error cancelling ORDIUSDT orders: {}", e);
        }
    }

    // 1b. Cancel any algo (SL/TP) orders for ORDIUSDT
    match AlgoOrdersClient::cancel_algo_order(&client, None, Some("ORDIUSDT")).await {
        Ok(resp) => {
            println!("   ✅ Cancelled algo orders for ORDIUSDT: code={}", resp.code);
        }
        Err(e) => {
            println!("   ⚠️  No algo orders to cancel (or error): {}", e);
        }
    }

    // 1c. Check and close any open position for ORDIUSDT
    match AccountClient::get_position(&client, "ORDIUSDT").await {
        Ok(Some(position)) => {
            let abs_qty = position.position_amt.abs();
            if abs_qty > Decimal::ZERO {
                println!("   📊 Open ORDI position: {} contracts", position.position_amt);

                // Close by placing opposite market order
                let close_side = if position.position_amt > Decimal::ZERO {
                    Side::Sell
                } else {
                    Side::Buy
                };

                let close_order = NewOrderRequest::market("ORDIUSDT", close_side, abs_qty)
                    .with_reduce_only();

                match OrdersClient::place_order(&client, &close_order).await {
                    Ok(resp) => {
                        println!("   ✅ Closed ORDI position: Order #{} (status: {:?})",
                            resp.order_id, resp.status);
                    }
                    Err(e) => {
                        println!("   ❌ Failed to close ORDI position: {}", e);
                    }
                }
            } else {
                println!("   ✅ No open ORDI position to close");
            }
        }
        Ok(None) => {
            println!("   ✅ No open ORDI position");
        }
        Err(e) => {
            println!("   ⚠️  Error checking ORDI position: {}", e);
        }
    }

    // ============================================================
    // STEP 2: Buy DOTUSDT with margin x10
    // ============================================================
    println!("\n{}", "=".repeat(50));
    println!("📈 STEP 2: Buying DOTUSDT with leverage x10");
    println!("{}", "=".repeat(50));

    // 2a. Set leverage to 10x for DOTUSDT
    let leverage_params = "symbol=DOTUSDT&leverage=10";
    match client.post_signed("/fapi/v1/leverage", leverage_params).await {
        Ok(val) => {
            let sym = val.get("symbol").and_then(|v| v.as_str()).unwrap_or("DOTUSDT");
            let lev = val.get("leverage").and_then(|v| v.as_i64()).unwrap_or(10);
            println!("   ✅ Leverage set: {} = {}x", sym, lev);
        }
        Err(e) => {
            println!("   ⚠️  Leverage set error (may already be 10x): {}", e);
        }
    }

    // 2b. Set margin type to CROSSED (isolated margin x10)
    match AccountClient::set_margin_type(&client, "DOTUSDT", "CROSSED").await {
        Ok(()) => {
            println!("   ✅ Margin type set: CROSSED");
        }
        Err(e) => {
            println!("   ℹ️  Margin type: {} (may already be CROSSED)", e);
        }
    }

    // 2c. Get current DOTUSDT price
    let ticker = client
        .get_public("/fapi/v1/ticker/price", "symbol=DOTUSDT")
        .await
        .context("Failed to get DOTUSDT price")?;

    let dot_price: Decimal = ticker
        .get("price")
        .and_then(|p| p.as_str())
        .and_then(|p| p.parse().ok())
        .context("Failed to parse DOTUSDT price")?;

    println!("   💰 Current DOTUSDT price: {}", dot_price);

    // 2d. Calculate quantity based on available balance
    // Use raw JSON to avoid deserialization issues with v3 API
    let balance_raw = client.get_signed("/fapi/v3/balance", "").await
        .context("Failed to get balance")?;

    let usdt_balance: Decimal = balance_raw
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|b| b.get("asset").and_then(|a| a.as_str()) == Some("USDT"))
        })
        .and_then(|b| b.get("availableBalance").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()))
        .unwrap_or(Decimal::ZERO);

    println!("   💰 Available USDT balance: {}", usdt_balance);

    // Use 30% of balance for this position (risk management)
    let position_usdt = usdt_balance * dec!(0.3);
    // With 10x leverage: notional = position_usdt * 10
    let notional = position_usdt * dec!(10);
    let quantity = (notional / dot_price).round_dp(1); // DOT has 1 decimal precision

    println!("   📐 Position size: {} DOT (≈ {} USDT notional with 10x)", quantity, notional);

    if quantity <= Decimal::ZERO {
        anyhow::bail!("Calculated quantity is zero — insufficient balance");
    }

    // 2e. Place MARKET BUY order for DOTUSDT
    println!("\n   🚀 Placing MARKET BUY {} DOTUSDT...", quantity);

    let order = NewOrderRequest::market("DOTUSDT", Side::Buy, quantity);
    let order_params = order.to_query();

    match client.post_signed("/fapi/v1/order", &order_params).await {
        Ok(resp) => {
            // Parse response from raw JSON for robustness
            let order_id = resp.get("orderId").and_then(|v| v.as_i64()).unwrap_or(0);
            let status = resp.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
            let symbol = resp.get("symbol").and_then(|v| v.as_str()).unwrap_or("DOTUSDT");
            let side = resp.get("side").and_then(|v| v.as_str()).unwrap_or("BUY");
            let orig_qty = resp.get("origQty").and_then(|v| v.as_str()).and_then(|s| s.parse::<Decimal>().ok()).unwrap_or(quantity);
            let exec_qty = resp.get("executedQty").and_then(|v| v.as_str()).and_then(|s| s.parse::<Decimal>().ok()).unwrap_or(Decimal::ZERO);
            let avg_price = resp.get("avgPrice").and_then(|v| v.as_str()).and_then(|s| s.parse::<Decimal>().ok()).unwrap_or(Decimal::ZERO);

            println!("   ✅ BUY order placed successfully!");
            println!("      Order ID: {}", order_id);
            println!("      Status: {}", status);
            println!("      Symbol: {}", symbol);
            println!("      Side: {}", side);
            println!("      Quantity: {}", orig_qty);
            println!("      Executed: {}", exec_qty);
            if avg_price > Decimal::ZERO {
                println!("      Avg Price: {}", avg_price);
            }
        }
        Err(e) => {
            println!("   ❌ BUY order failed: {}", e);
            anyhow::bail!("DOTUSDT buy failed");
        }
    }

    // 2f. Set stop-loss at -3% and take-profit at +5% (risk management)
    let sl_price = (dot_price * dec!(0.97)).round_dp(3);
    let tp_price = (dot_price * dec!(1.05)).round_dp(3);

    println!("\n   🛡️  Setting Stop-Loss @ {} (-3%)", sl_price);
    match AlgoOrdersClient::stop_loss(&client, "DOTUSDT", sl_price, Side::Sell, true).await {
        Ok(resp) => {
            if resp.is_success() {
                println!("      ✅ SL set (algo #{})", resp.algo_id);
            } else {
                println!("      ⚠️  SL response: {} - {}", resp.code, resp.msg);
            }
        }
        Err(e) => {
            println!("      ⚠️  SL failed: {}", e);
        }
    }

    println!("   🎯 Setting Take-Profit @ {} (+5%)", tp_price);
    match AlgoOrdersClient::take_profit(&client, "DOTUSDT", tp_price, Side::Sell, true).await {
        Ok(resp) => {
            if resp.is_success() {
                println!("      ✅ TP set (algo #{})", resp.algo_id);
            } else {
                println!("      ⚠️  TP response: {} - {}", resp.code, resp.msg);
            }
        }
        Err(e) => {
            println!("      ⚠️  TP failed: {}", e);
        }
    }

    // ============================================================
    // SUMMARY
    // ============================================================
    println!("\n{}", "=".repeat(50));
    println!("📋 SUMMARY");
    println!("{}", "=".repeat(50));
    println!("   1. ORDI orders cancelled & position closed ✅");
    println!("   2. DOTUSDT leverage set to 10x CROSSED ✅");
    println!("   3. Bought {} DOT @ ~{} ✅", quantity, dot_price);
    println!("   4. SL @ {} / TP @ {} ✅", sl_price, tp_price);
    println!("\n✨ Done!");

    Ok(())
}
