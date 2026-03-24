use std::collections::HashMap;

use crate::session::Session;

/// Result of running a sandbox backend.
pub struct BackendResult {
    pub exit_code: i32,
    pub session: Session,
}

/// Errors from sandbox backends.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("{0}")]
    Backend(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Core(#[from] crate::error::CoreError),
}

/// Which backend was used for a session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    #[default]
    Native,
    Container,
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::Container => write!(f, "container"),
        }
    }
}

impl std::str::FromStr for BackendKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "native" => Ok(Self::Native),
            "container" | "docker" | "podman" => Ok(Self::Container),
            _ => Err(format!("unknown backend: {}", s)),
        }
    }
}

/// Trait for sandbox execution backends.
///
/// A backend takes a Session + Config and runs a command in an isolated
/// environment, returning the exit code and updated session.
pub trait SandboxBackend {
    /// Execute a command in the sandbox.
    fn run(
        self,
        command: &[String],
        env: HashMap<String, String>,
        dry_run: bool,
    ) -> Result<BackendResult, BackendError>;

    /// Return the backend kind for session metadata.
    fn kind(&self) -> BackendKind;
}
