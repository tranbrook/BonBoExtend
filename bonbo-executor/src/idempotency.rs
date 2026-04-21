//! Idempotency tracking — prevents duplicate order submissions.

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Tracks used order IDs to prevent duplicate submissions.
#[derive(Debug, Clone)]
pub struct IdempotencyTracker {
    used_ids: Arc<RwLock<HashSet<String>>>,
    max_size: usize,
}

impl IdempotencyTracker {
    /// Create a new tracker with max retained IDs.
    pub fn new(max_size: usize) -> Self {
        Self {
            used_ids: Arc::new(RwLock::new(HashSet::new())),
            max_size,
        }
    }

    /// Check if an ID has been used. If not, claim it.
    /// Returns true if the ID was successfully claimed (first use).
    pub async fn claim(&self, id: &str) -> bool {
        let mut used = self.used_ids.write().await;
        if used.contains(id) {
            return false;
        }
        // Evict oldest if at capacity (simple approach)
        if used.len() >= self.max_size {
            let to_remove = used.iter().next().cloned();
            if let Some(old) = to_remove {
                used.remove(&old);
            }
        }
        used.insert(id.to_string());
        true
    }

    /// Check if an ID has been used.
    pub async fn contains(&self, id: &str) -> bool {
        self.used_ids.read().await.contains(id)
    }

    /// Get count of tracked IDs.
    pub async fn len(&self) -> usize {
        self.used_ids.read().await.len()
    }
}

impl Default for IdempotencyTracker {
    fn default() -> Self {
        Self::new(10000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_claim_first_use() {
        let tracker = IdempotencyTracker::new(100);
        assert!(tracker.claim("order-1").await);
        assert!(!tracker.claim("order-1").await); // duplicate
        assert!(tracker.claim("order-2").await); // new
    }

    #[tokio::test]
    async fn test_eviction() {
        let tracker = IdempotencyTracker::new(2);
        tracker.claim("a").await;
        tracker.claim("b").await;
        tracker.claim("c").await; // should evict "a"
        assert_eq!(tracker.len().await, 2);
    }
}
