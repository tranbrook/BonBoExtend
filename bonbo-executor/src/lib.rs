//! BonBo Extend Execution Module
//!
//! Production-grade order execution with market impact minimization:
//!
//! # Architecture (4 layers)
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │  Layer 4: Execution Router (pre-trade analysis)         │
//! │  ─ select_algorithm(), estimate cost, risk guard         │
//! ├─────────────────────────────────────────────────────────┤
//! │  Layer 3: TWAP / AdaptiveLimit / Iceberg                 │
//! │  ─ async state machines, slice scheduling               │
//! ├─────────────────────────────────────────────────────────┤
//! │  Layer 2: OrderBook Analytics (L2 depth → slippage)     │
//! │  ─ available liquidity, expected impact, spread analysis│
//! ├─────────────────────────────────────────────────────────┤
//! │  Layer 1: Risk Guards (kill switch, max loss)            │
//! │  ─ per-slice limits, total limits, time limits          │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Quick Start
//!
//! ```ignore
//! use bonbo_executor::execution_algo::{select_execution_algo, execute_twap, TwapConfig};
//! use bonbo_executor::risk_guards::{CumulativeRiskState, ExecutionRiskLimits};
//!
//! // 1. Select optimal algorithm
//! let sel = select_execution_algo(500.0, 120.0, 50_000_000.0, 2.0);
//!
//! // 2. Execute with TWAP
//! let report = execute_twap(&placer, "SEIUSDT", Side::Buy, qty, &config, &risk, &limits).await?;
//! println!("Grade: {} | IS: {:.1} bps", report.grade, report.is_bps);
//! ```

pub mod async_dispatcher;
pub mod dry_run;
pub mod execution_algo;
pub mod execution_errors;
pub mod flash_limit;
pub mod idempotency;
pub mod is;
pub mod market_impact;
pub mod ofi;
pub mod optimal_slicer;
pub mod order_builder;
pub mod orderbook;
pub mod pov;
pub mod risk_guards;
pub mod saga;
pub mod smart_execution;
pub mod smart_market;
pub mod twap;
pub mod utils;
pub mod vwap;

pub use async_dispatcher::{
    AsyncOrderDispatcher, ConcurrentBatchResult, ConcurrentSliceConfig,
    ConcurrentSliceExecutor, DispatchedResult, OrderRateGate, OrderTask,
};
pub use dry_run::DryRunExecutor;
pub use execution_algo::{
    AdaptiveLimitConfig, AlgoSelection, ExecutionReport, FillResult, OrderPlacer,
    execute_adaptive_limit, select_execution_algo,
};
pub use execution_errors::{
    BinanceErrorCode, ErrorDecision, ExecutionError, PartialFillResult, PartialFillStrategy,
    decide, handle_partial_fill,
};
pub use flash_limit::{
    execute_flash_limit, analyze_spread, FlashLimitConfig, FlashLimitResult,
    OrderRoute, SpreadAnalysis, SpreadTracker,
};
pub use idempotency::IdempotencyTracker;
pub use is::{
    IsConfig, IsDecomposition, IsReport, IsSliceRecord, OptimalTrajectory,
    execute_is,
};
pub use market_impact::{
    CascadeDetection, ImpactEstimate, ImpactParams, SlippageAtRisk, TransientImpactState,
    compute_slippage_at_risk, estimate_impact,
};
pub use ofi::{
    OfiConfig, OfiReport, OfiScore, OfiSignal, OfiSliceRecord, OfiTracker,
    execute_ofi,
};
pub use order_builder::OrderBuilder;
pub use orderbook::{
    OrderBookSnapshot, PriceLevel, Side as ExecutionSide, SlippageEstimate,
};
pub use pov::{
    AggTrade, PovConfig, PovReport, PovSliceRecord, TradeFetcher, VolumeWindow,
    execute_pov,
};
pub use risk_guards::{
    CumulativeRiskState, ExecutionRiskLimits, PreTradeCheck, RiskCheckResult,
    activate_kill_switch, deactivate_kill_switch, is_kill_switch_active,
};
pub use saga::{SagaExecutor, SagaResult};
pub use smart_execution::{ExecutionAlgo, ExecutionParams, select_optimal_algo};
pub use smart_market::{
    BookState, SmartMarketConfig, SmartMarketPhaseRecord, SmartMarketReport,
    execute_smart_market,
};
pub use optimal_slicer::{
    OptimalSliceConfig, OptimalSliceResult, OptimalSlicer, SliceAdjustment,
    SliceTransientState,
};
pub use twap::{
    SliceRecord, SliceStatus, TwapConfig, TwapReport, SimpleRng,
    execute_twap,
};
pub use utils::{compute_jitter, decimal_to_f64};
pub use vwap::{
    KlineFetcher, VolumeBucket, VolumeProfile, VwapConfig, VwapReport, VwapSchedule, VwapSlice,
    VwapSliceRecord, execute_vwap,
};
