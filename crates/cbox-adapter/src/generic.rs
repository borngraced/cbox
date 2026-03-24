use std::collections::HashMap;

use cbox_core::CboxConfig;

use crate::adapter::{AgentAdapter, SandboxCommand};
use crate::error::AdapterError;

/// Generic adapter that passes through commands as-is.
pub struct GenericAdapter;

impl AgentAdapter for GenericAdapter {
    fn name(&self) -> &str {
        "generic"
    }

    fn validate(&self, _config: &CboxConfig) -> Result<(), AdapterError> {
        Ok(())
    }

    fn prepare_env(
        &self,
        env: &mut HashMap<String, String>,
        config: &CboxConfig,
    ) -> Result<(), AdapterError> {
        // Pass through requested env vars from host
        for key in &config.adapter.env_passthrough {
            if let Ok(val) = std::env::var(key) {
                env.insert(key.clone(), val);
            }
        }
        Ok(())
    }

    fn build_command(
        &self,
        user_command: &[String],
        _config: &CboxConfig,
    ) -> Result<SandboxCommand, AdapterError> {
        if user_command.is_empty() {
            return Err(AdapterError::Validation("no command provided".to_string()));
        }
        Ok(SandboxCommand {
            program: user_command[0].clone(),
            args: user_command[1..].to_vec(),
            env: HashMap::new(),
            working_dir: None,
        })
    }
}
