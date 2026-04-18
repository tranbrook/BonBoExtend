use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegimeError {
    #[error("Insufficient data for regime detection: need {0} points, got {1}")]
    InsufficientData(usize, usize),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Detection error: {0}")]
    Detection(String),
}
