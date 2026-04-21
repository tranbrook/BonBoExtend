# BonBo Top100 Analyzer — Nâng cấp toàn diện

## 📋 Nhiệm vụ: Nâng cấp analyze_top100.py

### Phân tích hiện trạng (v1)
- File gốc: 253 dòng, sequential, hardcoded targets (18 coins), chỉ dùng stdlib urllib
- Chỉ phân tích 18 coins cố định (không phải top 100 thực sự)
- Parsing text-based rất brittle (regex thủ công)
- Composite score đơn giản (các trọng số cố định)
- Không có async → chậm (4 MCP calls/coin × 18 coins = 72 sequential calls)
- Không có caching → gọi lại data mỗi lần chạy
- Output chỉ là text bảng đơn giản
- Không có fear & greed, không có multi-timeframe
- Không lưu kết quả

### Kế hoạch nâng cấp

- [x] T1: Auto-discover top 100 coins từ Binance (thay hardcoded list)
- [x] T2: Async/parallel MCP calls (aiohttp + asyncio) → tốc độ 5-10x
- [x] T3: Robust parsing (structured dataclasses thay vì brittle regex)
- [x] T4: Multi-timeframe analysis (1h + 4h + 1d, configurable)
- [x] T5: Advanced scoring với DMA weights (Dynamic Model Averaging, 11 factors)
- [x] T6: Fear & Greed Index + Sentiment integration
- [x] T7: Risk-adjusted position sizing cho top picks (ATR-based SL/TP)
- [x] T8: Rich terminal output (bảng màu, panels) + plain text fallback
- [x] T9: JSON/CSV/HTML export
- [x] T10: Backtest integration (1h SMA crossover)
- [x] T11: Smart caching (SQLite, 5-min TTL)
- [x] T12: CLI arguments (--top-n, --intervals, --output, --no-cache, --verbose, --quick)
- [x] T13: Integration với self-learning journal (lưu predictions vào SQLite)
- [x] T14: Testing + syntax validation + bug fixes

## 🐛 Bug Fixes
- [x] Fix cache.get() returning timestamp instead of data (row[1] → row[0])
- [x] Fix MCPClient.call() redundant DB query on cache hit
- [x] Fix CoinAnalysis.position_advice type (PositionData → PositionAdvice)
- [x] Remove unused import (math, Any)
- [x] Fix file truncation at line 785 (position_size_pct cutoff)
