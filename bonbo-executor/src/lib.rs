//! BonBo Executor — Order execution with Saga pattern.
//!
//! Handles the 3-order placement pattern (Entry + SL + TP)
//! with compensating actions on failure.

pub mod dry_run;
pub mod idempotency;
pub mod order_builder;
pub mod saga;

pub use dry_run::DryRunExecutor;
pub use idempotency::IdempotencyTracker;
pub use order_builder::OrderBuilder;
pub use saga::{SagaExecutor, SagaResult};
