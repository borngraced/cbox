use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContainerError {
    #[error("no container runtime found (install docker or podman)")]
    NoRuntime,

    #[error("container runtime error: {0}")]
    Runtime(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ContainerError> for cbox_core::BackendError {
    fn from(e: ContainerError) -> Self {
        cbox_core::BackendError::Backend(e.to_string())
    }
}
