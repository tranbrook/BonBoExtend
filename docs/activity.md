# BonBo Top 100 Full Scan Activity Log

## 2026-04-22 21:55 - Script Created & Tested

### Script: `scripts/top100_full_scan.py`
- **Version**: 4.0
- **Architecture**: 3-Phase
  - Phase 1: Quick scan 100 coins (1h+4h indicators, 6 parallel workers)
  - Phase 2: Deep analysis top 30 (15m+1h+4h+1d + signals + regime + S/R)
  - Phase 3: Multi-strategy backtest top 20 × 13 strategies × 3 timeframes

### 13 Strategies Tested
| Category | Strategy |
|----------|----------|
| Traditional | sma_crossover, ema_crossover, rsi_mean_reversion, bollinger_bands, momentum, breakout, macd_crossover |
| Financial-Hacker | alma_crossover, laguerre_rsi, cmo_momentum, fh_composite, ehlers_trend, enhanced_mean_reversion |

### Test Results (3 coins)
- BTCUSDT: Score 57.6, Best BT: enhanced_mean_reversion (4h) +2.77% WR:100%
- ETHUSDT: Score 55.7, Best BT: enhanced_mean_reversion (4h) +3.31% WR:100%
- SOLUSDT: Score 53.8, Best BT: cmo_momentum (1d) +7.99% WR:100%

## 2026-04-22 21:57 - Full Scan Executed

### Results Summary
- **Sentiment**: 32/100 (Fear)  
- **Coins scanned**: 100 (Phase 1), 30 deep (Phase 2), 20 backtested (Phase 3)
- **Total backtests**: 780 (20 coins × 13 strategies × 3 TFs)

### Top 10 Coins (Composite Score + Confluence)
1. 🟢 AMBUSDT — Score: 68.9 STRONG_BUY — BT: fh_composite (1d) +26.75%
2. 🟢 LDOUSDT — Score: 67.9 STRONG_BUY — BT: cmo_momentum (1h) +8.15%
3. 🟢 ORDIUSDT — Score: 66.6 STRONG_BUY — BT: ema_crossover (1h) +10.74%
4. 🟢 AAVEUSDT — Score: 65.5 BUY — BT: enhanced_mean_reversion (1d) +11.09%
5. 🟢 AXSUSDT — Score: 65.4 STRONG_BUY — BT: enhanced_mean_reversion (1d) +6.79%
6. 🟢 CFXUSDT — Score: 65.2 STRONG_BUY — BT: cmo_momentum (1h) +9.22%
7. 🟢 IMXUSDT — Score: 64.3 STRONG_BUY — BT: laguerre_rsi (1h) +5.75%
8. 🟢 SEIUSDT — Score: 64.0 STRONG_BUY — BT: sma_crossover (1h) +15.61%
9. 🟢 OPUSDT — Score: 63.6 STRONG_BUY — BT: cmo_momentum (1h) +10.87%
10. 🟢 FILUSDT — Score: 63.4 STRONG_BUY — BT: cmo_momentum (1h) +8.45%

### Best Strategy Per Coin
- **FH strategies dominate**: cmo_momentum, fh_composite, enhanced_mean_reversion
- **Best overall**: ema_crossover on SNXUSDT (1d) +32.44%
- **FH best**: ehlers_trend on NEIROUSDT (1h) +27.81%

### Report saved
- `~/.bonbo/reports/full_scan_20260422_220041.json`


## 2026-04-23 23:39 - Session Started
- Project structure files verified
- Resumed work on existing project
- Todo.md updated with new session section
- PROJECT_README.md context checked
- Ready for continued development



## 2026-04-24 12:17 - Session Started
- Project structure files verified
- Resumed work on existing project
- Todo.md updated with new session section
- PROJECT_README.md context checked
- Ready for continued development

