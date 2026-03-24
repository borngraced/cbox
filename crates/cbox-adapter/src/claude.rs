use std::collections::HashMap;

use cbox_core::CboxConfig;

use crate::adapter::{AgentAdapter, SandboxCommand};
use crate::error::AdapterError;

/// Adapter for Claude Code that sets up the appropriate environment...
pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn validate(&self, _config: &CboxConfig) -> Result<(), AdapterError> {
        // Check that claude CLI is available (will be inside sandbox)
        // We can't fully validate here since we're on the host
        Ok(())
    }

    fn prepare_env(
        &self,
        env: &mut HashMap<String, String>,
        config: &CboxConfig,
    ) -> Result<(), AdapterError> {
        // Pass through API key
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            env.insert("ANTHROPIC_API_KEY".to_string(), key);
        }

        // Tell Claude Code it's running in a cbox sandbox
        env.insert("CLAUDE_CODE_SANDBOX".to_string(), "cbox".to_string());

        // Pass through any additional env vars from config
        for key in &config.adapter.env_passthrough {
            if let Ok(val) = std::env::var(key) {
                env.insert(key.clone(), val);
            }
        }

        // Standard env setup
        if let Ok(home) = std::env::var("HOME") {
            env.insert("HOME".to_string(), home);
        }
        env.insert("TERM".to_string(), std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()));

        Ok(())
    }

    fn build_command(
        &self,
        user_command: &[String],
        _config: &CboxConfig,
    ) -> Result<SandboxCommand, AdapterError> {
        let (program, args) = if user_command.is_empty() {
            // Default to launching claude
            ("claude".to_string(), vec![])
        } else {
            (user_command[0].clone(), user_command[1..].to_vec())
        };

        Ok(SandboxCommand {
            program,
            args,
            env: HashMap::new(),
            working_dir: None,
        })
    }
}
