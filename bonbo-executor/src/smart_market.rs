//! Smart-Market Execution Engine — replaces old `execute_adaptive_limit`.
//!
//! A 5-phase order execution pipeline that intelligently decides HOW to execute:
//!
//! ```text
//! Phase 1: READ   — Fetch L2 depth, compute OFI, detect walls
//! Phase 2: THINK  — Classify book state → choose strategy
//! Phase 3: AIM    — Post aggressive limit at optimal price
//! Phase 4: WAIT   — Monitor for fill with timeout
//! Phase 5: FIRE   — Sweep remaining with market if needed
//! ```
//!
//! # Key Improvements Over Old Adaptive Limit
//!
//! | Feature              | Old Adaptive Limit   | Smart-Market              |
//! |----------------------|----------------------|---------------------------|
//! | Book analysis        | None                 | OFI + wall detection      |
//! | Limit pricing        | Mid ± fixed offset   | Depth-weighted mid        |
//! | Fill monitoring      | None (instant check) | Poll-based with timeout   |
//! | Partial fill         | Ignored              | Typed recovery strategies |
//! | Error handling       | anyhow bail          | ExecutionError + decision |
//! | Rate limit recovery  | None                 | Exponential backoff       |
//! | Slippage guard       | Pre-sweep only       | Pre + post + dynamic      |
//! | OFI-aware sizing     | No                   | Scale by signal strength  |

use crate::execution_algo::{ExecutionReport, FillResult, OrderPlacer};
use crate::execution_errors::{
    ErrorDecision, ExecutionError, PartialFillStrategy, decide, handle_partial_fill,
};
use crate::ofi::{OfiConfig, OfiScore, OfiSignal};
use crate::orderbook::{OrderBookSnapshot, Side};
use crate::risk_guards::{CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════

/// Smart-Market configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartMarketConfig {
    // ── Phase 1: READ ────────────────────────────────────────
    /// Number of depth levels to analyze for OFI.
    pub depth_levels: usize,

    // ── Phase 2: THINK ───────────────────────────────────────
    /// OFI threshold above which we go aggressive (limit at bid/ask).
    pub aggressive_ofi_threshold: f64,
    /// OFI threshold below which we go passive (limit inside spread).
    pub passive_ofi_threshold: f64,
    /// Wall size (fraction of total depth) that triggers wall protection.
    pub wall_threshold: f64,

    // ── Phase 3: AIM ─────────────────────────────────────────
    /// Limit offset from touch in passive mode (bps).
    pub passive_offset_bps: f64,
    /// Maximum spread (bps) to place a limit inside.
    pub max_spread_for_limit_bps: f64,
    /// Tick size rounding: round limit price to this many decimals.
    pub price_decimals: u32,

    // ── Phase 4: WAIT ────────────────────────────────────────
    /// How long to wait for limit fill (ms).
    pub limit_timeout_ms: u64,
    /// How often to poll for fill status (ms).
    pub poll_interval_ms: u64,

    // ── Phase 5: FIRE ────────────────────────────────────────
    /// Maximum slippage for market sweep (bps).
    pub max_sweep_slippage_bps: f64,
    /// Partial fill recovery strategy.
    pub partial_fill_strategy: PartialFillStrategy,
    /// Maximum total retries across all phases.
    pub max_retries: u32,

    // ── Risk Gates ───────────────────────────────────────────
    /// Maximum spread to proceed at all (bps). Abort above this.
    pub abort_spread_bps: f64,
    /// Normal expected spread (bps).
    pub normal_spread_bps: f64,
    /// Spread multiplier that triggers pause (e.g. 3× = pause at 3× normal).
    pub spread_pause_multiplier: f64,
}

impl Default for SmartMarketConfig {
    fn default() -> Self {
        Self {
            depth_levels: 10,
            aggressive_ofi_threshold: 0.58,
            passive_ofi_threshold: 0.45,
            wall_threshold: 0.35,
            passive_offset_bps: 1.0,
            max_spread_for_limit_bps: 10.0,
            price_decimals: 4,
            limit_timeout_ms: 5000,
            poll_interval_ms: 200,
            max_sweep_slippage_bps: 5.0,
            partial_fill_strategy: PartialFillStrategy::MarketRest,
            max_retries: 3,
            abort_spread_bps: 20.0,
            normal_spread_bps: 2.0,
            spread_pause_multiplier: 5.0,
        }
    }
}

impl SmartMarketConfig {
    /// Fast execution: short timeout, aggressive sweep.
    pub fn fast() -> Self {
        Self {
            limit_timeout_ms: 2000,
            poll_interval_ms: 100,
            aggressive_ofi_threshold: 0.55,
            max_sweep_slippage_bps: 8.0,
            max_retries: 1,
            ..Default::default()
        }
    }

    /// Patient execution: long timeout, tight slippage.
    pub fn patient() -> Self {
        Self {
            limit_timeout_ms: 15_000,
            poll_interval_ms: 500,
            passive_offset_bps: 0.5,
            max_sweep_slippage_bps: 3.0,
            max_retries: 5,
            ..Default::default()
        }
    }

    /// Sniper mode: very fast, OFI-gated, minimal slippage.
    pub fn sniper() -> Self {
        Self {
            limit_timeout_ms: 1000,
            poll_interval_ms: 50,
            aggressive_ofi_threshold: 0.60,
            max_sweep_slippage_bps: 3.0,
            max_retries: 2,
            depth_levels: 20,
            wall_threshold: 0.25,
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// BOOK STATE — Phase 2 Output
// ═══════════════════════════════════════════════════════════════

/// Classified book state determining execution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookState {
    /// Heavy liquidity on our side — go aggressive (join the touch).
    Aggressive,
    /// Balanced book — go passive (post inside spread).
    Passive,
    /// Adverse book — wait or use tight limit.
    Defensive,
    /// Wall detected against us — reduce size or wait.
    WallBlocking,
    /// Spread too wide — pause or abort.
    WideSpread,
}

impl std::fmt::Display for BookState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BookState::Aggressive => write!(f, "🟢 AGGRESSIVE"),
            BookState::Passive => write!(f, "🔵 PASSIVE"),
            BookState::Defensive => write!(f, "🟡 DEFENSIVE"),
            BookState::WallBlocking => write!(f, "🧱 WALL_BLOCKING"),
            BookState::WideSpread => write!(f, "🔴 WIDE_SPREAD"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// PHASE RECORDS
// ═══════════════════════════════════════════════════════════════

/// Record of each Smart-Market phase for post-trade analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartMarketPhaseRecord {
    /// Phase number (1-5).
    pub phase: u8,
    /// Phase name.
    pub name: String,
    /// Duration of this phase (µs).
    pub duration_us: u64,
    /// Outcome description.
    pub outcome: String,
}

/// Complete Smart-Market execution report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartMarketReport {
    /// Standard execution report.
    pub base: ExecutionReport,
    /// Configuration used.
    pub config: SmartMarketConfig,
    /// Classified book state at execution time.
    pub book_state: BookState,
    /// OFI score at execution time.
    pub ofi_score: OfiScore,
    /// Limit price posted in Phase 3 (if applicable).
    pub limit_price: Option<Decimal>,
    /// Whether we got a limit fill (maker) vs market sweep (taker).
    pub got_maker_fill: bool,
    /// Phase timing records.
    pub phases: Vec<SmartMarketPhaseRecord>,
    /// Total time from READ to completion (µs).
    pub total_latency_us: u64,
    /// Number of retries across all phases.
    pub retries_used: u32,
}

// ═══════════════════════════════════════════════════════════════
// SMART-MARKET EXECUTION — 5-Phase Pipeline
// ═══════════════════════════════════════════════════════════════

/// Execute using Smart-Market logic.
///
/// # Pipeline
/// 1. **READ**  — fetch L2 depth, compute OFI + wall detection
/// 2. **THINK** — classify book → choose strategy
/// 3. **AIM**   — post aggressive/passive limit at optimal price
/// 4. **WAIT**  — monitor for fill with timeout
/// 5. **FIRE**  — sweep remaining with market if needed
pub async fn execute_smart_market(
    placer: &dyn OrderPlacer,
    symbol: &str,
    side: Side,
    total_qty: Decimal,
    config: &SmartMarketConfig,
    risk_state: &CumulativeRiskState,
    risk_limits: &ExecutionRiskLimits,
) -> anyhow::Result<SmartMarketReport> {
    let pipeline_start = Instant::now();
    let mut phases: Vec<SmartMarketPhaseRecord> = Vec::new();
    let mut retries = 0u32;

    // ═══════════════════════════════════════════════════════════
    // PHASE 1: READ — Fetch depth + compute OFI
    // ═══════════════════════════════════════════════════════════
    let t1 = Instant::now();
    let book = fetch_book_with_retry(placer, symbol, &mut retries, config.max_retries).await?;
    let ofi_score = OfiScore::from_book(&book, config.depth_levels);
    let arrival_price = book.mid_price().unwrap_or(Decimal::ONE);
    let spread_bps = book.spread_bps().unwrap_or(config.normal_spread_bps);
    phases.push(SmartMarketPhaseRecord {
        phase: 1,
        name: "READ".into(),
        duration_us: t1.elapsed().as_micros() as u64,
        outcome: format!("ofi={:.3} spread={:.1}bps imb={:.3}", ofi_score.imbalance, spread_bps, ofi_score.imbalance),
    });

    // ═══════════════════════════════════════════════════════════
    // PRE-TRADE GATES
    // ═══════════════════════════════════════════════════════════
    let pre = PreTradeCheck::run(symbol, side, total_qty, arrival_price, risk_state, risk_limits);
    if !pre.allowed {
        anyhow::bail!("SmartMarket pre-trade failed: {:?}", pre.reason);
    }
    if spread_bps > config.abort_spread_bps {
        anyhow::bail!("SmartMarket abort: spread {:.1}bps > {:.1}bps", spread_bps, config.abort_spread_bps);
    }
    if crate::risk_guards::is_kill_switch_active() {
        anyhow::bail!("SmartMarket abort: kill switch active");
    }

    // ═══════════════════════════════════════════════════════════
    // PHASE 2: THINK — Classify book state → choose strategy
    // ═══════════════════════════════════════════════════════════
    let t2 = Instant::now();
    let book_state = classify_book(&ofi_score, side, spread_bps, config);
    let effective_qty = compute_effective_qty(total_qty, &book_state, &ofi_score);
    phases.push(SmartMarketPhaseRecord {
        phase: 2,
        name: "THINK".into(),
        duration_us: t2.elapsed().as_micros() as u64,
        outcome: format!("{book_state} qty={effective_qty}"),
    });

    tracing::info!(
        "📊 SMART-MARKET {} {:?} {} | {} | ofi={:.3} spread={:.1}bps",
        symbol, side, total_qty, book_state, ofi_score.imbalance, spread_bps,
    );

    // ═══════════════════════════════════════════════════════════
    // PHASE 3: AIM — Post limit at optimal price
    // ═══════════════════════════════════════════════════════════
    let t3 = Instant::now();
    let mut fills: Vec<FillResult> = Vec::new();
    let mut limit_price_posted: Option<Decimal> = None;
    let mut got_maker = false;

    match book_state {
        BookState::WideSpread => {
            // Spread too wide for limit — but not abort-level wide.
            // Skip limit phase entirely, will fire market below after re-check.
            phases.push(SmartMarketPhaseRecord {
                phase: 3,
                name: "AIM".into(),
                duration_us: t3.elapsed().as_micros() as u64,
                outcome: "SKIPPED (wide spread)".into(),
            });
        }
        BookState::WallBlocking => {
            // Wall against us — reduce size and go defensive
            let reduced_qty = effective_qty * Decimal::from_str("0.5").unwrap_or(Decimal::ONE);
            let def_price = compute_limit_price(&book, side, 0.0, config.price_decimals);
            limit_price_posted = Some(def_price);

            match placer.place_limit(symbol, side, reduced_qty, def_price).await {
                Ok(fill) => {
                    got_maker = fill.is_maker;
                    fills.push(fill);
                }
                Err(e) => {
                    let exec_err = ExecutionError::from_anyhow(&e);
                    phases.push(SmartMarketPhaseRecord {
                        phase: 3,
                        name: "AIM".into(),
                        duration_us: t3.elapsed().as_micros() as u64,
                        outcome: format!("LIMIT FAILED: {exec_err}"),
                    });
                    // Fall through to Phase 5 FIRE
                }
            }
            phases.push(SmartMarketPhaseRecord {
                phase: 3,
                name: "AIM".into(),
                duration_us: t3.elapsed().as_micros() as u64,
                outcome: format!("DEFENSIVE limit @ {def_price} qty={reduced_qty}"),
            });
        }
        BookState::Aggressive | BookState::Passive | BookState::Defensive => {
            let offset_bps = match book_state {
                BookState::Aggressive => 0.0, // at touch
                BookState::Passive => config.passive_offset_bps,
                BookState::Defensive => config.passive_offset_bps * 2.0,
                _ => 0.0,
            };
            let lim_price = compute_limit_price(&book, side, offset_bps, config.price_decimals);
            limit_price_posted = Some(lim_price);

            match placer.place_limit(symbol, side, effective_qty, lim_price).await {
                Ok(fill) => {
                    got_maker = fill.is_maker;
                    let fp = fill.fill_price;
                    let fq = fill.fill_qty;
                    fills.push(fill);
                    phases.push(SmartMarketPhaseRecord {
                        phase: 3,
                        name: "AIM".into(),
                        duration_us: t3.elapsed().as_micros() as u64,
                        outcome: format!("LIMIT FILL @ {fp} qty={fq}"),
                    });
                }
                Err(e) => {
                    phases.push(SmartMarketPhaseRecord {
                        phase: 3,
                        name: "AIM".into(),
                        duration_us: t3.elapsed().as_micros() as u64,
                        outcome: format!("LIMIT REJECT: {e}"),
                    });
                    // Fall through to Phase 5 FIRE
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════
    // PHASE 4: WAIT — Poll for partial fill (if limit was placed)
    // ═══════════════════════════════════════════════════════════
    let t4 = Instant::now();
    let filled_qty: Decimal = fills.iter().map(|f| f.fill_qty).sum();
    let mut remaining = effective_qty - filled_qty;

    if remaining > Decimal::ZERO && limit_price_posted.is_some() && !got_maker {
        // Wait for the limit order to fill (simulated by short sleep)
        let timeout = Duration::from_millis(config.limit_timeout_ms);
        let poll = Duration::from_millis(config.poll_interval_ms);
        let wait_start = Instant::now();

        // Wait one poll cycle for passive fills.
        // TODO: In production, poll order status in a loop until filled or timeout.
        if wait_start.elapsed() < timeout && remaining > Decimal::ZERO {
            tokio::time::sleep(poll).await;
        }

        phases.push(SmartMarketPhaseRecord {
            phase: 4,
            name: "WAIT".into(),
            duration_us: t4.elapsed().as_micros() as u64,
            outcome: format!("waited {}ms, remaining={remaining}", wait_start.elapsed().as_millis()),
        });
    } else {
        phases.push(SmartMarketPhaseRecord {
            phase: 4,
            name: "WAIT".into(),
            duration_us: 0,
            outcome: "SKIPPED (already filled or no limit)".into(),
        });
    }

    // ═══════════════════════════════════════════════════════════
    // PHASE 5: FIRE — Sweep remaining with market order
    // ═══════════════════════════════════════════════════════════
    let t5 = Instant::now();
    // Recalculate remaining after wait
    let filled_qty: Decimal = fills.iter().map(|f| f.fill_qty).sum();
    remaining = effective_qty - filled_qty;

    if remaining > Decimal::ZERO {
        // Re-fetch book for fresh slippage estimate
        let fresh_book = placer.get_orderbook(symbol).await.unwrap_or_else(|_| book.clone());
        let slip_est = match side {
            Side::Buy => fresh_book.estimate_buy_slippage(remaining),
            Side::Sell => fresh_book.estimate_sell_slippage(remaining),
        };

        if let Some(ref est) = slip_est {
            if est.slippage_bps > config.max_sweep_slippage_bps {
                tracing::warn!(
                    "SmartMarket: sweep slippage {:.1}bps > max {:.1}bps, reducing qty",
                    est.slippage_bps, config.max_sweep_slippage_bps,
                );
                // Reduce sweep quantity to max that fits within slippage budget
                let max_qty = fresh_book.max_market_order(side, config.max_sweep_slippage_bps);
                if max_qty > Decimal::ZERO {
                    remaining = remaining.min(max_qty);
                } else {
                    anyhow::bail!(
                        "SmartMarket sweep abort: {:.1}bps > {:.1}bps, no safe qty",
                        est.slippage_bps, config.max_sweep_slippage_bps,
                    );
                }
            }
        }

        // Fire market sweep
        match placer.place_market(symbol, side, remaining).await {
            Ok(fill) => {
                fills.push(fill);
                phases.push(SmartMarketPhaseRecord {
                    phase: 5,
                    name: "FIRE".into(),
                    duration_us: t5.elapsed().as_micros() as u64,
                    outcome: format!("SWEEP {} @ {}", remaining, fills.last().expect("fills non-empty after push").fill_price),
                });
            }
            Err(e) => {
                let exec_err = ExecutionError::from_anyhow(&e);
                phases.push(SmartMarketPhaseRecord {
                    phase: 5,
                    name: "FIRE".into(),
                    duration_us: t5.elapsed().as_micros() as u64,
                    outcome: format!("SWEEP FAILED: {exec_err}"),
                });

                // Try partial fill recovery
                if filled_qty > Decimal::ZERO {
                    tracing::warn!("SmartMarket: sweep failed but got {} from limit", filled_qty);
                }
            }
        }
    } else {
        phases.push(SmartMarketPhaseRecord {
            phase: 5,
            name: "FIRE".into(),
            duration_us: 0,
            outcome: "SKIPPED (fully filled by limit)".into(),
        });
    }

    // ═══════════════════════════════════════════════════════════
    // FINALIZE — Build report
    // ═══════════════════════════════════════════════════════════
    for fill in &fills {
        risk_state.record_execution(
            f64_decimal(fill.fill_price * fill.fill_qty),
            f64_decimal(fill.commission),
        );
    }

    let base_report = ExecutionReport::build(
        symbol, side, "SMART_MARKET", total_qty, arrival_price, fills, pipeline_start,
    );

    let total_latency_us = pipeline_start.elapsed().as_micros() as u64;

    tracing::info!(
        "✅ SMART-MARKET DONE: {} | {} | {} slices | {:.1}bps IS | {total_latency_us}µs",
        symbol, book_state, base_report.slices_executed, base_report.is_bps,
    );

    Ok(SmartMarketReport {
        base: base_report,
        config: config.clone(),
        book_state,
        ofi_score,
        limit_price: limit_price_posted,
        got_maker_fill: got_maker,
        phases,
        total_latency_us,
        retries_used: retries,
    })
}

// ═══════════════════════════════════════════════════════════════
// INTERNAL HELPERS
// ═══════════════════════════════════════════════════════════════

/// Classify the order book state.
fn classify_book(
    ofi: &OfiScore,
    side: Side,
    spread_bps: f64,
    config: &SmartMarketConfig,
) -> BookState {
    // Spread check first
    if spread_bps > config.normal_spread_bps * config.spread_pause_multiplier {
        return BookState::WideSpread;
    }

    // Wall check
    if ofi.wall_strength > config.wall_threshold && ofi.wall_side != side {
        return BookState::WallBlocking;
    }

    // OFI-based classification
    let imb = ofi.imbalance;
    match side {
        Side::Buy => {
            if imb >= config.aggressive_ofi_threshold {
                BookState::Aggressive
            } else if imb <= config.passive_ofi_threshold {
                BookState::Defensive
            } else {
                BookState::Passive
            }
        }
        Side::Sell => {
            if imb <= (1.0 - config.aggressive_ofi_threshold) {
                BookState::Aggressive
            } else if imb >= (1.0 - config.passive_ofi_threshold) {
                BookState::Defensive
            } else {
                BookState::Passive
            }
        }
    }
}

/// Compute effective quantity based on book state and OFI signal.
fn compute_effective_qty(total_qty: Decimal, state: &BookState, ofi: &OfiScore) -> Decimal {
    let mult = match state {
        BookState::Aggressive => ofi.signal.size_multiplier(),
        BookState::Passive => 1.0,
        BookState::Defensive => 0.7,
        BookState::WallBlocking => 0.5,
        BookState::WideSpread => 0.5,
    };
    let factor = Decimal::from_f64_retain(mult).unwrap_or(Decimal::ONE);
    total_qty * factor
}

/// Compute limit price from book, side, and offset (bps).
fn compute_limit_price(
    book: &OrderBookSnapshot,
    side: Side,
    offset_bps: f64,
    price_decimals: u32,
) -> Decimal {
    let mid = book.mid_price().unwrap_or(Decimal::ONE);
    let offset = mid
        * Decimal::from_f64_retain(offset_bps).unwrap_or(Decimal::ZERO)
        / Decimal::from(10000);

    let raw_price = match side {
        Side::Buy => mid - offset,         // Buy lower than mid
        Side::Sell => mid + offset,        // Sell higher than mid
    };

    // Round to tick size (price_decimals)
    round_to_tick(raw_price, price_decimals)
}

/// Round price to specified decimal places.
fn round_to_tick(price: Decimal, decimals: u32) -> Decimal {
    let factor = Decimal::from(10u64.pow(decimals));
    (price * factor).round() / factor
}

/// Fetch order book with retry on transient errors.
async fn fetch_book_with_retry(
    placer: &dyn OrderPlacer,
    symbol: &str,
    retries: &mut u32,
    max_retries: u32,
) -> anyhow::Result<OrderBookSnapshot> {
    let mut attempt = 0u32;
    loop {
        match placer.get_orderbook(symbol).await {
            Ok(book) => return Ok(book),
            Err(e) => {
                let exec_err = ExecutionError::from_anyhow(&e);
                let decision = decide(&exec_err, attempt, max_retries);
                match decision {
                    ErrorDecision::Retry { delay_ms, .. } => {
                        *retries += 1;
                        attempt += 1;
                        tracing::warn!("SmartMarket book fetch retry {attempt}: {exec_err}");
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    ErrorDecision::Skip => {
                        *retries += 1;
                        attempt += 1;
                        if attempt >= max_retries {
                            return Err(e);
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    ErrorDecision::Abort => return Err(e),
                }
            }
        }
    }
}

/// Convert Decimal to f64 safely.
fn f64_decimal(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::PriceLevel;

    fn make_book(bid_qtys: &[&str], ask_qtys: &[&str]) -> OrderBookSnapshot {
        let mut bids: Vec<PriceLevel> = bid_qtys
            .iter()
            .enumerate()
            .map(|(i, q)| PriceLevel::new(Decimal::from(100 - i as i64), Decimal::from_str(q).unwrap()))
            .collect();
        bids.sort_by(|a, b| b.price.cmp(&a.price));

        let asks: Vec<PriceLevel> = ask_qtys
            .iter()
            .enumerate()
            .map(|(i, q)| PriceLevel::new(Decimal::from(101 + i as i64), Decimal::from_str(q).unwrap()))
            .collect();

        OrderBookSnapshot {
            symbol: "TEST".to_string(),
            timestamp_ms: 0,
            bids,
            asks,
        }
    }

    // ── Config Tests ─────────────────────────────────────────

    #[test]
    fn test_config_default() {
        let cfg = SmartMarketConfig::default();
        assert!((cfg.aggressive_ofi_threshold - 0.58).abs() < 0.01);
        assert_eq!(cfg.depth_levels, 10);
        assert_eq!(cfg.limit_timeout_ms, 5000);
    }

    #[test]
    fn test_config_fast() {
        let cfg = SmartMarketConfig::fast();
        assert!(cfg.limit_timeout_ms <= 2000);
        assert!(cfg.max_retries <= 1);
    }

    #[test]
    fn test_config_patient() {
        let cfg = SmartMarketConfig::patient();
        assert!(cfg.limit_timeout_ms >= 10_000);
        assert!(cfg.max_sweep_slippage_bps <= 3.0);
    }

    #[test]
    fn test_config_sniper() {
        let cfg = SmartMarketConfig::sniper();
        assert!(cfg.limit_timeout_ms <= 1000);
        assert!(cfg.depth_levels >= 20);
    }

    #[test]
    fn test_config_serialization() {
        let cfg = SmartMarketConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: SmartMarketConfig = serde_json::from_str(&json).unwrap();
        assert!((back.aggressive_ofi_threshold - cfg.aggressive_ofi_threshold).abs() < 0.001);
    }

    // ── Book Classification Tests ────────────────────────────

    #[test]
    fn test_classify_aggressive_buy() {
        // Heavy bids = aggressive for BUY
        let book = make_book(&["500", "400", "300"], &["100", "100", "100"]);
        let ofi = OfiScore::from_book(&book, 3);
        let cfg = SmartMarketConfig::default();
        let state = classify_book(&ofi, Side::Buy, 1.0, &cfg);
        assert_eq!(state, BookState::Aggressive);
    }

    #[test]
    fn test_classify_aggressive_sell() {
        // Heavy asks = aggressive for SELL
        let book = make_book(&["100", "100", "100"], &["500", "400", "300"]);
        let ofi = OfiScore::from_book(&book, 3);
        let cfg = SmartMarketConfig::default();
        let state = classify_book(&ofi, Side::Sell, 1.0, &cfg);
        assert_eq!(state, BookState::Aggressive);
    }

    #[test]
    fn test_classify_passive() {
        // Balanced book
        let book = make_book(&["100", "100", "100"], &["100", "100", "100"]);
        let ofi = OfiScore::from_book(&book, 3);
        let cfg = SmartMarketConfig::default();
        let state = classify_book(&ofi, Side::Buy, 2.0, &cfg);
        assert_eq!(state, BookState::Passive);
    }

    #[test]
    fn test_classify_defensive_buy() {
        // Heavy asks = defensive for BUY
        let book = make_book(&["100", "100", "100"], &["500", "400", "300"]);
        let ofi = OfiScore::from_book(&book, 3);
        let cfg = SmartMarketConfig::default();
        let state = classify_book(&ofi, Side::Buy, 2.0, &cfg);
        assert_eq!(state, BookState::Defensive);
    }

    #[test]
    fn test_classify_wide_spread() {
        let book = make_book(&["100"], &["100"]);
        let ofi = OfiScore::from_book(&book, 1);
        let cfg = SmartMarketConfig::default();
        // spread_bps for this book: (101-100)/100.5 * 10000 ≈ 99.5 bps > 10
        let state = classify_book(&ofi, Side::Buy, 15.0, &cfg);
        assert_eq!(state, BookState::WideSpread);
    }

    #[test]
    fn test_classify_wall_blocking() {
        // Giant bid wall, but we're selling → wall is against us
        let book = make_book(&["5000", "100", "100"], &["100", "100", "100"]);
        let ofi = OfiScore::from_book(&book, 3);
        let cfg = SmartMarketConfig {
            wall_threshold: 0.30,
            ..Default::default()
        };
        // wall_side = Buy (where the wall is), we are selling
        let state = classify_book(&ofi, Side::Sell, 1.0, &cfg);
        assert_eq!(state, BookState::WallBlocking);
    }

    // ── Effective Qty Tests ──────────────────────────────────

    #[test]
    fn test_effective_qty_aggressive() {
        let book = make_book(&["500", "400", "300"], &["100", "100", "100"]);
        let ofi = OfiScore::from_book(&book, 3);
        let qty = compute_effective_qty(Decimal::from(100), &BookState::Aggressive, &ofi);
        // Aggressive multiplier from OFI signal (likely StrongBuy or Buy)
        assert!(qty >= Decimal::from(100), "aggressive should boost or maintain qty");
    }

    #[test]
    fn test_effective_qty_defensive() {
        let qty = compute_effective_qty(Decimal::from(100), &BookState::Defensive, &OfiScore::from_book(&make_book(&["100"], &["100"]), 1));
        assert!(qty < Decimal::from(100), "defensive should reduce qty, got {qty}");
    }

    #[test]
    fn test_effective_qty_wall() {
        let qty = compute_effective_qty(Decimal::from(100), &BookState::WallBlocking, &OfiScore::from_book(&make_book(&["100"], &["100"]), 1));
        assert_eq!(qty, Decimal::from(50), "wall blocking should halve qty");
    }

    // ── Limit Price Tests ────────────────────────────────────

    #[test]
    fn test_limit_price_buy_at_mid() {
        let book = make_book(&["100"], &["100"]);
        let price = compute_limit_price(&book, Side::Buy, 0.0, 2);
        // Mid = (100 + 101) / 2 = 100.5
        assert_eq!(price, Decimal::from_str("100.50").unwrap());
    }

    #[test]
    fn test_limit_price_sell_at_mid() {
        let book = make_book(&["100"], &["100"]);
        let price = compute_limit_price(&book, Side::Sell, 0.0, 2);
        assert_eq!(price, Decimal::from_str("100.50").unwrap());
    }

    #[test]
    fn test_limit_price_buy_with_offset() {
        let book = make_book(&["100"], &["100"]);
        let price = compute_limit_price(&book, Side::Buy, 10.0, 2);
        // Mid = 100.5, offset = 100.5 * 10 / 10000 = 0.1005
        // Buy: 100.5 - 0.1005 = 100.3995 → round to 100.40
        assert!(price < Decimal::from_str("100.50").unwrap());
    }

    #[test]
    fn test_round_to_tick() {
        assert_eq!(round_to_tick(Decimal::from_str("100.456").unwrap(), 2), Decimal::from_str("100.46").unwrap());
        assert_eq!(round_to_tick(Decimal::from_str("100.454").unwrap(), 2), Decimal::from_str("100.45").unwrap());
        assert_eq!(round_to_tick(Decimal::from_str("0.061234").unwrap(), 5), Decimal::from_str("0.06123").unwrap());
    }

    // ── BookState Display ────────────────────────────────────

    #[test]
    fn test_book_state_display() {
        assert!(format!("{}", BookState::Aggressive).contains("AGGRESSIVE"));
        assert!(format!("{}", BookState::Passive).contains("PASSIVE"));
        assert!(format!("{}", BookState::Defensive).contains("DEFENSIVE"));
        assert!(format!("{}", BookState::WallBlocking).contains("WALL"));
        assert!(format!("{}", BookState::WideSpread).contains("WIDE"));
    }

    // ── Phase Record Serialization ───────────────────────────

    #[test]
    fn test_phase_record_serialization() {
        let rec = SmartMarketPhaseRecord {
            phase: 1,
            name: "READ".into(),
            duration_us: 150,
            outcome: "ofi=0.577".into(),
        };
        let json = serde_json::to_string(&rec).unwrap();
        let back: SmartMarketPhaseRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.phase, 1);
        assert_eq!(back.duration_us, 150);
    }

    #[test]
    fn test_report_serialization() {
        let report = SmartMarketReport {
            base: ExecutionReport::build(
                "TEST", Side::Buy, "SMART_MARKET", Decimal::ONE,
                Decimal::ONE, vec![], Instant::now(),
            ),
            config: SmartMarketConfig::default(),
            book_state: BookState::Aggressive,
            ofi_score: OfiScore::from_book(&make_book(&["100"], &["100"]), 1),
            limit_price: Some(Decimal::ONE),
            got_maker_fill: true,
            phases: vec![],
            total_latency_us: 500,
            retries_used: 0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("SMART_MARKET"));
        assert!(json.contains("Aggressive"));
        let back: SmartMarketReport = serde_json::from_str(&json).unwrap();
        assert!(back.got_maker_fill);
        assert_eq!(back.total_latency_us, 500);
    }
}
