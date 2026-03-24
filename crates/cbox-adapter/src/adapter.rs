use std::collections::HashMap;
use std::process::ExitStatus;

use cbox_core::{CboxConfig, Session};

use crate::error::AdapterError;

/// Command to execute inside the sandbox.
#[derive(Debug, Clone)]
pub struct SandboxCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<String>,
}

/// Trait for agent adapters that customize sandbox behavior for specific tools.
pub trait AgentAdapter: Send + Sync {
    /// Adapter name.
    fn name(&self) -> &str;

    /// Validate that the adapter can work with this config.
    fn validate(&self, config: &CboxConfig) -> Result<(), AdapterError>;

    /// Prepare environment variables for the sandbox.
    fn prepare_env(
        &self,
        env: &mut HashMap<String, String>,
        config: &CboxConfig,
    ) -> Result<(), AdapterError>;

    /// Build the command to execute inside the sandbox.
    fn build_command(
        &self,
        user_command: &[String],
        config: &CboxConfig,
    ) -> Result<SandboxCommand, AdapterError>;

    /// Additional directories to bind-mount read-only into the sandbox.
    /// Adapters can use this to expose tool-specific config directories.
    fn extra_ro_mounts(&self) -> Vec<String> {
        vec![]
    }

    /// Additional paths to bind-mount read-write into the sandbox.
    fn extra_rw_mounts(&self) -> Vec<String> {
        vec![]
    }

    /// Called after the agent process exits.
    fn post_run(&self, _exit_status: ExitStatus, _session: &Session) -> Result<(), AdapterError> {
        Ok(())
    }
}
