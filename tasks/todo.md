# BonBoExtend — Code Graph Recommendations Implementation

## Khuyến nghị từ Code Graph Analysis

### K1: Thêm tests cho FH strategies (advanced_strategies::on_bar — hub #10, 0 tests)
- [x] Test AlmaCrossoverStrategy entry/exit logic
- [x] Test LaguerreRsiStrategy oversold/overbought
- [x] Test CmoMomentumStrategy momentum signals
- [x] Test FhCompositeStrategy weighted scoring + Hurst veto
- [x] Test EhlersTrendStrategy SuperSmoother + Hurst filter
- [x] Test EnhancedMeanReversionStrategy
- [x] Test cross-strategy comparison (no panic, extreme prices)
- [x] All 31 tests passing

### K2: Thêm tests cho compare_strategies (46 callees, 0 tests)
- [x] Test run_backtest with each FH strategy (alma, laguerre, cmo, fh_composite, ehlers, enhanced_mr)
- [x] Test error handling for unknown strategy
- [x] Test compare_strategies with FH strategies
- [x] All 10 backtest tests passing

### K3: Tích hợp fh_analysis.py vào analyze_top100.py (isolated node)
- [x] Đánh giá: fh_analysis.py phục vụ mục đích khác (quick analysis), không nên merge
- [x] Thêm note cross-reference trong fh_analysis.py header

### K4: Refactor models.rs::new (betweenness 0.95, bottleneck toàn hệ thống)
- [x] Phân tích: models.rs chứa simple data structs (MarketDataCandle, FetchRequest)
- [x] Kết luận: Betweenness cao do code graph aggregation, không cần builder pattern
- [x] Thêm PluginMetadata::new() builder helper trong plugin.rs để giảm boilerplate

### K5: Thêm tests cho Python scripts (untested hotspots)
- [x] 14 unit tests cho parsers (indicators, signals, regime, backtest)
- [x] Tests cho FH scoring methods (hurst, laguerre, ALMA, CMO, weighted signals)
- [x] Test score_coin comprehensive
- [x] Test cache roundtrip
- [x] All 14 Python tests passing

### K6: Cleanup boilerplate metadata()/tools() trong services
- [x] Thêm PluginMetadata::new() builder pattern trong plugin.rs
- [x] Giảm boilerplate cho metadata creation
