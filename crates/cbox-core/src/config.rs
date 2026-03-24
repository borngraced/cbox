use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::CoreError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CboxConfig {
    #[serde(default)]
    pub sandbox: SandboxConfig,

    #[serde(default)]
    pub network: NetworkConfig,

    #[serde(default)]
    pub resources: ResourceConfig,

    #[serde(default)]
    pub adapter: AdapterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Container image to use for the container backend (e.g. "ubuntu:24.04")
    #[serde(default = "default_image")]
    pub image: String,

    /// Directories to mount read-only inside the sandbox
    #[serde(default = "default_ro_mounts")]
    pub ro_mounts: Vec<String>,

    /// Extra directories to overlay (read-write via overlayfs)
    #[serde(default)]
    pub overlay_dirs: Vec<String>,

    /// Paths to bind-mount read-write (direct pass-through to host)
    #[serde(default)]
    pub rw_mounts: Vec<String>,

    /// Syscalls to additionally block (beyond default denylist)
    #[serde(default)]
    pub blocked_syscalls: Vec<String>,

    /// Glob patterns to exclude from diff/merge (e.g. shell history, editor caches)
    #[serde(default = "default_merge_exclude")]
    pub merge_exclude: Vec<String>,
}

fn default_image() -> String {
    "ubuntu:24.04".to_string()
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            image: default_image(),
            ro_mounts: default_ro_mounts(),
            rw_mounts: vec![],
            overlay_dirs: vec![],
            blocked_syscalls: vec![],
            merge_exclude: default_merge_exclude(),
        }
    }
}

fn default_merge_exclude() -> Vec<String> {
    vec![
        "root/.bash_history".to_string(),
        "root/.cache/**".to_string(),
        "root/.local/**".to_string(),
        "root/.config/**".to_string(),
        "home/**".to_string(),
        ".bash_history".to_string(),
        ".viminfo".to_string(),
        ".lesshst".to_string(),
    ]
}

fn default_ro_mounts() -> Vec<String> {
    vec![
        "/usr".to_string(),
        "/lib".to_string(),
        "/lib64".to_string(),
        "/bin".to_string(),
        "/sbin".to_string(),
        "/etc".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network mode: "deny" (default) or "allow"
    #[serde(default = "default_network_mode")]
    pub mode: String,

    /// Whitelisted hosts (only used when mode = "deny")
    #[serde(default)]
    pub allow: Vec<String>,

    /// DNS servers to use inside the sandbox
    #[serde(default = "default_dns")]
    pub dns: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mode: default_network_mode(),
            allow: vec![],
            dns: default_dns(),
        }
    }
}

fn default_network_mode() -> String {
    "deny".to_string()
}

fn default_dns() -> Vec<String> {
    vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Memory limit (e.g., "4G", "512M")
    #[serde(default = "default_memory")]
    pub memory: String,

    /// CPU limit as percentage (e.g., "200%" for 2 cores)
    #[serde(default = "default_cpu")]
    pub cpu: String,

    /// Max number of PIDs
    #[serde(default = "default_max_pids")]
    pub max_pids: u64,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            memory: default_memory(),
            cpu: default_cpu(),
            max_pids: default_max_pids(),
        }
    }
}

fn default_memory() -> String {
    "4G".to_string()
}

fn default_cpu() -> String {
    "200%".to_string()
}

fn default_max_pids() -> u64 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterConfig {
    /// Default adapter name
    #[serde(default = "default_adapter")]
    pub default: String,

    /// Environment variables to pass through to the sandbox
    #[serde(default)]
    pub env_passthrough: Vec<String>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            default: default_adapter(),
            env_passthrough: vec![],
        }
    }
}

fn default_adapter() -> String {
    "auto".to_string()
}

impl CboxConfig {
    /// Load config from a file path.
    pub fn load(path: &Path) -> Result<Self, CoreError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| CoreError::Config(format!("failed to read {}: {}", path.display(), e)))?;
        toml::from_str(&contents)
            .map_err(|e| CoreError::Config(format!("failed to parse {}: {}", path.display(), e)))
    }

    /// Load config with layered resolution:
    /// 1. Built-in defaults
    /// 2. Global config at ~/.config/cbox/config.toml (overrides defaults)
    /// 3. Per-project cbox.toml found by walking up from `dir` (overrides global)
    pub fn find_and_load(dir: &Path) -> Result<Self, CoreError> {
        // Start with defaults
        let mut config = Self::default();

        // Layer 2: global config
        if let Some(global_path) = Self::global_config_path() {
            if global_path.exists() {
                let global = Self::load(&global_path)?;
                config.merge(global);
            }
        }

        // Layer 3: per-project cbox.toml (walk up from dir)
        let mut current = dir.to_path_buf();
        loop {
            let candidate = current.join("cbox.toml");
            if candidate.exists() {
                let project = Self::load(&candidate)?;
                config.merge(project);
                break;
            }
            if !current.pop() {
                break;
            }
        }

        Ok(config)
    }

    /// Path to global config file.
    pub fn global_config_path() -> Option<PathBuf> {
        let config_dir = if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(dir)
        } else {
            let home = std::env::var("HOME").ok()?;
            PathBuf::from(home).join(".config")
        };
        Some(config_dir.join("cbox/config.toml"))
    }

    /// Merge another config on top of this one.
    /// Non-default values in `other` override values in `self`.
    fn merge(&mut self, other: Self) {
        if other.sandbox.image != default_image() {
            self.sandbox.image = other.sandbox.image;
        }
        if other.sandbox.ro_mounts != default_ro_mounts() {
            self.sandbox.ro_mounts = other.sandbox.ro_mounts;
        }
        if !other.sandbox.rw_mounts.is_empty() {
            self.sandbox.rw_mounts = other.sandbox.rw_mounts;
        }
        if !other.sandbox.overlay_dirs.is_empty() {
            self.sandbox.overlay_dirs = other.sandbox.overlay_dirs;
        }
        if !other.sandbox.blocked_syscalls.is_empty() {
            self.sandbox.blocked_syscalls = other.sandbox.blocked_syscalls;
        }
        if other.sandbox.merge_exclude != default_merge_exclude() {
            self.sandbox.merge_exclude = other.sandbox.merge_exclude;
        }

        if other.network.mode != default_network_mode() {
            self.network.mode = other.network.mode;
        }
        if !other.network.allow.is_empty() {
            self.network.allow = other.network.allow;
        }
        if other.network.dns != default_dns() {
            self.network.dns = other.network.dns;
        }

        if other.resources.memory != default_memory() {
            self.resources.memory = other.resources.memory;
        }
        if other.resources.cpu != default_cpu() {
            self.resources.cpu = other.resources.cpu;
        }
        if other.resources.max_pids != default_max_pids() {
            self.resources.max_pids = other.resources.max_pids;
        }

        if other.adapter.default != default_adapter() {
            self.adapter.default = other.adapter.default;
        }
        if !other.adapter.env_passthrough.is_empty() {
            self.adapter.env_passthrough = other.adapter.env_passthrough;
        }
    }

    /// Parse a memory string like "4G" or "512M" into bytes.
    pub fn parse_memory_bytes(s: &str) -> Result<u64, CoreError> {
        let s = s.trim();
        let (num, multiplier) = if let Some(n) = s.strip_suffix('G') {
            (n, 1024 * 1024 * 1024u64)
        } else if let Some(n) = s.strip_suffix('M') {
            (n, 1024 * 1024u64)
        } else if let Some(n) = s.strip_suffix('K') {
            (n, 1024u64)
        } else {
            (s, 1u64)
        };
        let value: u64 = num
            .parse()
            .map_err(|_| CoreError::Config(format!("invalid memory value: {}", s)))?;
        Ok(value * multiplier)
    }

    /// Parse CPU percentage like "200%" into a quota (microseconds per period).
    /// Returns (quota_us, period_us).
    pub fn parse_cpu_quota(s: &str) -> Result<(u64, u64), CoreError> {
        let s = s.trim();
        let pct_str = s
            .strip_suffix('%')
            .ok_or_else(|| CoreError::Config(format!("CPU must end with %: {}", s)))?;
        let pct: u64 = pct_str
            .parse()
            .map_err(|_| CoreError::Config(format!("invalid CPU value: {}", s)))?;
        let period: u64 = 100_000; // 100ms
        let quota = period * pct / 100;
        Ok((quota, period))
    }

    /// Get the project root directory (where cbox.toml was found, or cwd).
    pub fn project_root(dir: &Path) -> PathBuf {
        let mut current = dir.to_path_buf();
        loop {
            if current.join("cbox.toml").exists() {
                return current;
            }
            if !current.pop() {
                return dir.to_path_buf();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CboxConfig::default();
        assert_eq!(config.network.mode, "deny");
        assert_eq!(config.resources.memory, "4G");
        assert_eq!(config.resources.max_pids, 4096);
    }

    #[test]
    fn test_parse_memory() {
        assert_eq!(
            CboxConfig::parse_memory_bytes("4G").unwrap(),
            4 * 1024 * 1024 * 1024
        );
        assert_eq!(
            CboxConfig::parse_memory_bytes("512M").unwrap(),
            512 * 1024 * 1024
        );
        assert_eq!(
            CboxConfig::parse_memory_bytes("1024K").unwrap(),
            1024 * 1024
        );
    }

    #[test]
    fn test_parse_cpu() {
        let (quota, period) = CboxConfig::parse_cpu_quota("200%").unwrap();
        assert_eq!(period, 100_000);
        assert_eq!(quota, 200_000);

        let (quota, period) = CboxConfig::parse_cpu_quota("50%").unwrap();
        assert_eq!(period, 100_000);
        assert_eq!(quota, 50_000);
    }

    #[test]
    fn test_parse_config_toml() {
        let toml_str = r#"
[network]
mode = "deny"
allow = ["api.anthropic.com:443"]

[resources]
memory = "2G"
cpu = "100%"
"#;
        let config: CboxConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.network.allow, vec!["api.anthropic.com:443"]);
        assert_eq!(config.resources.memory, "2G");
    }
}
