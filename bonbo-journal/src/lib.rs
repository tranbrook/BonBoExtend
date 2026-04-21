//! BonBo Trade Journal — Record, Track, Learn
//!
//! Persistent trade journal with SQLite backend.
//! Records every analysis snapshot and trade outcome for self-learning.

pub mod error;
pub mod journal;
pub mod models;
pub mod performance;

pub use error::JournalError;
pub use journal::JournalStore;
pub use models::*;
pub use performance::PerformanceTracker;
