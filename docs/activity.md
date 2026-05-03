# Activity Log — Trading Process Improvement

## 2026-04-27 20:30 — Phân tích & lập kế hoạch

### Đã hoàn thành
- Phân tích toàn bộ `bonbo-ta` crate (3,863 LOC, 15 indicators, 8 modules)
- Phân tích `bonbo-regime` (BOCPD + Hurst R/S + MtfGuard đã implement)
- Phân tích `bonbo-risk` (Kelly, ATR sizing, regime multiplier đã có)
- Phân tích `bonbo-quant` (13 strategies, 4,206 LOC)
- Phân tích `bonbo-learning` (DMA weight adaptation)
- Phân tích `bonbo-agent` (decision loop, state machine, MCP client)
- Đọc `docs/research/trading-process-improvement.md` (387 lines, 10 improvements)

### Key Findings
1. **Quick Wins #6, #7, #8**: Indicators đã sẵn trong `bonbo-ta`, chỉ cần application logic
2. **MTF Look-Ahead Fix #1**: `MtfGuard` đã implement trong `bonbo-regime`, cần integrate
3. **Position Sizing #5**: Kelly + ATR + regime sizing đã có trong `bonbo-risk`
4. **Hurst DFA #2**: Cần implement mới trong `bonbo-ta`
5. **Ensemble #3**: Cần tạo mới trong `bonbo-learning`

### Output
- Tạo `tasks/todo_trading_improvement.md` — kế hoạch 4 phases, 13 tasks
- Kế hoạch được chuẩn hóa theo dependency order


## 2026-04-28 11:38 - Session Started
- Project structure files verified
- Resumed work on existing project
- Todo.md updated with new session section
- PROJECT_README.md context checked
- Ready for continued development

## 2026-04-28 21:00 — v2 Session: Strategy Optimization + Monitoring

### Phase C — Strategy Optimization + Config
- **Task C2**: Created `config/regime_strategies.toml`
  - Regime → Strategy mapping with priority weights
  - 3 regimes: trending, mean_reverting, random_walk
  - Scoring weights for MTF consensus (1h/4h/1d)
  - Signal family weights (RSI, MACD, BB, ALMA, CMO, Laguerre, Hurst)
  - Risk management params (base risk, SL multipliers, TP R:R ratio)
  - Correlation filter + sentiment bonus/penalty

### Phase D — Monitoring & Automation
- **Task D1**: Created `scripts/auto_scan.sh`
  - 4-hour cron scanner: builds + runs `best_trade` example
  - Optional Python alert scanner integration
  - Auto-rotates reports (keeps last 24)
  - Made executable
- **Task D2**: Created `scripts/position_monitor.py`
  - Fetches open positions via MCP client
  - For each position, fetches 4H candles and computes:
    - EMA12/EMA26 trend direction
    - RSI(14) overbought/oversold detection
    - Liquidation proximity check (< 10% = CRITICAL, < 20% = CAUTION)
  - Prints summary table + alerts to console
  - Saves markdown report to `reports/position_monitor_YYYYMMDD_HHMM.md`
- **Task D3**: Updated docs (activity.md, PROJECT_README.md)

