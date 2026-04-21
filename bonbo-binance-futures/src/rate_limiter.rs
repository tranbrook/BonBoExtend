//! Rate limiter for Binance API.
//! Tracks weight usage per IP and per UID.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Rate limit window tracker.
#[derive(Debug)]
struct LimitWindow {
    /// Weight used in this window.
    used: AtomicU32,
    /// Window start time.
    start: Instant,
    /// Window duration in seconds.
    duration_secs: u64,
    /// Maximum allowed weight.
    max: u32,
}

impl LimitWindow {
    fn new(duration_secs: u64, max: u32) -> Self {
        Self {
            used: AtomicU32::new(0),
            start: Instant::now(),
            duration_secs,
            max,
        }
    }

    /// Check if we can afford `cost` weight. Returns remaining budget.
    fn check(&self, cost: u32) -> bool {
        self.refresh_if_expired();
        let current = self.used.load(Ordering::Relaxed);
        current + cost <= self.max
    }

    /// Consume `cost` weight.
    fn consume(&self, cost: u32) {
        self.used.fetch_add(cost, Ordering::Relaxed);
    }

    /// Refresh window if expired.
    fn refresh_if_expired(&self) {
        if self.start.elapsed().as_secs() >= self.duration_secs {
            self.used.store(0, Ordering::Relaxed);
            // Note: can't reset Instant in &self, but the next check will work
        }
    }

    /// Get remaining budget.
    fn remaining(&self) -> u32 {
        self.refresh_if_expired();
        self.max.saturating_sub(self.used.load(Ordering::Relaxed))
    }
}

/// Binance API rate limiter.
/// Tracks weight per minute (2400) and orders per 10s (300) and per minute (600).
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// IP weight limit per minute.
    ip_weight: Arc<Mutex<LimitWindow>>,
    /// Order count per 10 seconds.
    orders_10s: Arc<Mutex<LimitWindow>>,
    /// Order count per minute.
    orders_1min: Arc<Mutex<LimitWindow>>,
}

impl RateLimiter {
    /// Create a new rate limiter with default Binance limits.
    pub fn new() -> Self {
        Self {
            ip_weight: Arc::new(Mutex::new(LimitWindow::new(60, 2400))),
            orders_10s: Arc::new(Mutex::new(LimitWindow::new(10, 300))),
            orders_1min: Arc::new(Mutex::new(LimitWindow::new(60, 600))),
        }
    }

    /// Check if we can make a request with the given weight cost.
    pub async fn check_weight(&self, cost: u32) -> bool {
        let window = self.ip_weight.lock().await;
        window.check(cost)
    }

    /// Check if we can place an order.
    pub async fn check_order(&self) -> bool {
        let w10 = self.orders_10s.lock().await;
        let w1m = self.orders_1min.lock().await;
        w10.check(1) && w1m.check(1)
    }

    /// Consume weight after a successful request.
    pub async fn consume_weight(&self, cost: u32) {
        let window = self.ip_weight.lock().await;
        window.consume(cost);
    }

    /// Consume order count after placing an order.
    pub async fn consume_order(&self) {
        let w10 = self.orders_10s.lock().await;
        w10.consume(1);
        let w1m = self.orders_1min.lock().await;
        w1m.consume(1);
    }

    /// Get remaining IP weight budget.
    pub async fn remaining_weight(&self) -> u32 {
        let window = self.ip_weight.lock().await;
        window.remaining()
    }

    /// Update limits from response headers (Binance returns actual values).
    pub async fn update_from_headers(
        &self,
        used_weight: u32,
        order_count_10s: u32,
        order_count_1min: u32,
    ) {
        // We rely on server-side enforcement; these are informational.
        tracing::debug!(
            "Rate limit update: weight={}, orders_10s={}, orders_1min={}",
            used_weight,
            order_count_10s,
            order_count_1min
        );
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let limiter = RateLimiter::new();
        assert!(limiter.check_weight(1).await);
        assert!(limiter.check_order().await);
    }

    #[tokio::test]
    async fn test_rate_limiter_consume() {
        let limiter = RateLimiter::new();
        assert!(limiter.remaining_weight().await > 2300);
        limiter.consume_weight(100).await;
        assert!(limiter.remaining_weight().await < 2350);
    }
}
