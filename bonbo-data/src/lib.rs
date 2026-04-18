//! BonBo Data Layer — market data fetching, caching, WebSocket streaming.

pub mod cache;
pub mod fetcher;
pub mod models;
pub mod websocket;

pub use cache::DataCache;
pub use fetcher::{MarketDataFetcher, parse_klines_response};
pub use models::{
    DataResult, DataTimeFrame, FetchRequest, MarketDataCandle,
};
pub use websocket::{WebSocketStream, RealtimeTick, RealtimeKline};
