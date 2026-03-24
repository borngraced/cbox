use std::path::Path;
use std::process::Command;

use tracing::{info, warn};

/// Detected system capabilities for sandbox features.
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub user_namespaces: bool,
    pub overlayfs: bool,
    pub cgroups_v2: bool,
    pub iptables: bool,
    pub ip_command: bool,
}

impl Capabilities {
    /// Probe the current system for available capabilities.
    pub fn detect() -> Self {
        let caps = Self {
            user_namespaces: Self::probe_user_namespaces(),
            overlayfs: Self::probe_overlayfs(),
            cgroups_v2: Self::probe_cgroups_v2(),
            iptables: Self::probe_command("iptables"),
            ip_command: Self::probe_command("ip"),
        };

        if caps.user_namespaces {
            info!("user namespaces: available");
        } else {
            warn!("user namespaces: NOT available — sandboxing will not work");
        }
        if caps.overlayfs {
            info!("overlayfs: available");
        } else {
            warn!("overlayfs: NOT available — will attempt fuse-overlayfs fallback");
        }
        if caps.cgroups_v2 {
            info!("cgroups v2: available");
        } else {
            warn!("cgroups v2: NOT available — resource limits disabled");
        }
        if !caps.iptables || !caps.ip_command {
            warn!(
                "network tools missing — network isolation will use empty netns (no connectivity)"
            );
        }

        caps
    }

    fn probe_user_namespaces() -> bool {
        // Check if user namespaces are enabled via sysctl
        if let Ok(contents) = std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
        {
            if contents.trim() == "0" {
                return false;
            }
        }
        // Also check max_user_namespaces
        if let Ok(contents) = std::fs::read_to_string("/proc/sys/user/max_user_namespaces") {
            if let Ok(max) = contents.trim().parse::<u64>() {
                if max == 0 {
                    return false;
                }
            }
        }
        true
    }

    fn probe_overlayfs() -> bool {
        // Check if overlay is listed in /proc/filesystems
        if let Ok(contents) = std::fs::read_to_string("/proc/filesystems") {
            return contents.contains("overlay");
        }
        false
    }

    fn probe_cgroups_v2() -> bool {
        // cgroups v2 is mounted at /sys/fs/cgroup with type cgroup2
        Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
    }

    fn probe_command(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Ensure minimum requirements are met, return error message if not.
    pub fn check_minimum(&self) -> Result<(), String> {
        if !self.user_namespaces {
            return Err("User namespaces are not available. Enable with: \
                 sudo sysctl -w kernel.unprivileged_userns_clone=1"
                .to_string());
        }
        Ok(())
    }
}
