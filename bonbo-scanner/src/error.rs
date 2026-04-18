use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScannerError {
    #[error("Scan error: {0}")]
    Scan(String),

    #[error("Schedule error: {0}")]
    Schedule(String),
}
