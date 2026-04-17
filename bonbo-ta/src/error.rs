//! Error types for bonbo-ta.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TaError {
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Insufficient data: need {required} points, have {available}")]
    InsufficientData { required: usize, available: usize },

    #[error("Division by zero in indicator computation: {indicator}")]
    DivisionByZero { indicator: String },

    #[error("NaN detected in {indicator} at step {step}")]
    NaNDetected { indicator: String, step: usize },
}

pub type TaResult<T> = Result<T, TaError>;
