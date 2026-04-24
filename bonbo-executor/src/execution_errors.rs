//! Typed error handling for execution: RateLimit, PartialFill, Network, and more.
//!
//! # Problem
//! All execution algorithms (TWAP/VWAP/POV/IS/OFI) currently use `anyhow::Error`
//! for everything — rate limits, partial fills, network errors, and rejections
//! are all the same type. Algorithms can't make intelligent retry/skip/abort
//! decisions without matching on error strings.
//!
//! # Solution
//! `ExecutionError` — typed, structured errors with actionable metadata:
//! - **RateLimited**: back off for `retry_after_secs`
//! - **PartialFill**: we got X of Y, need to handle the rest
//! - **InsufficientMargin**: can't place the order at all
//! - **Rejected**: Binance rejected (price, quantity, etc.)
//! - **NetworkError**: transient, retry with backoff
//! - **KillSwitched**: global abort
//! - **SpreadTooWide**: pause, not abort
//! - **SlippageExceeded**: reduce size or skip
//!
//! # Usage in Algorithms
//! ```ignore
//! match placer.place_market(symbol, side, qty).await {
//!     Ok(fill) => { /* happy path */ }
//!     Err(e) => match ExecutionError::from_anyhow(&e) {
//!         ExecutionError::RateLimited { retry_after_secs } => {
//!             tokio::time::sleep(Duration::from_secs(retry_after_secs)).await;
//!             continue; // retry same slice
//!         }
//!         ExecutionError::PartialFill { filled, remaining } => {
//!             remaining_qty = remaining;
//!             continue; // handle rest
//!         }
//!         ExecutionError::SpreadTooWide { spread_bps } => {
//!             pause_count += 1;
//!             if pause_count > 3 { break; }
//!             continue; // skip and wait
//!         }
//!         ExecutionError::KillSwitched => break, // abort all
//!         _ => { /* other: log and skip */ }
//!     }
//! }
//! ```

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

// ═══════════════════════════════════════════════════════════════
// BINARY ERROR CODES (from Binance API docs)
// ═══════════════════════════════════════════════════════════════

/// Known Binance API error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinanceErrorCode {
    /// -1000: UNKNOWN
    Unknown,
    /// -1001: DISCONNECTED (internal error)
    Disconnected,
    /// -1015: TOO_MANY_ORDERS
    TooManyOrders,
    /// -1016: SERVICE_SHUTTING_DOWN
    ServiceShuttingDown,
    /// -1020: UNSUPPORTED_ORDERS
    UnsupportedOrders,
    /// -1021: TIMESTAMP_NOT_SYNCED
    TimestampNotSynced,
    /// -1022: INVALID_SIGNATURE
    InvalidSignature,
    /// -2010: NEW_ORDER_REJECTED
    NewOrderRejected,
    /// -2011: CANCEL_REJECTED
    CancelRejected,
    /// -2013: NO_SUCH_ORDER
    NoSuchOrder,
    /// -2019: MARGIN_INSUFFICIENT
    MarginInsufficient,
    /// -2022: EXCEED_MAX_POSITION
    ExceedMaxPosition,
    /// -4000: INVALID_PRICE
    InvalidPrice,
    /// -4001: INVALID_QUANTITY
    InvalidQuantity,
    /// -4003: MIN_NOTIONAL
    MinNotional,
    /// -4131: REDUCE_ONLY_REJECT
    ReduceOnlyReject,
    /// Any other code
    Other(i64),
}

impl BinanceErrorCode {
    /// Parse from Binance API numeric code.
    pub fn from_code(code: i64) -> Self {
        match code {
            -1000 => Self::Unknown,
            -1001 => Self::Disconnected,
            -1015 => Self::TooManyOrders,
            -1016 => Self::ServiceShuttingDown,
            -1020 => Self::UnsupportedOrders,
            -1021 => Self::TimestampNotSynced,
            -1022 => Self::InvalidSignature,
            -2010 => Self::NewOrderRejected,
            -2011 => Self::CancelRejected,
            -2013 => Self::NoSuchOrder,
            -2019 => Self::MarginInsufficient,
            -2022 => Self::ExceedMaxPosition,
            -4000 => Self::InvalidPrice,
            -4001 => Self::InvalidQuantity,
            -4003 => Self::MinNotional,
            -4131 => Self::ReduceOnlyReject,
            other => Self::Other(other),
        }
    }

    /// Is this a transient error (retry makes sense)?
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::TooManyOrders
                | Self::Disconnected
                | Self::TimestampNotSynced
                | Self::ServiceShuttingDown
        )
    }

    /// Is this a permanent rejection (retry won't help)?
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            Self::MarginInsufficient
                | Self::ExceedMaxPosition
                | Self::InvalidSignature
                | Self::MinNotional
                | Self::InvalidPrice
                | Self::InvalidQuantity
        )
    }
}

impl fmt::Display for BinanceErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "UNKNOWN(-1000)"),
            Self::Disconnected => write!(f, "DISCONNECTED(-1001)"),
            Self::TooManyOrders => write!(f, "TOO_MANY_ORDERS(-1015)"),
            Self::ServiceShuttingDown => write!(f, "SERVICE_SHUTTING_DOWN(-1016)"),
            Self::UnsupportedOrders => write!(f, "UNSUPPORTED_ORDERS(-1020)"),
            Self::TimestampNotSynced => write!(f, "TIMESTAMP_NOT_SYNCED(-1021)"),
            Self::InvalidSignature => write!(f, "INVALID_SIGNATURE(-1022)"),
            Self::NewOrderRejected => write!(f, "NEW_ORDER_REJECTED(-2010)"),
            Self::CancelRejected => write!(f, "CANCEL_REJECTED(-2011)"),
            Self::NoSuchOrder => write!(f, "NO_SUCH_ORDER(-2013)"),
            Self::MarginInsufficient => write!(f, "MARGIN_INSUFFICIENT(-2019)"),
            Self::ExceedMaxPosition => write!(f, "EXCEED_MAX_POSITION(-2022)"),
            Self::InvalidPrice => write!(f, "INVALID_PRICE(-4000)"),
            Self::InvalidQuantity => write!(f, "INVALID_QUANTITY(-4001)"),
            Self::MinNotional => write!(f, "MIN_NOTIONAL(-4003)"),
            Self::ReduceOnlyReject => write!(f, "REDUCE_ONLY_REJECT(-4131)"),
            Self::Other(code) => write!(f, "OTHER({code})"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// EXECUTION ERROR — Typed, Actionable
// ═══════════════════════════════════════════════════════════════

/// Typed execution error with actionable recovery strategies.
///
/// Each variant carries the metadata an algorithm needs to decide:
/// - retry? with what delay?
/// - skip this slice?
/// - reduce order size?
/// - abort entire execution?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionError {
    // ── Rate Limiting ────────────────────────────────────────
    /// Binance rate limit hit. Back off for specified duration.
    RateLimited {
        /// Binance error code.
        code: BinanceErrorCode,
        /// Human-readable message.
        message: String,
        /// How long to wait before retrying (seconds).
        retry_after_secs: u64,
    },

    // ── Partial Fills ────────────────────────────────────────
    /// Order was partially filled. Need to handle the remainder.
    PartialFill {
        /// Quantity that was filled.
        filled_qty: Decimal,
        /// Quantity still unfilled.
        remaining_qty: Decimal,
        /// Average fill price for the filled portion.
        fill_price: Decimal,
        /// Commission on filled portion.
        commission: Decimal,
        /// Reason the rest didn't fill (e.g., "IOC expired").
        reason: String,
    },

    // ── Order Rejections ─────────────────────────────────────
    /// Insufficient margin to place the order.
    InsufficientMargin {
        /// Required margin.
        required: Decimal,
        /// Available margin.
        available: Decimal,
    },

    /// Order would exceed maximum position.
    ExceedMaxPosition {
        /// Current position.
        current: Decimal,
        /// Max allowed.
        max: Decimal,
    },

    /// Order rejected by exchange.
    Rejected {
        /// Binance error code.
        code: BinanceErrorCode,
        /// Rejection reason.
        reason: String,
    },

    // ── Market Conditions ────────────────────────────────────
    /// Spread is too wide to execute.
    SpreadTooWide {
        /// Current spread in bps.
        spread_bps: f64,
        /// Maximum acceptable spread in bps.
        max_spread_bps: f64,
    },

    /// Estimated slippage exceeds maximum.
    SlippageExceeded {
        /// Estimated slippage in bps.
        estimated_bps: f64,
        /// Maximum allowed in bps.
        max_bps: f64,
    },

    // ── Control Flow ─────────────────────────────────────────
    /// Global kill switch activated.
    KillSwitched,

    /// Pre-trade risk check failed.
    RiskCheckFailed {
        /// Reason.
        reason: String,
    },

    // ── Transient ────────────────────────────────────────────
    /// Network / connectivity error.
    NetworkError {
        /// Error description.
        message: String,
        /// Suggested retry count so far.
        retry_count: u32,
    },

    /// Timestamp out of sync with server.
    TimestampSync {
        /// Estimated drift in ms.
        drift_ms: i64,
    },

    /// Unknown / unexpected error.
    Unknown {
        /// Raw error message.
        message: String,
    },
}

impl ExecutionError {
    // ── Constructors ─────────────────────────────────────────

    /// Create from a Binance API error message.
    /// Parses "Binance API error: -1015 — Too many orders" format.
    pub fn from_binance_api(msg: &str) -> Self {
        // Parse "Binance API error: -1015 — Too many orders"
        let (code, message) = if let Some(rest) = msg.strip_prefix("Binance API error: ") {
            let parts: Vec<&str> = rest.splitn(2, " — ").collect();
            if parts.len() == 2 {
                let code_num: i64 = parts[0].trim().parse().unwrap_or(0);
                (BinanceErrorCode::from_code(code_num), parts[1].trim().to_string())
            } else {
                (BinanceErrorCode::Unknown, rest.to_string())
            }
        } else {
            (BinanceErrorCode::Unknown, msg.to_string())
        };

        match code {
            BinanceErrorCode::TooManyOrders => Self::RateLimited {
                code,
                message,
                retry_after_secs: 10,
            },
            BinanceErrorCode::Disconnected => Self::NetworkError {
                message,
                retry_count: 0,
            },
            BinanceErrorCode::TimestampNotSynced => Self::TimestampSync { drift_ms: 0 },
            BinanceErrorCode::MarginInsufficient => Self::InsufficientMargin {
                required: Decimal::ZERO,
                available: Decimal::ZERO,
            },
            BinanceErrorCode::ExceedMaxPosition => Self::ExceedMaxPosition {
                current: Decimal::ZERO,
                max: Decimal::ZERO,
            },
            BinanceErrorCode::NewOrderRejected => Self::Rejected { code, reason: message },
            _ => Self::Unknown { message },
        }
    }

    /// Create from an anyhow error (tries to parse Binance format).
    pub fn from_anyhow(err: &anyhow::Error) -> Self {
        let msg = format!("{err:#}");
        if msg.contains("Binance API error:") {
            Self::from_binance_api(&msg)
        } else if msg.contains("kill switch") {
            Self::KillSwitched
        } else if msg.contains("spread") && msg.contains("bps") {
            Self::SpreadTooWide {
                spread_bps: 0.0,
                max_spread_bps: 0.0,
            }
        } else if msg.contains("slippage") {
            Self::SlippageExceeded {
                estimated_bps: 0.0,
                max_bps: 0.0,
            }
        } else if msg.contains("pre-trade") || msg.contains("risk") {
            Self::RiskCheckFailed { reason: msg }
        } else if msg.contains("timeout") || msg.contains("connection") {
            Self::NetworkError {
                message: msg,
                retry_count: 0,
            }
        } else {
            Self::Unknown { message: msg }
        }
    }

    // ── Classification ───────────────────────────────────────

    /// Can we retry this error?
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. }
                | Self::NetworkError { .. }
                | Self::TimestampSync { .. }
                | Self::SpreadTooWide { .. }
        )
    }

    /// Should we abort the entire execution?
    pub fn should_abort(&self) -> bool {
        matches!(
            self,
            Self::KillSwitched
                | Self::InsufficientMargin { .. }
                | Self::ExceedMaxPosition { .. }
                | Self::RiskCheckFailed { .. }
        )
    }

    /// Should we skip this slice and move to the next?
    pub fn should_skip(&self) -> bool {
        matches!(
            self,
            Self::SlippageExceeded { .. }
                | Self::Rejected { .. }
                | Self::Unknown { .. }
        )
    }

    /// Is this a partial fill (need to handle remainder)?
    pub fn is_partial_fill(&self) -> bool {
        matches!(self, Self::PartialFill { .. })
    }

    /// Get suggested retry delay.
    pub fn retry_delay(&self) -> Duration {
        match self {
            Self::RateLimited { retry_after_secs, .. } => {
                Duration::from_secs(*retry_after_secs)
            }
            Self::NetworkError { retry_count, .. } => {
                let base = Duration::from_millis(500);
                base * 2u32.pow(*retry_count.min(&5))
            }
            Self::TimestampSync { .. } => Duration::from_secs(1),
            Self::SpreadTooWide { .. } => Duration::from_secs(5),
            _ => Duration::from_secs(2),
        }
    }
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RateLimited { code, retry_after_secs, .. } => {
                write!(f, "RATE_LIMITED({code}, retry after {retry_after_secs}s)")
            }
            Self::PartialFill { filled_qty, remaining_qty, fill_price, .. } => {
                write!(f, "PARTIAL_FILL(got {filled_qty} @ {fill_price}, remaining {remaining_qty})")
            }
            Self::InsufficientMargin { required, available } => {
                write!(f, "INSUFFICIENT_MARGIN(need {required}, have {available})")
            }
            Self::ExceedMaxPosition { current, max } => {
                write!(f, "EXCEED_MAX_POSITION(have {current}, max {max})")
            }
            Self::Rejected { code, reason } => {
                write!(f, "REJECTED({code}: {reason})")
            }
            Self::SpreadTooWide { spread_bps, max_spread_bps } => {
                write!(f, "SPREAD_TOO_WIDE({spread_bps:.1}bps > {max_spread_bps:.1}bps)")
            }
            Self::SlippageExceeded { estimated_bps, max_bps } => {
                write!(f, "SLIPPAGE_EXCEEDED({estimated_bps:.1}bps > {max_bps:.1}bps)")
            }
            Self::KillSwitched => write!(f, "KILL_SWITCHED"),
            Self::RiskCheckFailed { reason } => write!(f, "RISK_CHECK_FAILED({reason})"),
            Self::NetworkError { message, retry_count } => {
                write!(f, "NETWORK_ERROR(retry #{retry_count}: {message})")
            }
            Self::TimestampSync { drift_ms } => {
                write!(f, "TIMESTAMP_SYNC(drift {drift_ms}ms)")
            }
            Self::Unknown { message } => write!(f, "UNKNOWN({message})"),
        }
    }
}

impl std::error::Error for ExecutionError {}

// ═══════════════════════════════════════════════════════════════
// PARTIAL FILL HANDLER
// ═══════════════════════════════════════════════════════════════

/// Strategies for handling partial fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartialFillStrategy {
    /// Immediately place a market order for the remaining quantity.
    MarketRest,
    /// Place a limit order at last trade price for the remainder.
    LimitRest,
    /// Cancel remaining and absorb the partial fill.
    AcceptAndMove,
    /// Cancel remaining and retry the full slice.
    RetryFull,
}

/// Result of handling a partial fill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialFillResult {
    /// Quantity from the initial fill.
    pub initial_fill_qty: Decimal,
    /// Price from the initial fill.
    pub initial_fill_price: Decimal,
    /// Strategy used for the remainder.
    pub strategy: PartialFillStrategy,
    /// Final total quantity filled (after recovery).
    pub total_filled_qty: Decimal,
    /// Final VWAP across all fills.
    pub total_vwap: Decimal,
    /// Total commission.
    pub total_commission: Decimal,
    /// How many recovery attempts were needed.
    pub recovery_attempts: usize,
    /// Did we fully fill?
    pub fully_filled: bool,
}

/// Handle a partial fill with the chosen strategy.
pub async fn handle_partial_fill(
    placer: &dyn crate::execution_algo::OrderPlacer,
    symbol: &str,
    side: crate::orderbook::Side,
    original_qty: Decimal,
    filled_qty: Decimal,
    fill_price: Decimal,
    strategy: PartialFillStrategy,
) -> anyhow::Result<PartialFillResult> {
    let remaining = original_qty - filled_qty;
    if remaining <= Decimal::ZERO {
        return Ok(PartialFillResult {
            initial_fill_qty: filled_qty,
            initial_fill_price: fill_price,
            strategy,
            total_filled_qty: filled_qty,
            total_vwap: fill_price,
            total_commission: Decimal::ZERO,
            recovery_attempts: 0,
            fully_filled: true,
        });
    }

    let mut total_filled = filled_qty;
    let mut total_notional = fill_price * filled_qty;
    let mut total_commission = Decimal::ZERO;
    let mut attempts = 0;

    match strategy {
        PartialFillStrategy::AcceptAndMove => {
            tracing::warn!(
                "PartialFill: accepting {filled_qty}/{original_qty}, moving on ({remaining} unfilled)"
            );
        }
        PartialFillStrategy::MarketRest => {
            tracing::info!("PartialFill: market-rest {remaining} of {symbol}");
            attempts = 1;
            match placer.place_market(symbol, side, remaining).await {
                Ok(fill) => {
                    total_filled += fill.fill_qty;
                    total_notional += fill.fill_price * fill.fill_qty;
                    total_commission += fill.commission;
                }
                Err(e) => {
                    tracing::warn!("PartialFill: market-rest failed: {e}");
                }
            }
        }
        PartialFillStrategy::LimitRest => {
            tracing::info!("PartialFill: limit-rest {remaining} of {symbol} @ {fill_price}");
            attempts = 1;
            match placer.place_limit(symbol, side, remaining, fill_price).await {
                Ok(fill) => {
                    total_filled += fill.fill_qty;
                    total_notional += fill.fill_price * fill.fill_qty;
                    total_commission += fill.commission;
                }
                Err(e) => {
                    tracing::warn!("PartialFill: limit-rest failed: {e}");
                }
            }
        }
        PartialFillStrategy::RetryFull => {
            tracing::info!("PartialFill: retrying full {original_qty} of {symbol}");
            attempts = 1;
            match placer.place_market(symbol, side, remaining).await {
                Ok(fill) => {
                    total_filled += fill.fill_qty;
                    total_notional += fill.fill_price * fill.fill_qty;
                    total_commission += fill.commission;
                }
                Err(e) => {
                    tracing::warn!("PartialFill: retry-full failed: {e}");
                }
            }
        }
    }

    let total_vwap = if total_filled > Decimal::ZERO {
        total_notional / total_filled
    } else {
        fill_price
    };

    Ok(PartialFillResult {
        initial_fill_qty: filled_qty,
        initial_fill_price: fill_price,
        strategy,
        total_filled_qty: total_filled,
        total_vwap,
        total_commission,
        recovery_attempts: attempts,
        fully_filled: total_filled >= original_qty,
    })
}

// ═══════════════════════════════════════════════════════════════
// EXECUTION ERROR HANDLER — Unified Retry/Skip/Abort Logic
// ═══════════════════════════════════════════════════════════════

/// Decision after processing an execution error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorDecision {
    /// Retry the same slice after the specified delay.
    Retry { delay_ms: u64, max_retries: u32 },
    /// Skip this slice and move on.
    Skip,
    /// Abort the entire execution.
    Abort,
}

/// Decide what to do with an execution error.
pub fn decide(error: &ExecutionError, retry_count: u32, max_retries: u32) -> ErrorDecision {
    // Always abort on these
    if error.should_abort() {
        tracing::error!("🚨 EXEC ABORT: {error}");
        return ErrorDecision::Abort;
    }

    // Partial fills are handled separately
    if error.is_partial_fill() {
        return ErrorDecision::Skip; // handled by handle_partial_fill
    }

    // Retryable errors with backoff
    if error.is_retryable() && retry_count < max_retries {
        let delay = error.retry_delay();
        let delay_ms = delay.as_millis() as u64;
        tracing::warn!(
            "⚠️ EXEC RETRY ({}/{max_retries}): {error} — waiting {delay_ms}ms",
            retry_count + 1,
        );
        return ErrorDecision::Retry {
            delay_ms,
            max_retries,
        };
    }

    // Max retries exceeded
    if error.is_retryable() && retry_count >= max_retries {
        tracing::error!("🚨 EXEC RETRY EXHAUSTED: {error}");
        return ErrorDecision::Abort;
    }

    // Skip non-retryable errors
    if error.should_skip() {
        tracing::warn!("⏭️ EXEC SKIP: {error}");
        return ErrorDecision::Skip;
    }

    // Default: abort
    tracing::error!("🚨 EXEC ABORT (unhandled): {error}");
    ErrorDecision::Abort
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── BinanceErrorCode Tests ───────────────────────────────

    #[test]
    fn test_error_code_from_code() {
        assert_eq!(BinanceErrorCode::from_code(-1015), BinanceErrorCode::TooManyOrders);
        assert_eq!(BinanceErrorCode::from_code(-2019), BinanceErrorCode::MarginInsufficient);
        assert_eq!(BinanceErrorCode::from_code(-2022), BinanceErrorCode::ExceedMaxPosition);
        assert_eq!(BinanceErrorCode::from_code(-1021), BinanceErrorCode::TimestampNotSynced);
        assert_eq!(BinanceErrorCode::from_code(-9999), BinanceErrorCode::Other(-9999));
    }

    #[test]
    fn test_error_code_transient() {
        assert!(BinanceErrorCode::TooManyOrders.is_transient());
        assert!(BinanceErrorCode::Disconnected.is_transient());
        assert!(BinanceErrorCode::TimestampNotSynced.is_transient());
        assert!(!BinanceErrorCode::MarginInsufficient.is_transient());
    }

    #[test]
    fn test_error_code_permanent() {
        assert!(BinanceErrorCode::MarginInsufficient.is_permanent());
        assert!(BinanceErrorCode::InvalidSignature.is_permanent());
        assert!(!BinanceErrorCode::TooManyOrders.is_permanent());
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(
            format!("{}", BinanceErrorCode::TooManyOrders),
            "TOO_MANY_ORDERS(-1015)"
        );
    }

    // ── ExecutionError Construction ──────────────────────────

    #[test]
    fn test_from_binance_rate_limit() {
        let err = ExecutionError::from_binance_api("Binance API error: -1015 — Too many orders");
        assert!(matches!(err, ExecutionError::RateLimited { retry_after_secs: 10, .. }));
    }

    #[test]
    fn test_from_binance_margin() {
        let err = ExecutionError::from_binance_api("Binance API error: -2019 — Margin insufficient");
        assert!(matches!(err, ExecutionError::InsufficientMargin { .. }));
    }

    #[test]
    fn test_from_binance_timestamp() {
        let err = ExecutionError::from_binance_api("Binance API error: -1021 — Timestamp for this request was 1000ms ahead");
        assert!(matches!(err, ExecutionError::TimestampSync { .. }));
    }

    #[test]
    fn test_from_binance_unknown() {
        let err = ExecutionError::from_binance_api("Binance API error: -9999 — Something weird");
        assert!(matches!(err, ExecutionError::Unknown { .. }));
    }

    #[test]
    fn test_from_anyhow_binance() {
        let orig = anyhow::anyhow!("Binance API error: -1015 — Too many orders");
        let err = ExecutionError::from_anyhow(&orig);
        assert!(matches!(err, ExecutionError::RateLimited { .. }));
    }

    #[test]
    fn test_from_anyhow_kill_switch() {
        let orig = anyhow::anyhow!("kill switch is active");
        let err = ExecutionError::from_anyhow(&orig);
        assert!(matches!(err, ExecutionError::KillSwitched));
    }

    #[test]
    fn test_from_anyhow_network() {
        let orig = anyhow::anyhow!("connection timeout after 10s");
        let err = ExecutionError::from_anyhow(&orig);
        assert!(matches!(err, ExecutionError::NetworkError { .. }));
    }

    // ── Classification Tests ─────────────────────────────────

    #[test]
    fn test_is_retryable() {
        assert!(ExecutionError::RateLimited {
            code: BinanceErrorCode::TooManyOrders,
            message: "test".into(),
            retry_after_secs: 5,
        }.is_retryable());

        assert!(ExecutionError::NetworkError {
            message: "timeout".into(),
            retry_count: 0,
        }.is_retryable());

        assert!(!ExecutionError::KillSwitched.is_retryable());
        assert!(!ExecutionError::InsufficientMargin {
            required: Decimal::ONE,
            available: Decimal::ZERO,
        }.is_retryable());
    }

    #[test]
    fn test_should_abort() {
        assert!(ExecutionError::KillSwitched.should_abort());
        assert!(ExecutionError::InsufficientMargin {
            required: Decimal::from(1000),
            available: Decimal::from(100),
        }.should_abort());
        assert!(ExecutionError::RiskCheckFailed {
            reason: "daily loss limit".into(),
        }.should_abort());

        assert!(!ExecutionError::NetworkError {
            message: "timeout".into(),
            retry_count: 0,
        }.should_abort());
    }

    #[test]
    fn test_should_skip() {
        assert!(ExecutionError::SlippageExceeded {
            estimated_bps: 10.0,
            max_bps: 5.0,
        }.should_skip());

        assert!(ExecutionError::Rejected {
            code: BinanceErrorCode::NewOrderRejected,
            reason: "bad price".into(),
        }.should_skip());

        assert!(!ExecutionError::KillSwitched.should_skip());
    }

    // ── Retry Delay Tests ────────────────────────────────────

    #[test]
    fn test_retry_delay_rate_limited() {
        let err = ExecutionError::RateLimited {
            code: BinanceErrorCode::TooManyOrders,
            message: "test".into(),
            retry_after_secs: 15,
        };
        assert_eq!(err.retry_delay(), Duration::from_secs(15));
    }

    #[test]
    fn test_retry_delay_network_exponential() {
        let err = ExecutionError::NetworkError {
            message: "timeout".into(),
            retry_count: 3,
        };
        let delay = err.retry_delay();
        assert_eq!(delay, Duration::from_millis(500) * 8); // 2^3 = 8
    }

    // ── Decision Tests ───────────────────────────────────────

    #[test]
    fn test_decide_abort_on_kill_switch() {
        let err = ExecutionError::KillSwitched;
        assert_eq!(decide(&err, 0, 3), ErrorDecision::Abort);
    }

    #[test]
    fn test_decide_abort_on_margin() {
        let err = ExecutionError::InsufficientMargin {
            required: Decimal::from(1000),
            available: Decimal::from(100),
        };
        assert_eq!(decide(&err, 0, 3), ErrorDecision::Abort);
    }

    #[test]
    fn test_decide_retry_on_rate_limit() {
        let err = ExecutionError::RateLimited {
            code: BinanceErrorCode::TooManyOrders,
            message: "test".into(),
            retry_after_secs: 5,
        };
        let decision = decide(&err, 0, 3);
        assert!(matches!(decision, ErrorDecision::Retry { delay_ms: 5000, .. }));
    }

    #[test]
    fn test_decide_abort_after_max_retries() {
        let err = ExecutionError::NetworkError {
            message: "timeout".into(),
            retry_count: 5,
        };
        assert_eq!(decide(&err, 3, 3), ErrorDecision::Abort);
    }

    #[test]
    fn test_decide_skip_on_slippage() {
        let err = ExecutionError::SlippageExceeded {
            estimated_bps: 15.0,
            max_bps: 5.0,
        };
        assert_eq!(decide(&err, 0, 3), ErrorDecision::Skip);
    }

    // ── Display Tests ────────────────────────────────────────

    #[test]
    fn test_display_rate_limited() {
        let err = ExecutionError::RateLimited {
            code: BinanceErrorCode::TooManyOrders,
            message: "Too many orders".into(),
            retry_after_secs: 10,
        };
        let s = format!("{err}");
        assert!(s.contains("RATE_LIMITED"));
        assert!(s.contains("10s"));
    }

    #[test]
    fn test_display_partial_fill() {
        let err = ExecutionError::PartialFill {
            filled_qty: Decimal::from(5),
            remaining_qty: Decimal::from(3),
            fill_price: Decimal::from(100),
            commission: Decimal::ZERO,
            reason: "IOC expired".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("PARTIAL_FILL"));
        assert!(s.contains("5"));
        assert!(s.contains("3"));
    }

    // ── Serialization Tests ──────────────────────────────────

    #[test]
    fn test_execution_error_serialization() {
        let err = ExecutionError::RateLimited {
            code: BinanceErrorCode::TooManyOrders,
            message: "test".into(),
            retry_after_secs: 10,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ExecutionError = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ExecutionError::RateLimited { .. }));
    }

    #[test]
    fn test_error_code_serialization() {
        let json = serde_json::to_string(&BinanceErrorCode::TooManyOrders).unwrap();
        let back: BinanceErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BinanceErrorCode::TooManyOrders);
    }

    #[test]
    fn test_partial_fill_strategy_serialization() {
        for strategy in [
            PartialFillStrategy::MarketRest,
            PartialFillStrategy::LimitRest,
            PartialFillStrategy::AcceptAndMove,
            PartialFillStrategy::RetryFull,
        ] {
            let json = serde_json::to_string(&strategy).unwrap();
            let back: PartialFillStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(back, strategy);
        }
    }
}
