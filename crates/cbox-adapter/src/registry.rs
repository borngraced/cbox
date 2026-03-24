use std::collections::HashMap;

use crate::adapter::AgentAdapter;
use crate::claude::ClaudeCodeAdapter;
use crate::error::AdapterError;
use crate::generic::GenericAdapter;

/// Registry of available agent adapters.
pub struct AdapterRegistry {
    adapters: HashMap<String, Box<dyn AgentAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            adapters: HashMap::new(),
        };
        reg.register(Box::new(GenericAdapter));
        reg.register(Box::new(ClaudeCodeAdapter));
        reg
    }

    pub fn register(&mut self, adapter: Box<dyn AgentAdapter>) {
        self.adapters.insert(adapter.name().to_string(), adapter);
    }

    pub fn get(&self, name: &str) -> Result<&dyn AgentAdapter, AdapterError> {
        self.adapters
            .get(name)
            .map(|a| a.as_ref())
            .ok_or_else(|| AdapterError::NotFound(name.to_string()))
    }

    /// Auto-detect adapter based on the command being run.
    pub fn detect(&self, command: &[String]) -> &dyn AgentAdapter {
        if let Some(cmd) = command.first() {
            let cmd_lower = cmd.to_lowercase();
            if cmd_lower.contains("claude") {
                if let Ok(adapter) = self.get("claude") {
                    return adapter;
                }
            }
        }
        self.get("generic").expect("generic adapter must exist")
    }

    /// Resolve adapter name, handling "auto" by using detect.
    pub fn resolve<'a>(
        &'a self,
        name: &str,
        command: &[String],
    ) -> Result<&'a dyn AgentAdapter, AdapterError> {
        if name == "auto" {
            Ok(self.detect(command))
        } else {
            self.get(name)
        }
    }

    pub fn list_names(&self) -> Vec<&str> {
        self.adapters.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_defaults() {
        let reg = AdapterRegistry::new();
        assert!(reg.get("generic").is_ok());
        assert!(reg.get("claude").is_ok());
        assert!(reg.get("nonexistent").is_err());
    }

    #[test]
    fn test_auto_detect_claude() {
        let reg = AdapterRegistry::new();
        let cmd = vec!["claude".to_string(), "code".to_string()];
        let adapter = reg.detect(&cmd);
        assert_eq!(adapter.name(), "claude");
    }

    #[test]
    fn test_auto_detect_generic() {
        let reg = AdapterRegistry::new();
        let cmd = vec!["bash".to_string()];
        let adapter = reg.detect(&cmd);
        assert_eq!(adapter.name(), "generic");
    }
}
