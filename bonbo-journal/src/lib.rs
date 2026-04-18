//! BonBo Trade Journal — Record, Track, Learn
//!
//! Persistent trade journal with SQLite backend.
//! Records every analysis snapshot and trade outcome for self-learning.

pub mod error;
pub mod models;
pub mod journal;
pub mod performance;

pub use error::JournalError;
pub use models::*;
pub use journal::JournalStore;
pub use performance::PerformanceTracker;
