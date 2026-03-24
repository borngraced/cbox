use std::process::Command;

use crate::error::ContainerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerRuntime {
    Docker,
    Podman,
}

impl ContainerRuntime {
    /// Detect available container runtime. Prefers podman (rootless-friendly).
    pub fn detect() -> Result<Self, ContainerError> {
        if Self::is_available("podman") {
            Ok(Self::Podman)
        } else if Self::is_available("docker") {
            Ok(Self::Docker)
        } else {
            Err(ContainerError::NoRuntime)
        }
    }

    fn is_available(cmd: &str) -> bool {
        Command::new(cmd)
            .arg("version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn command_name(&self) -> &str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }
}

impl std::fmt::Display for ContainerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.command_name())
    }
}
