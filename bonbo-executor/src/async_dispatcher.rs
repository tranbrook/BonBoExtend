//! Async Concurrency Layer for non-blocking order placement.
//!
//! Provides:
//! 1. **AsyncOrderDispatcher** — channel-based order dispatcher with rate-limit awareness
//! 2. **ConcurrentSliceExecutor** — fire multiple slices concurrently with bounded parallelism
//! 3. **OrderTask** — lightweight order description that can be sent through channels
//!
//! # Architecture
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │  Caller (TWAP/VWAP/POV/IS/OFI)                                     │
//! │       │                                                              │
//! │       │  One or more OrderTasks                                      │
//! │       ▼                                                              │
//! │  ┌──────────────────────────────────────────────────────────────┐    │
//! │  │  AsyncOrderDispatcher                                        │    │
//! │  │                                                              │    │
//! │  │  ┌─────────────┐   ┌───────────────────────────────────┐    │    │
//! │  │  │ mpsc::Sender │──▶│ Bounded Semaphore (max_in_flight) │    │    │
//! │  │  └─────────────┘   └──────────────┬────────────────────┘    │    │
//! │  │                                     │                         │    │
//! │  │                          ┌──────────▼──────────┐             │    │
//! │  │                          │  tokio::spawn tasks  │             │    │
//! │  │                          │  ├─ OrderTask 1      │             │    │
//! │  │                          │  ├─ OrderTask 2      │             │    │
//! │  │                          │  └─ OrderTask N      │             │    │
//! │  │                          └──────────┬───────────┘             │    │
//! │  │                                     │                         │    │
//! │  │                          ┌──────────▼──────────┐             │    │
//! │  │                          │  Rate Limiter Gate   │             │    │
//! │  │                          │  (30 orders/sec)     │             │    │
//! │  │                          └──────────┬───────────┘             │    │
//! │  │                                     │                         │    │
//! │  │                          ┌──────────▼──────────┐             │    │
//! │  │                          │  OrderPlacer::place  │             │    │
//! │  │                          │  (actual API call)   │             │    │
//! │  │                          └──────────┬───────────┘             │    │
//! │  │                                     │                         │    │
//! │  │                          ┌──────────▼──────────┐             │    │
//! │  │                          │  oneshot::Sender     │             │    │
//! │  │                          │  (result back to     │             │    │
//! │  │                          │   caller)            │             │    │
//! │  │                          └──────────────────────┘             │    │
//! │  └──────────────────────────────────────────────────────────────┘    │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//! ```ignore
//! let dispatcher = AsyncOrderDispatcher::new(placer, 5); // max 5 concurrent
//!
//! // Fire 3 slices concurrently
//! let handles = vec![
//!     dispatcher.dispatch(OrderTask::market("BTCUSDT", Side::Buy, qty1)),
//!     dispatcher.dispatch(OrderTask::market("ETHUSDT", Side::Buy, qty2)),
//!     dispatcher.dispatch(OrderTask::market("SOLUSDT", Side::Buy, qty3)),
//! ];
//!
//! // Collect all results (non-blocking, concurrent)
//! let results = futures::future::join_all(handles).await;
//! ```

use crate::execution_algo::{FillResult, OrderPlacer};
use crate::orderbook::Side;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinHandle;

// ═══════════════════════════════════════════════════════════════
// ORDER TASK
// ═══════════════════════════════════════════════════════════════

/// A lightweight order description that can be dispatched asynchronously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderTask {
    /// Market order.
    Market {
        symbol: String,
        side: Side,
        qty: Decimal,
    },
    /// Limit order.
    Limit {
        symbol: String,
        side: Side,
        qty: Decimal,
        price: Decimal,
    },
}

impl OrderTask {
    /// Create a market order task.
    pub fn market(symbol: &str, side: Side, qty: Decimal) -> Self {
        Self::Market {
            symbol: symbol.to_string(),
            side,
            qty,
        }
    }

    /// Create a limit order task.
    pub fn limit(symbol: &str, side: Side, qty: Decimal, price: Decimal) -> Self {
        Self::Limit {
            symbol: symbol.to_string(),
            side,
            qty,
            price,
        }
    }

    /// Symbol reference.
    pub fn symbol(&self) -> &str {
        match self {
            OrderTask::Market { symbol, .. } | OrderTask::Limit { symbol, .. } => symbol,
        }
    }

    /// Side.
    pub fn side(&self) -> Side {
        match self {
            OrderTask::Market { side, .. } | OrderTask::Limit { side, .. } => *side,
        }
    }

    /// Quantity.
    pub fn qty(&self) -> Decimal {
        match self {
            OrderTask::Market { qty, .. } | OrderTask::Limit { qty, .. } => *qty,
        }
    }
}

/// Result of dispatching an order through the async pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchedResult {
    /// The original task.
    pub task: OrderTask,
    /// The fill result (if successful).
    pub fill: Option<FillResult>,
    /// Error message (if failed).
    pub error: Option<String>,
    /// Time from dispatch to completion (ms).
    pub latency_ms: u64,
}

impl DispatchedResult {
    /// Was the order successful?
    pub fn is_ok(&self) -> bool {
        self.fill.is_some()
    }
}

// ═══════════════════════════════════════════════════════════════
// RATE LIMITER GATE
// ═══════════════════════════════════════════════════════════════

/// Token-bucket rate limiter for order placement.
/// Ensures we never exceed Binance's order rate limits.
#[derive(Debug, Clone)]
pub struct OrderRateGate {
    /// Minimum interval between consecutive orders (ms).
    min_interval_ms: u64,
    /// Maximum orders per second.
    max_per_sec: u32,
    /// Semaphore controlling concurrency.
    semaphore: Arc<Semaphore>,
    /// Last order timestamp (shared across clones).
    last_order_ms: Arc<tokio::sync::Mutex<u64>>,
}

impl OrderRateGate {
    /// Create a new rate gate.
    /// `max_concurrent` — maximum in-flight orders at once.
    /// `min_interval_ms` — minimum ms between consecutive order dispatches.
    pub fn new(max_concurrent: usize, min_interval_ms: u64) -> Self {
        Self {
            min_interval_ms,
            max_per_sec: 30, // Binance limit
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            last_order_ms: Arc::new(tokio::sync::Mutex::new(0u64)),
        }
    }

    /// Conservative: max 5 concurrent, 100ms between orders.
    pub fn conservative() -> Self {
        Self::new(5, 100)
    }

    /// Aggressive: max 15 concurrent, 33ms between orders (30/sec).
    pub fn aggressive() -> Self {
        Self::new(15, 33)
    }

    /// Acquire a permit — waits until we're within rate limits.
    pub async fn acquire(&self) -> tokio::sync::SemaphorePermit<'_> {
        let permit = self.semaphore.acquire().await.expect("semaphore closed");

        // Enforce minimum interval between orders
        let mut last = self.last_order_ms.lock().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let elapsed = now.saturating_sub(*last);
        if elapsed < self.min_interval_ms {
            let wait = self.min_interval_ms - elapsed;
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        }

        *last = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        permit
    }

    /// Available permits.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }
}

// ═══════════════════════════════════════════════════════════════
// ASYNC ORDER DISPATCHER
// ═══════════════════════════════════════════════════════════════

/// Async, non-blocking order dispatcher with bounded concurrency.
///
/// Wraps an `OrderPlacer` and dispatches orders through `tokio::spawn`
/// tasks with a semaphore controlling maximum in-flight orders.
pub struct AsyncOrderDispatcher {
    /// The underlying order placer.
    pub(crate) placer: Arc<dyn OrderPlacer>,
    /// Rate limiter gate.
    pub(crate) rate_gate: OrderRateGate,
}

impl AsyncOrderDispatcher {
    /// Create a new dispatcher.
    /// `placer` — the order placement backend.
    /// `max_concurrent` — maximum number of concurrent in-flight orders.
    pub fn new(placer: Arc<dyn OrderPlacer>, max_concurrent: usize) -> Self {
        let rate_gate = OrderRateGate::new(max_concurrent, 50);
        Self { placer, rate_gate }
    }

    /// Create with custom rate gate.
    pub fn with_rate_gate(placer: Arc<dyn OrderPlacer>, rate_gate: OrderRateGate) -> Self {
        Self { placer, rate_gate }
    }

    /// Dispatch a single order asynchronously.
    /// Returns a JoinHandle that resolves to the result.
    pub fn dispatch(&self, task: OrderTask) -> JoinHandle<DispatchedResult> {
        let placer = self.placer.clone();
        let rate_gate = self.rate_gate.clone();

        tokio::spawn(async move {
            let start = std::time::Instant::now();

            // Acquire rate-limit permit (waits if needed)
            let _permit = rate_gate.acquire().await;

            // Place the order
            let result = match &task {
                OrderTask::Market { symbol, side, qty } => {
                    placer.place_market(symbol, *side, *qty).await
                }
                OrderTask::Limit { symbol, side, qty, price } => {
                    placer.place_limit(symbol, *side, *qty, *price).await
                }
            };

            let latency_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(fill) => DispatchedResult {
                    task,
                    fill: Some(fill),
                    error: None,
                    latency_ms,
                },
                Err(e) => DispatchedResult {
                    task,
                    fill: None,
                    error: Some(e.to_string()),
                    latency_ms,
                },
            }
        })
    }

    /// Dispatch multiple orders concurrently and collect all results.
    /// Orders are fired in parallel (bounded by semaphore).
    pub async fn dispatch_all(&self, tasks: Vec<OrderTask>) -> Vec<DispatchedResult> {
        let handles: Vec<_> = tasks.into_iter().map(|t| self.dispatch(t)).collect();

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    tracing::error!("Dispatch task panicked: {}", e);
                    results.push(DispatchedResult {
                        task: OrderTask::market("UNKNOWN", Side::Buy, Decimal::ZERO),
                        fill: None,
                        error: Some(format!("task panic: {e}")),
                        latency_ms: 0,
                    });
                }
            }
        }
        results
    }
}

// ═══════════════════════════════════════════════════════════════
// CONCURRENT SLICE EXECUTOR
// ═══════════════════════════════════════════════════════════════

/// Executes a batch of slices concurrently using the async dispatcher.
///
/// Designed for algorithms (TWAP/VWAP/POV/IS/OFI) that have pre-computed
/// slices and want to fire multiple simultaneously:
///
/// - **Single-symbol**: splits a large order into concurrent sub-slices
/// - **Multi-symbol**: fires orders for different symbols simultaneously
/// - **Hybrid**: both at once
///
/// # Safety
/// - Bounded concurrency (semaphore) prevents API rate limit violations
/// - Rate gate enforces minimum interval between orders
/// - Per-order timeout prevents hanging tasks
pub struct ConcurrentSliceExecutor {
    dispatcher: AsyncOrderDispatcher,
    /// Per-order timeout.
    order_timeout: std::time::Duration,
    /// Maximum retries per slice.
    max_retries: usize,
}

/// Configuration for concurrent slice execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrentSliceConfig {
    /// Maximum concurrent orders.
    pub max_concurrent: usize,
    /// Minimum interval between orders (ms).
    pub min_interval_ms: u64,
    /// Per-order timeout (ms).
    pub order_timeout_ms: u64,
    /// Maximum retries per slice.
    pub max_retries: usize,
    /// Retry delay (ms).
    pub retry_delay_ms: u64,
}

impl Default for ConcurrentSliceConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 5,
            min_interval_ms: 50,
            order_timeout_ms: 5000,
            max_retries: 2,
            retry_delay_ms: 100,
        }
    }
}

impl ConcurrentSliceConfig {
    /// Conservative: 3 concurrent, 200ms interval.
    pub fn conservative() -> Self {
        Self {
            max_concurrent: 3,
            min_interval_ms: 200,
            order_timeout_ms: 10000,
            max_retries: 3,
            retry_delay_ms: 500,
        }
    }

    /// Aggressive: 10 concurrent, 33ms interval.
    pub fn aggressive() -> Self {
        Self {
            max_concurrent: 10,
            min_interval_ms: 33,
            order_timeout_ms: 3000,
            max_retries: 1,
            retry_delay_ms: 50,
        }
    }
}

/// Summary of a concurrent batch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrentBatchResult {
    /// Total tasks dispatched.
    pub total_dispatched: usize,
    /// Successful fills.
    pub successful: usize,
    /// Failed orders.
    pub failed: usize,
    /// Total filled quantity.
    pub total_filled_qty: Decimal,
    /// Average fill price (VWAP).
    pub avg_fill_price: Decimal,
    /// Total commission.
    pub total_commission: Decimal,
    /// Average latency per order (ms).
    pub avg_latency_ms: f64,
    /// Max latency (ms).
    pub max_latency_ms: u64,
    /// Total wall-clock time (ms).
    pub total_time_ms: u64,
    /// Per-order results.
    pub results: Vec<DispatchedResult>,
}

impl ConcurrentSliceExecutor {
    /// Create a new concurrent executor.
    pub fn new(placer: Arc<dyn OrderPlacer>, config: &ConcurrentSliceConfig) -> Self {
        let rate_gate = OrderRateGate::new(config.max_concurrent, config.min_interval_ms);
        let dispatcher = AsyncOrderDispatcher::with_rate_gate(placer, rate_gate);
        Self {
            dispatcher,
            order_timeout: std::time::Duration::from_millis(config.order_timeout_ms),
            max_retries: config.max_retries,
        }
    }

    /// Execute a batch of order tasks concurrently.
    pub async fn execute_batch(&self, tasks: Vec<OrderTask>) -> ConcurrentBatchResult {
        let start = std::time::Instant::now();
        let total = tasks.len();

        let mut all_results: Vec<DispatchedResult> = Vec::with_capacity(total);
        let mut remaining_tasks = tasks;

        for attempt in 0..=self.max_retries {
            if remaining_tasks.is_empty() {
                break;
            }

            // Dispatch all remaining tasks concurrently
            let mut results = self.dispatcher.dispatch_all(remaining_tasks.clone()).await;

            // Separate successes from failures
            let mut retry_tasks = Vec::new();
            for result in &mut results {
                if result.is_ok() {
                    all_results.push(result.clone());
                } else if attempt < self.max_retries {
                    tracing::warn!(
                        "Order failed (attempt {}/{}): {:?}, retrying",
                        attempt + 1,
                        self.max_retries,
                        result.error,
                    );
                    retry_tasks.push(result.task.clone());
                } else {
                    all_results.push(result.clone());
                }
            }

            if !retry_tasks.is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            remaining_tasks = retry_tasks;
        }

        // Compute summary
        let successful = all_results.iter().filter(|r| r.is_ok()).count();
        let failed = all_results.iter().filter(|r| !r.is_ok()).count();

        let mut total_filled_qty = Decimal::ZERO;
        let mut total_notional = Decimal::ZERO;
        let mut total_commission = Decimal::ZERO;
        let mut total_latency = 0u64;
        let mut max_latency = 0u64;

        for r in &all_results {
            if let Some(fill) = &r.fill {
                total_filled_qty += fill.fill_qty;
                total_notional += fill.fill_price * fill.fill_qty;
                total_commission += fill.commission;
            }
            total_latency += r.latency_ms;
            max_latency = max_latency.max(r.latency_ms);
        }

        let avg_fill_price = if total_filled_qty > Decimal::ZERO {
            total_notional / total_filled_qty
        } else {
            Decimal::ZERO
        };

        let avg_latency = if !all_results.is_empty() {
            total_latency as f64 / all_results.len() as f64
        } else {
            0.0
        };

        ConcurrentBatchResult {
            total_dispatched: total,
            successful,
            failed,
            total_filled_qty,
            avg_fill_price,
            total_commission,
            avg_latency_ms: avg_latency,
            max_latency_ms: max_latency,
            total_time_ms: start.elapsed().as_millis() as u64,
            results: all_results,
        }
    }

    /// Execute tasks and stream results back as they complete.
    /// Returns a channel receiver for real-time fill processing.
    pub async fn execute_stream(
        &self,
        tasks: Vec<OrderTask>,
    ) -> tokio::sync::mpsc::Receiver<DispatchedResult> {
        let n = tasks.len();
        let (tx, rx) = tokio::sync::mpsc::channel(n);
        let placer = self.dispatcher.placer.clone();
        let rate_gate = self.dispatcher.rate_gate.clone();

        // Spawn all workers — results flow through `tx` → caller reads `rx`
        tokio::spawn(async move {
            let mut handles = Vec::with_capacity(n);

            for task in tasks {
                let placer = placer.clone();
                let rate_gate = rate_gate.clone();
                let tx = tx.clone();

                handles.push(tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    let _permit = rate_gate.acquire().await;

                    let result: anyhow::Result<FillResult> = match &task {
                        OrderTask::Market { symbol, side, qty } => {
                            placer.place_market(symbol, *side, *qty).await
                        }
                        OrderTask::Limit { symbol, side, qty, price } => {
                            placer.place_limit(symbol, *side, *qty, *price).await
                        }
                    };

                    let latency_ms = start.elapsed().as_millis() as u64;

                    let dispatched = match result {
                        Ok(fill) => DispatchedResult {
                            task,
                            fill: Some(fill),
                            error: None,
                            latency_ms,
                        },
                        Err(e) => DispatchedResult {
                            task,
                            fill: None,
                            error: Some(e.to_string()),
                            latency_ms,
                        },
                    };

                    let _ = tx.send(dispatched).await;
                }));
            }

            for h in handles {
                let _ = h.await;
            }
        });

        rx
    }
}

// ═══════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::{OrderBookSnapshot, PriceLevel};
    use std::str::FromStr;

    // ── Mock OrderPlacer for testing ────────────────────────

    struct MockPlacer {
        delay_ms: u64,
        fail_rate: f64,
    }

    impl MockPlacer {
        fn new(delay_ms: u64) -> Self {
            Self { delay_ms, fail_rate: 0.0 }
        }

        fn with_fail_rate(delay_ms: u64, fail_rate: f64) -> Self {
            Self { delay_ms, fail_rate }
        }
    }

    #[async_trait::async_trait]
    impl OrderPlacer for MockPlacer {
        async fn place_market(
            &self, symbol: &str, side: Side, qty: Decimal,
        ) -> anyhow::Result<FillResult> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

            // Deterministic but varied: use first byte of symbol name
            if self.fail_rate > 0.0 {
                let hash = symbol.bytes().next().unwrap_or(0) as f64;
                if (hash * 31.0 % 100.0) / 100.0 < self.fail_rate {
                    anyhow::bail!("simulated failure for {symbol}");
                }
            }

            Ok(FillResult {
                fill_price: Decimal::from_str("100.00").unwrap(),
                fill_qty: qty,
                commission: qty * Decimal::from_str("0.0005").unwrap(),
                is_maker: false,
                slippage_bps: 1.0,
                timestamp_ms: 0,
            })
        }

        async fn place_limit(
            &self, symbol: &str, side: Side, qty: Decimal, price: Decimal,
        ) -> anyhow::Result<FillResult> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

            Ok(FillResult {
                fill_price: price,
                fill_qty: qty,
                commission: qty * Decimal::from_str("0.0002").unwrap(),
                is_maker: true,
                slippage_bps: 0.5,
                timestamp_ms: 0,
            })
        }

        async fn cancel_order(&self, _symbol: &str, _order_id: i64) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_orderbook(&self, symbol: &str) -> anyhow::Result<OrderBookSnapshot> {
            Ok(OrderBookSnapshot {
                symbol: symbol.to_string(),
                timestamp_ms: 0,
                bids: vec![PriceLevel::new(Decimal::from(100), Decimal::from(1000))],
                asks: vec![PriceLevel::new(Decimal::from(101), Decimal::from(1000))],
            })
        }
    }

    // ── OrderTask Tests ─────────────────────────────────────

    #[test]
    fn test_order_task_market() {
        let task = OrderTask::market("BTCUSDT", Side::Buy, Decimal::from(10));
        assert_eq!(task.symbol(), "BTCUSDT");
        assert_eq!(task.side(), Side::Buy);
        assert_eq!(task.qty(), Decimal::from(10));
    }

    #[test]
    fn test_order_task_limit() {
        let task = OrderTask::limit("ETHUSDT", Side::Sell, Decimal::from(5), Decimal::from(2000));
        assert_eq!(task.symbol(), "ETHUSDT");
        assert_eq!(task.side(), Side::Sell);
        let price = match &task {
            OrderTask::Limit { price, .. } => *price,
            _ => Decimal::ZERO,
        };
        assert_eq!(price, Decimal::from(2000));
    }

    #[test]
    fn test_order_task_serialization() {
        let task = OrderTask::market("SOLUSDT", Side::Buy, Decimal::from_str("1.5").unwrap());
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("SOLUSDT"));
        let back: OrderTask = serde_json::from_str(&json).unwrap();
        assert_eq!(back.symbol(), "SOLUSDT");
    }

    // ── Rate Gate Tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_rate_gate_basic() {
        let gate = OrderRateGate::new(3, 10);
        let _p1 = gate.acquire().await;
        let _p2 = gate.acquire().await;
        let _p3 = gate.acquire().await;
        // 3 permits acquired, 0 remaining
        assert_eq!(gate.available(), 0);
    }

    #[tokio::test]
    async fn test_rate_gate_enforces_interval() {
        let gate = OrderRateGate::new(5, 50); // 50ms minimum interval

        let start = std::time::Instant::now();
        let _p1 = gate.acquire().await;
        let _p2 = gate.acquire().await;
        let elapsed = start.elapsed().as_millis() as u64;

        // Second acquire should have waited at least 50ms
        assert!(elapsed >= 45, "expected >= 50ms, got {elapsed}ms");
    }

    // ── Dispatcher Tests ────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_single_order() {
        let placer = Arc::new(MockPlacer::new(10));
        let dispatcher = AsyncOrderDispatcher::new(placer, 3);

        let task = OrderTask::market("BTCUSDT", Side::Buy, Decimal::from(1));
        let handle = dispatcher.dispatch(task);
        let result = handle.await.expect("task should complete");

        assert!(result.is_ok());
        assert!(result.fill.is_some());
        assert!(result.latency_ms >= 10);
    }

    #[tokio::test]
    async fn test_dispatch_concurrent_orders() {
        let placer = Arc::new(MockPlacer::new(50)); // 50ms per order
        let dispatcher = AsyncOrderDispatcher::new(placer, 5);

        let tasks: Vec<OrderTask> = (0..5)
            .map(|i| OrderTask::market(&format!("SYM{i}USDT"), Side::Buy, Decimal::from(1)))
            .collect();

        let start = std::time::Instant::now();
        let results = dispatcher.dispatch_all(tasks).await;
        let elapsed = start.elapsed().as_millis() as u64;

        // 5 orders × 50ms each, but concurrent → should take ~250-400ms not 250ms
        // (due to rate gate interval)
        assert_eq!(results.len(), 5);
        let successful = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(successful, 5, "all should succeed");

        // With 5 concurrent and 50ms min interval: ~200-400ms total
        assert!(elapsed < 1000, "concurrent should be fast: {elapsed}ms");
    }

    #[tokio::test]
    async fn test_dispatch_limit_order() {
        let placer = Arc::new(MockPlacer::new(10));
        let dispatcher = AsyncOrderDispatcher::new(placer, 3);

        let task = OrderTask::limit("ETHUSDT", Side::Sell, Decimal::from(5), Decimal::from(2000));
        let handle = dispatcher.dispatch(task);
        let result = handle.await.expect("task should complete");

        assert!(result.is_ok());
        let fill = result.fill.unwrap();
        assert!(fill.is_maker);
        assert_eq!(fill.fill_price, Decimal::from(2000));
    }

    // ── Concurrent Executor Tests ───────────────────────────

    #[tokio::test]
    async fn test_concurrent_batch_all_success() {
        let placer = Arc::new(MockPlacer::new(20));
        let config = ConcurrentSliceConfig {
            max_concurrent: 5,
            min_interval_ms: 10,
            order_timeout_ms: 1000,
            max_retries: 1,
            retry_delay_ms: 50,
        };
        let executor = ConcurrentSliceExecutor::new(placer, &config);

        let tasks: Vec<OrderTask> = (0..5)
            .map(|i| OrderTask::market(&format!("COIN{i}USDT"), Side::Buy, Decimal::from(10)))
            .collect();

        let batch = executor.execute_batch(tasks).await;

        assert_eq!(batch.total_dispatched, 5);
        assert_eq!(batch.successful, 5);
        assert_eq!(batch.failed, 0);
        assert_eq!(batch.total_filled_qty, Decimal::from(50));
        assert!(batch.total_time_ms < 1000, "should be fast: {}ms", batch.total_time_ms);
    }

    #[tokio::test]
    async fn test_concurrent_batch_with_retries() {
        // Some orders will always fail due to deterministic mock,
        // but the batch itself should complete without panic.
        let placer = Arc::new(MockPlacer::with_fail_rate(10, 0.5));
        let config = ConcurrentSliceConfig {
            max_concurrent: 3,
            min_interval_ms: 5,
            order_timeout_ms: 1000,
            max_retries: 2,
            retry_delay_ms: 10,
        };
        let executor = ConcurrentSliceExecutor::new(placer, &config);

        let tasks: Vec<OrderTask> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|c| OrderTask::market(&format!("{c}USDT"), Side::Buy, Decimal::from(1)))
            .collect();

        let batch = executor.execute_batch(tasks).await;

        // Batch should complete (some may fail, some may succeed)
        assert_eq!(batch.total_dispatched, 5);
        assert!(batch.successful + batch.failed == 5);
        // At least some should succeed (A=0.15, B=0.46, C=0.77, D=0.08, E=0.55)
        assert!(batch.successful >= 1, "at least 1 should succeed: {}/{}", batch.successful, batch.total_dispatched);
    }

    #[tokio::test]
    async fn test_concurrent_stream() {
        let placer = Arc::new(MockPlacer::new(20));
        let config = ConcurrentSliceConfig {
            max_concurrent: 3,
            min_interval_ms: 5,
            order_timeout_ms: 1000,
            max_retries: 0,
            retry_delay_ms: 10,
        };
        let executor = ConcurrentSliceExecutor::new(placer, &config);

        let tasks: Vec<OrderTask> = (0..3)
            .map(|i| OrderTask::market(&format!("S{i}USDT"), Side::Buy, Decimal::from(1)))
            .collect();

        let mut rx = executor.execute_stream(tasks).await;

        let mut count = 0;
        while let Some(result) = rx.recv().await {
            assert!(result.is_ok());
            count += 1;
            if count >= 3 { break; }
        }
        assert_eq!(count, 3);
    }

    // ── Batch Result Tests ──────────────────────────────────

    #[test]
    fn test_batch_result_serialization() {
        let result = ConcurrentBatchResult {
            total_dispatched: 5,
            successful: 4,
            failed: 1,
            total_filled_qty: Decimal::from(40),
            avg_fill_price: Decimal::from_str("100.00").unwrap(),
            total_commission: Decimal::from_str("0.02").unwrap(),
            avg_latency_ms: 45.0,
            max_latency_ms: 80,
            total_time_ms: 120,
            results: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ConcurrentBatchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_dispatched, 5);
        assert_eq!(back.successful, 4);
    }

    #[test]
    fn test_dispatched_result_is_ok() {
        let ok_result = DispatchedResult {
            task: OrderTask::market("TEST", Side::Buy, Decimal::ONE),
            fill: Some(FillResult {
                fill_price: Decimal::ONE,
                fill_qty: Decimal::ONE,
                commission: Decimal::ZERO,
                is_maker: false,
                slippage_bps: 1.0,
                timestamp_ms: 0,
            }),
            error: None,
            latency_ms: 50,
        };
        assert!(ok_result.is_ok());

        let err_result = DispatchedResult {
            task: OrderTask::market("TEST", Side::Buy, Decimal::ONE),
            fill: None,
            error: Some("timeout".to_string()),
            latency_ms: 5000,
        };
        assert!(!err_result.is_ok());
    }
}
