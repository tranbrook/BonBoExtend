//! WebSocket integration tests — message parsing and stream configuration.

use bonbo_binance_futures::websocket::*;
use bonbo_binance_futures::FuturesConfig;

// ============================================================
// StreamConfig Tests
// ============================================================

#[test]
fn test_stream_config_kline_url() {
    let config = FuturesConfig::mainnet("testkey".to_string(), "testsecret".to_string());
    let stream = market_stream::StreamConfig::kline("BTCUSDT", "1h");
    assert_eq!(stream.symbol, "btcusdt");
    assert_eq!(stream.streams, vec!["btcusdt@kline_1h"]);
    let url = stream.to_url(&config.ws_url);
    assert!(url.contains("btcusdt@kline_1h"));
}

#[test]
fn test_stream_config_mark_price_url() {
    let config = FuturesConfig::mainnet("testkey".to_string(), "testsecret".to_string());
    let stream = market_stream::StreamConfig::mark_price("ETHUSDT");
    let url = stream.to_url(&config.ws_url);
    assert!(url.contains("ethusdt@markPrice@1s"));
}

#[test]
fn test_stream_config_mini_ticker_url() {
    let config = FuturesConfig::mainnet("k".to_string(), "s".to_string());
    let stream = market_stream::StreamConfig::mini_ticker("SOLUSDT");
    let url = stream.to_url(&config.ws_url);
    assert!(url.contains("solusdt@miniTicker"));
}

#[test]
fn test_stream_config_combined_url() {
    let config = FuturesConfig::mainnet("k".to_string(), "s".to_string());
    let stream = market_stream::StreamConfig {
        symbol: "btcusdt".to_string(),
        streams: vec![
            "btcusdt@kline_1h".to_string(),
            "btcusdt@markPrice@1s".to_string(),
        ],
    };
    let url = stream.to_url(&config.ws_url);
    assert!(url.contains("stream?streams="));
    assert!(url.contains("btcusdt@kline_1h/btcusdt@markPrice@1s"));
}

#[test]
fn test_stream_config_single_url_format() {
    let config = FuturesConfig::mainnet("k".to_string(), "s".to_string());
    let stream = market_stream::StreamConfig::kline("BTCUSDT", "15m");
    let url = stream.to_url(&config.ws_url);
    assert!(url.contains("/ws/btcusdt@kline_15m"));
}

// ============================================================
// Message Construction Tests
// ============================================================

#[test]
fn test_ws_message_variants() {
    let kline = WsMessage::Kline(KlineMessage {
        symbol: "BTCUSDT".to_string(),
        interval: "1h".to_string(),
        open_time: 1700000000000,
        close_time: 1700003600000,
        open: "75000.00".to_string(),
        high: "76000.00".to_string(),
        low: "74500.00".to_string(),
        close: "75500.00".to_string(),
        volume: "1234.56".to_string(),
        is_closed: true,
    });
    assert!(matches!(kline, WsMessage::Kline(_)));

    let mark = WsMessage::MarkPrice(MarkPriceMessage {
        symbol: "ETHUSDT".to_string(),
        mark_price: "2300.50".to_string(),
        index_price: "2300.00".to_string(),
        funding_rate: "0.0001".to_string(),
        next_funding_time: 1700000000000,
    });
    assert!(matches!(mark, WsMessage::MarkPrice(_)));

    let raw = WsMessage::Raw("test".to_string());
    assert!(matches!(raw, WsMessage::Raw(_)));
}

#[test]
fn test_kline_message_fields() {
    let msg = KlineMessage {
        symbol: "BTCUSDT".to_string(),
        interval: "4h".to_string(),
        open_time: 1700000000000,
        close_time: 1700014400000,
        open: "75000.00".to_string(),
        high: "76500.00".to_string(),
        low: "74200.00".to_string(),
        close: "75800.00".to_string(),
        volume: "5678.90".to_string(),
        is_closed: false,
    };
    assert_eq!(msg.symbol, "BTCUSDT");
    assert_eq!(msg.interval, "4h");
    assert!(!msg.is_closed);
}

#[test]
fn test_mark_price_message_fields() {
    let msg = MarkPriceMessage {
        symbol: "SOLUSDT".to_string(),
        mark_price: "86.15".to_string(),
        index_price: "86.10".to_string(),
        funding_rate: "0.00034".to_string(),
        next_funding_time: 1700032000000,
    };
    assert_eq!(msg.symbol, "SOLUSDT");
    assert_eq!(msg.funding_rate, "0.00034");
}

// ============================================================
// FuturesConfig WebSocket URLs
// ============================================================

#[test]
fn test_mainnet_ws_url() {
    let config = FuturesConfig::mainnet("k".to_string(), "s".to_string());
    assert_eq!(config.ws_url, "wss://fstream.binance.com");
    assert!(!config.testnet);
}

#[test]
fn test_testnet_ws_url() {
    let config = FuturesConfig::testnet("k".to_string(), "s".to_string());
    assert_eq!(config.ws_url, "wss://stream.binancefuture.com");
    assert!(config.testnet);
}

// ============================================================
// Reconnect Backoff Tests
// ============================================================

#[test]
fn test_rand_jitter_never_exceeds_max() {
    for _ in 0..100 {
        let max_ms = 1000u64;
        let jitter = reconnect::rand_jitter_for_test(max_ms);
        assert!(jitter <= max_ms, "jitter {} exceeded max {}", jitter, max_ms);
    }
}

#[test]
fn test_rand_jitter_with_zero_max() {
    let jitter = reconnect::rand_jitter_for_test(0);
    assert_eq!(jitter, 0);
}

#[test]
fn test_exponential_backoff_calculation() {
    let base = std::time::Duration::from_secs(1);
    let max = std::time::Duration::from_secs(60);

    assert_eq!((base * 2u32.pow(0)).min(max), std::time::Duration::from_secs(1));
    assert_eq!((base * 2u32.pow(1)).min(max), std::time::Duration::from_secs(2));
    assert_eq!((base * 2u32.pow(5)).min(max), std::time::Duration::from_secs(32));
    assert_eq!((base * 2u32.pow(6)).min(max), std::time::Duration::from_secs(60));
}

// ============================================================
// JSON Message Format Tests
// ============================================================

#[test]
fn test_parse_kline_json() {
    let json = r#"{"e":"kline","E":1700000000000,"s":"BTCUSDT","k":{"t":1700000000000,"T":1700003600000,"s":"BTCUSDT","i":"1h","o":"75000.00","c":"75500.00","h":"76000.00","l":"74500.00","v":"1234.56","x":true}}"#;
    let value: serde_json::Value = serde_json::from_str(json).expect("valid json");
    assert_eq!(value["e"], "kline");
    assert_eq!(value["s"], "BTCUSDT");
    assert_eq!(value["k"]["i"], "1h");
    assert_eq!(value["k"]["x"], true);
}

#[test]
fn test_parse_mark_price_json() {
    let json = r#"{"e":"markPriceUpdate","E":1700000000000,"s":"ETHUSDT","p":"2300.50","i":"2300.00","r":"0.0001","T":1700032000000}"#;
    let value: serde_json::Value = serde_json::from_str(json).expect("valid json");
    assert_eq!(value["e"], "markPriceUpdate");
    assert_eq!(value["p"], "2300.50");
}

#[test]
fn test_parse_order_update_json() {
    let json = r#"{"e":"ORDER_TRADE_UPDATE","E":1700000000000,"o":{"s":"ORDIUSDT","c":"bonbo_entry_ordi_001","S":"BUY","o":"LIMIT","f":"GTC","q":"122","p":"4.55","X":"FILLED","i":18928261846}}"#;
    let value: serde_json::Value = serde_json::from_str(json).expect("valid json");
    assert_eq!(value["e"], "ORDER_TRADE_UPDATE");
    assert_eq!(value["o"]["s"], "ORDIUSDT");
    assert_eq!(value["o"]["X"], "FILLED");
}

#[test]
fn test_parse_account_update_json() {
    let json = r#"{"e":"ACCOUNT_UPDATE","E":1700000000000,"a":{"m":"POSITION","B":[{"a":"USDT","wb":"190.36"}],"P":[{"s":"ORDIUSDT","pa":"122","ep":"4.55"}]}}"#;
    let value: serde_json::Value = serde_json::from_str(json).expect("valid json");
    assert_eq!(value["e"], "ACCOUNT_UPDATE");
    assert_eq!(value["a"]["P"][0]["s"], "ORDIUSDT");
}

#[test]
fn test_parse_combined_stream_json() {
    let json = r#"{"stream":"btcusdt@kline_1h","data":{"e":"kline","E":1700000000000,"s":"BTCUSDT","k":{"t":1700000000000,"T":1700003600000,"i":"1h","o":"75000.00","c":"75500.00","h":"76000.00","l":"74500.00","v":"1234.56","x":true}}}"#;
    let value: serde_json::Value = serde_json::from_str(json).expect("valid json");
    assert_eq!(value["stream"], "btcusdt@kline_1h");
    assert_eq!(value["data"]["e"], "kline");
}
