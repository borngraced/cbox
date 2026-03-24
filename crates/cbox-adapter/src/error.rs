use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("adapter not found: {0}")]
    NotFound(String),

    #[error("adapter validation failed: {0}")]
    Validation(String),

    #[error("adapter error: {0}")]
    Runtime(String),
}
