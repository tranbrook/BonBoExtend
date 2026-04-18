use thiserror::Error;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Insufficient data: {0}")]
    InsufficientData(String),

    #[error("Validation error: {0}")]
    Validation(String),
}
