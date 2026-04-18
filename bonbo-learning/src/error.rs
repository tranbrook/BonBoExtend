use thiserror::Error;

#[derive(Error, Debug)]
pub enum LearningError {
    #[error("Invalid weights: sum = {0}, expected 1.0")]
    InvalidWeightsSum(f64),

    #[error("Not enough data for learning: {0} outcomes, need {1}")]
    InsufficientData(usize, usize),

    #[error("Learning error: {0}")]
    Learning(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
