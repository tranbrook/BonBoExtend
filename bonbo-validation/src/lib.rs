//! BonBo Validation — CPCV, DSR, PBO for strategy validation.

pub mod cpcv;
pub mod error;
pub mod models;
pub mod report;
pub mod walk_forward;

pub use error::ValidationError;
pub use models::*;
pub use report::ValidationReport;
