# BonBo Top 100 Full Scan + Multi-Strategy Backtest

## Overview
Script phân tích toàn diện top 100 coin multi-timeframe + backtest tất cả 13 chiến dịch.

## Script
`scripts/top100_full_scan.py` — 1400 lines, Python 3

## Architecture
- **Phase 1**: Quick scan 100 coins (1h+4h indicators, 6 parallel MCP workers) → rank → top 30
- **Phase 2**: Deep analysis top 30 (15m+1h+4h+1d indicators + signals + regime + S/R)
- **Phase 3**: Multi-strategy backtest top 20 × 13 strategies × 3 timeframes (780 tests)

## 13 Strategies
| Category | Strategies |
|----------|-----------|
| Traditional (7) | sma_crossover, ema_crossover, rsi_mean_reversion, bollinger_bands, momentum, breakout, macd_crossover |
| Financial-Hacker (6) | alma_crossover, laguerre_rsi, cmo_momentum, fh_composite, ehlers_trend, enhanced_mean_reversion |

## Usage
```bash
python3 scripts/top100_full_scan.py                   # Full run (~3 min)
python3 scripts/top100_full_scan.py --quick            # 20 coins, fast
python3 scripts/top100_full_scan.py --coins BTCUSDT    # Specific coins
python3 scripts/top100_full_scan.py --no-backtest      # Skip Phase 3
python3 scripts/top100_full_scan.py --workers 8        # More parallelism
```

## Output
- Terminal: color-coded tables, recommendations with Entry/SL/TP
- JSON: `~/.bonbo/reports/full_scan_TIMESTAMP.json`

## Dependencies
- `target/release/bonbo-extend-mcp` (must be built)
- Python 3.7+ (stdlib only)


---

## Session Update - 2026-04-23 23:39
- **Session Started**: 2026-04-23 23:39
- **Context Status**: Verified and up-to-date

*Context automatically updated for new development session*


---

## Session Update - 2026-04-24 12:17
- **Session Started**: 2026-04-24 12:17
- **Context Status**: Verified and up-to-date

*Context automatically updated for new development session*
