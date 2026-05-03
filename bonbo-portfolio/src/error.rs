//! Portfolio analysis errors.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PortfolioError {
    #[error("Insufficient data: {0} returns, need {1}")]
    InsufficientData(usize, usize),

    #[error("Invalid position: {0}")]
    InvalidPosition(String),

    #[error("Computation error: {0}")]
    Computation(String),
}
