//! BonBo Validation — CPCV, DSR, PBO for strategy validation.

pub mod error;
pub mod models;
pub mod cpcv;
pub mod walk_forward;
pub mod report;

pub use error::ValidationError;
pub use models::*;
pub use report::ValidationReport;
