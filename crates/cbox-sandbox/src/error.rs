use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("namespace setup failed: {0}")]
    Namespace(String),

    #[error("mount failed: {0}")]
    Mount(String),

    #[error("seccomp setup failed: {0}")]
    Seccomp(String),

    #[error("cgroup setup failed: {0}")]
    Cgroup(String),

    #[error("process error: {0}")]
    Process(String),

    #[error("overlay error: {0}")]
    Overlay(#[from] cbox_overlay::OverlayError),

    #[error("network error: {0}")]
    Network(#[from] cbox_network::NetworkError),

    #[error("core error: {0}")]
    Core(#[from] cbox_core::CoreError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("nix error: {0}")]
    Nix(#[from] nix::Error),
}
