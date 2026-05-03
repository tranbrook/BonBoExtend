//! BonBo Data Layer — market data fetching, caching, WebSocket streaming, funding rate tracking.

pub mod binance_config;
pub mod cache;
pub mod fetcher;
pub mod funding_fetcher;
pub mod funding_tracker;
pub mod models;
pub mod websocket;

pub use binance_config::{BinanceEndpoints, MarketType, market_type};
pub use cache::DataCache;
pub use fetcher::{MarketDataFetcher, parse_klines_response};
pub use funding_fetcher::FundingFetcher;
pub use funding_tracker::{FundingRecord, FundingTracker};
pub use models::{DataResult, DataTimeFrame, FetchRequest, MarketDataCandle, to_ohlcv};
pub use websocket::{RealtimeKline, RealtimeTick, WebSocketStream};
