//! Crypto-specific debate context with on-chain metrics

use serde::{Deserialize, Serialize};

/// Extended debate context with crypto-specific data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoDebateContext {
    /// Base ticker
    pub ticker: String,
    /// BTC dominance at decision time
    pub btc_dominance: f64,
    /// 24h funding rate
    pub funding_rate: f64,
    /// Open interest change 24h (%)
    pub oi_change_24h: f64,
    /// Long/Short ratio
    pub long_short_ratio: f64,
    /// Taker buy/sell volume ratio
    pub taker_buy_sell_ratio: f64,
    /// Top trader long/short ratio
    pub top_trader_ls_ratio: f64,
    /// 24h liquidation volume (USD)
    pub liquidation_24h_usd: f64,
    /// Market cap rank
    pub mcap_rank: Option<u32>,
    /// DeFi TVL (if DeFi token)
    pub defi_tvl: Option<f64>,
    /// On-chain active addresses 24h
    pub active_addresses_24h: Option<u64>,
    /// Whale transactions >100k last 24h
    pub whale_tx_count_24h: Option<u64>,
    /// Network volume change vs 7d avg (%)
    pub network_vol_change_pct: Option<f64>,
    /// Exchange netflow (positive = inflow to exchanges = bearish)
    pub exchange_netflow: Option<f64>,
    /// Market summary text
    pub market_summary: String,
    /// Fear & Greed index (0-100)
    pub fear_greed_index: u8,
}

impl CryptoDebateContext {
    /// Quick bullish/bearish assessment from derivatives data
    pub fn derivatives_bias(&self) -> DerivativesBias {
        let mut bullish_signals = 0u32;
        let mut bearish_signals = 0u32;

        // Funding rate: negative = bullish (shorts paying longs)
        if self.funding_rate < -0.01 {
            bullish_signals += 2; // Strong signal
        } else if self.funding_rate < 0.0 {
            bullish_signals += 1;
        } else if self.funding_rate > 0.05 {
            bearish_signals += 2; // Overleveraged longs
        } else if self.funding_rate > 0.01 {
            bearish_signals += 1;
        }

        // OI change: rising OI + rising price = bullish
        if self.oi_change_24h > 5.0 {
            bullish_signals += 1;
        } else if self.oi_change_24h < -5.0 {
            bearish_signals += 1;
        }

        // L/S ratio: crowded long = contrarian bearish
        if self.long_short_ratio > 2.0 {
            bearish_signals += 2; // Too many longs
        } else if self.long_short_ratio < 0.5 {
            bullish_signals += 2; // Too many shorts
        }

        // Taker B/S ratio
        if self.taker_buy_sell_ratio > 1.2 {
            bullish_signals += 1;
        } else if self.taker_buy_sell_ratio < 0.8 {
            bearish_signals += 1;
        }

        // Exchange netflow: positive = bearish (selling pressure)
        if let Some(netflow) = self.exchange_netflow {
            if netflow > 0.0 {
                bearish_signals += 1;
            } else {
                bullish_signals += 1;
            }
        }

        let total = (bullish_signals + bearish_signals).max(1) as f64;
        let bull_pct = bullish_signals as f64 / total;

        if bull_pct > 0.65 {
            DerivativesBias::Bullish { confidence: bull_pct }
        } else if bull_pct < 0.35 {
            DerivativesBias::Bearish { confidence: 1.0 - bull_pct }
        } else {
            DerivativesBias::Neutral
        }
    }

    /// On-chain health assessment
    pub fn onchain_health(&self) -> OnChainHealth {
        let mut score = 50u8; // Neutral baseline

        // Active addresses growing = healthy
        if let Some(vol_change) = self.network_vol_change_pct {
            if vol_change > 20.0 {
                score = score.saturating_add(15);
            } else if vol_change > 5.0 {
                score = score.saturating_add(5);
            } else if vol_change < -20.0 {
                score = score.saturating_sub(15);
            } else if vol_change < -5.0 {
                score = score.saturating_sub(5);
            }
        }

        // Exchange outflow = accumulation (bullish)
        if let Some(netflow) = self.exchange_netflow {
            if netflow < 0.0 {
                score = score.saturating_add(10);
            } else {
                score = score.saturating_sub(10);
            }
        }

        // Whale activity
        if let Some(whale_count) = self.whale_tx_count_24h {
            if whale_count > 100 {
                score = score.saturating_add(10);
            }
        }

        // DeFi TVL growing = bullish for DeFi tokens
        if let Some(tvl) = self.defi_tvl {
            if tvl > 1_000_000_000.0 {
                score = score.saturating_add(5);
            }
        }

        OnChainHealth { score, assessment: if score >= 70 { "Healthy" } else if score >= 50 { "Neutral" } else { "Weak" }.to_string() }
    }
}

#[derive(Debug, Clone)]
pub enum DerivativesBias {
    Bullish { confidence: f64 },
    Bearish { confidence: f64 },
    Neutral,
}

#[derive(Debug, Clone)]
pub struct OnChainHealth {
    pub score: u8,
    pub assessment: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bullish_derivatives() {
        let ctx = CryptoDebateContext {
            ticker: "BTCUSDT".to_string(),
            btc_dominance: 52.0,
            funding_rate: -0.02, // Negative = shorts paying
            oi_change_24h: 8.0,
            long_short_ratio: 0.4, // More shorts
            taker_buy_sell_ratio: 1.5,
            top_trader_ls_ratio: 1.2,
            liquidation_24h_usd: 500_000_000.0,
            mcap_rank: Some(1),
            defi_tvl: None,
            active_addresses_24h: Some(1_200_000),
            whale_tx_count_24h: Some(250),
            network_vol_change_pct: Some(15.0),
            exchange_netflow: Some(-5000.0),
            market_summary: "BTC trending up".to_string(),
            fear_greed_index: 45,
        };

        let bias = ctx.derivatives_bias();
        assert!(matches!(bias, DerivativesBias::Bullish { .. }));
    }

    #[test]
    fn test_onchain_health() {
        let ctx = CryptoDebateContext {
            ticker: "ETHUSDT".to_string(),
            btc_dominance: 52.0,
            funding_rate: 0.01,
            oi_change_24h: 2.0,
            long_short_ratio: 1.1,
            taker_buy_sell_ratio: 1.0,
            top_trader_ls_ratio: 1.0,
            liquidation_24h_usd: 100_000_000.0,
            mcap_rank: Some(2),
            defi_tvl: Some(50_000_000_000.0),
            active_addresses_24h: Some(800_000),
            whale_tx_count_24h: Some(150),
            network_vol_change_pct: Some(25.0),
            exchange_netflow: Some(-3000.0),
            market_summary: "ETH DeFi growing".to_string(),
            fear_greed_index: 55,
        };

        let health = ctx.onchain_health();
        assert!(health.score >= 65); // Should be healthy
    }
}
