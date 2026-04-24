//! BonBo Data Layer — market data fetching, caching, WebSocket streaming.

pub mod cache;
pub mod fetcher;
pub mod models;
pub mod websocket;

pub use cache::DataCache;
pub use fetcher::{MarketDataFetcher, parse_klines_response};
pub use models::{DataResult, DataTimeFrame, FetchRequest, MarketDataCandle, to_ohlcv};
pub use websocket::{RealtimeKline, RealtimeTick, WebSocketStream};
