use thiserror::Error;

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("failed to create overlay directories: {0}")]
    Setup(String),

    #[error("failed to mount overlayfs: {0}")]
    Mount(String),

    #[error("failed to unmount overlayfs: {0}")]
    Unmount(String),

    #[error("failed to compute diff: {0}")]
    Diff(String),

    #[error("failed to merge changes: {0}")]
    Merge(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
