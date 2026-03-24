use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("diff error: {0}")]
    Diff(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
