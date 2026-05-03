//! Integration tests — cross-crate interactions.

use rust_decimal::Decimal;

#[test]
fn test_ta_indicators_on_market_data() {
    let candles: Vec<bonbo_data::MarketDataCandle> = (0..50)
        .map(|i| bonbo_data::MarketDataCandle {
            symbol: "BTCUSDT".to_string(),
            timeframe: "1h".to_string(),
            timestamp: 1700000000000 + i as i64 * 3600000,
            open: 50000.0 + i as f64 * 10.0,
            high: 50100.0 + i as f64 * 10.0,
            low: 49900.0 + i as f64 * 10.0,
            close: 50050.0 + i as f64 * 10.0,
            volume: 1000.0,
        })
        .collect();

    let ohlcv = bonbo_data::to_ohlcv(&candles);
    assert_eq!(ohlcv.len(), 50);

    let closes: Vec<f64> = ohlcv.iter().map(|c| c.close).collect();
    let analysis = bonbo_ta::batch::compute_full_analysis(&closes);
    // SMA20 is a Vec<Option<f64>> — last value should be Some for 50 data points
    let last_sma = analysis.sma20.last().unwrap_or(&None);
    assert!(
        last_sma.map_or(false, |v| v > 0.0),
        "SMA 20 should be positive"
    );
}

#[test]
fn test_data_pipeline_consistency() {
    let raw = vec![
        bonbo_data::MarketDataCandle {
            symbol: "ETHUSDT".to_string(),
            timeframe: "1h".to_string(),
            timestamp: 1700000000000,
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 103.0,
            volume: 1000.0,
        },
        bonbo_data::MarketDataCandle {
            symbol: "ETHUSDT".to_string(),
            timeframe: "1h".to_string(),
            timestamp: 1700000360000,
            open: 103.0,
            high: 108.0,
            low: 102.0,
            close: 107.0,
            volume: 1200.0,
        },
    ];

    let ohlcv = bonbo_data::to_ohlcv(&raw);
    assert_eq!(ohlcv.len(), 2);
    assert_eq!(ohlcv[0].close, 103.0);
    assert_eq!(ohlcv[1].close, 107.0);

    let tp = ohlcv[0].typical_price();
    assert!((tp - 102.0).abs() < 0.01);
}

#[test]
fn test_risk_position_sizing() {
    let equity = Decimal::new(10000, 0);
    let risk_pct = Decimal::new(1, 0);
    let risk_amount = equity * risk_pct / Decimal::ONE_HUNDRED;
    assert_eq!(risk_amount, Decimal::new(100, 0));

    let entry = Decimal::new(50000, 0);
    let sl = Decimal::new(49800, 0);
    let risk_per_unit = (entry - sl).abs();
    assert_eq!(risk_per_unit, Decimal::new(200, 0));

    let position_size = risk_amount / risk_per_unit;
    assert_eq!(position_size, Decimal::new(5, 1));
}

#[test]
fn test_journal_entry_serde_roundtrip() {
    let snapshot = bonbo_journal::AnalysisSnapshot::default();

    let entry = bonbo_journal::TradeJournalEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: 1700000000000,
        snapshot,
        recommendation: bonbo_journal::Recommendation::Buy,
        entry_price: 50000.0,
        stop_loss: 49800.0,
        target_price: 51000.0,
        risk_reward_ratio: 5.0,
        position_size_usd: 500.0,
        outcome: None,
    };

    let json = serde_json::to_string(&entry).unwrap();
    let decoded: bonbo_journal::TradeJournalEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, entry.id);
    assert_eq!(decoded.entry_price, 50000.0);
    assert_eq!(decoded.snapshot.symbol, "");
}

#[test]
fn test_decimal_consistency_across_crates() {
    let d1 = rust_decimal::Decimal::new(50000, 0);
    let d2 = rust_decimal::Decimal::from_f64_retain(50000.0).unwrap();
    assert_eq!(d1, d2);
}

#[test]
fn test_candle_conversion_preserves_data() {
    let original = bonbo_data::MarketDataCandle {
        symbol: "SOLUSDT".to_string(),
        timeframe: "15m".to_string(),
        timestamp: 1700000000000,
        open: 150.0,
        high: 155.0,
        low: 148.0,
        close: 153.0,
        volume: 5000.0,
    };

    let converted: bonbo_ta::OhlcvCandle = (&original).into();
    assert_eq!(converted.timestamp, original.timestamp);
    assert_eq!(converted.open, original.open);
    assert_eq!(converted.high, original.high);
    assert_eq!(converted.low, original.low);
    assert_eq!(converted.close, original.close);
    assert_eq!(converted.volume, original.volume);
}

#[test]
fn test_ta_regime_detection() {
    let trending_candles: Vec<bonbo_ta::OhlcvCandle> = (0..30)
        .map(|i| bonbo_ta::OhlcvCandle {
            timestamp: 1700000000000 + i as i64 * 3600000,
            open: 100.0 + i as f64 * 2.0,
            high: 102.0 + i as f64 * 2.0,
            low: 99.0 + i as f64 * 2.0,
            close: 101.0 + i as f64 * 2.0,
            volume: 1000.0,
        })
        .collect();

    let regime = bonbo_ta::batch::detect_market_regime(&trending_candles);
    assert!(
        matches!(
            regime,
            bonbo_ta::MarketRegime::TrendingUp
                | bonbo_ta::MarketRegime::TrendingDown
                | bonbo_ta::MarketRegime::Volatile
        ),
        "Expected trending regime for steadily rising prices, got {:?}",
        regime
    );
}
