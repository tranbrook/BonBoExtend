//! Account endpoints — balance, positions, leverage, margin.

use super::FuturesRestClient;
use crate::models::*;

/// Account-related API calls.
pub struct AccountClient;

impl AccountClient {
    /// Get futures account balance.
    pub async fn get_balance(client: &FuturesRestClient) -> anyhow::Result<Vec<FuturesBalance>> {
        let value = client.get_signed("/fapi/v3/balance", "").await?;
        let balances: Vec<FuturesBalance> = serde_json::from_value(value)?;
        Ok(balances)
    }

    /// Get futures account information.
    pub async fn get_account_info(client: &FuturesRestClient) -> anyhow::Result<AccountInfo> {
        let value = client.get_signed("/fapi/v3/account", "").await?;
        let info: AccountInfo = serde_json::from_value(value)?;
        Ok(info)
    }

    /// Get all open positions.
    pub async fn get_positions(client: &FuturesRestClient) -> anyhow::Result<Vec<FuturesPosition>> {
        let value = client.get_signed("/fapi/v3/positionRisk", "").await?;
        let positions: Vec<FuturesPosition> = serde_json::from_value(value)?;
        Ok(positions)
    }

    /// Get position for a specific symbol.
    pub async fn get_position(client: &FuturesRestClient, symbol: &str) -> anyhow::Result<Option<FuturesPosition>> {
        let params = format!("symbol={}", symbol);
        let value = client.get_signed("/fapi/v3/positionRisk", &params).await?;
        let positions: Vec<FuturesPosition> = serde_json::from_value(value)?;
        Ok(positions.into_iter().find(|p| p.is_open()))
    }

    /// Set leverage for a symbol.
    pub async fn set_leverage(client: &FuturesRestClient, symbol: &str, leverage: u32) -> anyhow::Result<Leverage> {
        let params = format!("symbol={}&leverage={}", symbol, leverage);
        let value = client.post_signed("/fapi/v1/leverage", &params).await?;
        let result: Leverage = serde_json::from_value(value)?;
        Ok(result)
    }

    /// Set margin type (CROSSED or ISOLATED).
    pub async fn set_margin_type(
        client: &FuturesRestClient,
        symbol: &str,
        margin_type: &str,
    ) -> anyhow::Result<()> {
        let params = format!("symbol={}&marginType={}", symbol, margin_type);
        match client.post_signed("/fapi/v1/marginType", &params).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // "No need to change margin type" is not an error
                let msg = e.to_string();
                if msg.contains("No need to change") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Get USDT balance (convenience method).
    pub async fn get_usdt_balance(client: &FuturesRestClient) -> anyhow::Result<Decimal> {
        let balances = Self::get_balance(client).await?;
        let usdt = balances
            .into_iter()
            .find(|b| b.asset == "USDT")
            .map(|b| b.available_balance)
            .unwrap_or(Decimal::ZERO);
        Ok(usdt)
    }
}

use rust_decimal::Decimal;
