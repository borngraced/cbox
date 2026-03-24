use std::collections::HashMap;
use std::path::PathBuf;

use cbox_core::CboxConfig;

use crate::adapter::{AgentAdapter, SandboxCommand};
use crate::error::AdapterError;

/// Adapter for Claude Code that sets up the appropriate environment.
pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    /// Find claude binary on the host and return its absolute path.
    fn find_claude_binary() -> Option<PathBuf> {
        let real_home = cbox_core::util::real_user_home();

        let mut candidates: Vec<PathBuf> =
            vec![PathBuf::from(&real_home).join(".local/bin/claude")];
        if let Ok(home) = std::env::var("HOME") {
            if home != real_home {
                candidates.push(PathBuf::from(home).join(".local/bin/claude"));
            }
        }
        candidates.push(PathBuf::from("/usr/local/bin/claude"));
        candidates.push(PathBuf::from("/usr/bin/claude"));

        candidates.into_iter().find(|p| p.exists())
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn validate(&self, config: &CboxConfig) -> Result<(), AdapterError> {
        if Self::find_claude_binary().is_none() {
            return Err(AdapterError::Validation(
                "claude binary not found in ~/.local/bin, /usr/local/bin, or /usr/bin".to_string(),
            ));
        }
        if config.network.mode == cbox_core::NetworkMode::Deny {
            return Err(AdapterError::Validation(
                "claude adapter requires network access. Use --network allow".to_string(),
            ));
        }
        Ok(())
    }

    fn prepare_env(
        &self,
        env: &mut HashMap<String, String>,
        config: &CboxConfig,
    ) -> Result<(), AdapterError> {
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            env.insert("ANTHROPIC_API_KEY".to_string(), key);
        }

        env.insert("CLAUDE_CODE_SANDBOX".to_string(), "cbox".to_string());

        for key in &config.adapter.env_passthrough {
            if let Ok(val) = std::env::var(key) {
                env.insert(key.clone(), val);
            }
        }

        // Use the real user's home so bind-mounted ~/.claude and ~/.local are found
        let home = cbox_core::util::real_user_home();
        env.insert("HOME".to_string(), home);
        env.insert(
            "TERM".to_string(),
            std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()),
        );

        Ok(())
    }

    fn extra_rw_mounts(&self) -> Vec<String> {
        let home = cbox_core::util::real_user_home();
        let mut mounts = Vec::new();
        for path in [
            format!("{}/.claude", home),
            format!("{}/.claude.json", home),
        ] {
            if std::path::Path::new(&path).exists() {
                mounts.push(path);
            }
        }
        mounts
    }

    fn build_command(
        &self,
        user_command: &[String],
        _config: &CboxConfig,
    ) -> Result<SandboxCommand, AdapterError> {
        let (program, args) = if user_command.is_empty() || user_command[0] == "claude" {
            // Resolve to absolute path so execve works after pivot_root
            let path = Self::find_claude_binary()
                .ok_or_else(|| AdapterError::Validation("claude binary not found".to_string()))?;
            let args = if user_command.len() > 1 {
                user_command[1..].to_vec()
            } else {
                vec![]
            };
            (path.to_string_lossy().to_string(), args)
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
