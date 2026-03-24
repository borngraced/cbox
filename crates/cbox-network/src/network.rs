use std::net::ToSocketAddrs;
use std::process::Command;

use cbox_core::Session;
use tracing::{debug, info, warn};

use crate::error::NetworkError;

/// Network configuration resolved from config + session.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub mode: NetworkMode,
    pub allowed_hosts: Vec<ResolvedHost>,
    pub dns_servers: Vec<String>,
    pub subnet_index: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkMode {
    Deny,
    Allow,
}

#[derive(Debug, Clone)]
pub struct ResolvedHost {
    pub original: String,
    pub ips: Vec<String>,
    pub port: Option<u16>,
}

pub struct NetworkSetup;

impl NetworkSetup {
    /// Resolve DNS for all whitelist entries while we still have host DNS.
    pub fn resolve_whitelist(
        hosts: &[String],
    ) -> Result<Vec<ResolvedHost>, NetworkError> {
        let mut resolved = Vec::new();
        for host_str in hosts {
            let (host, port) = if let Some((h, p)) = host_str.rsplit_once(':') {
                (h.to_string(), p.parse::<u16>().ok())
            } else {
                (host_str.clone(), None)
            };

            let lookup_addr = format!("{}:{}", host, port.unwrap_or(80));
            let ips: Vec<String> = match lookup_addr.to_socket_addrs() {
                Ok(addrs) => addrs.map(|a| a.ip().to_string()).collect(),
                Err(e) => {
                    warn!("DNS resolution failed for {}: {}", host, e);
                    vec![host.clone()]
                }
            };

            debug!("resolved {} -> {:?} (port: {:?})", host_str, ips, port);
            resolved.push(ResolvedHost {
                original: host_str.clone(),
                ips,
                port,
            });
        }
        Ok(resolved)
    }

    /// Generate the veth interface name for a session.
    pub fn veth_host_name(session_id: &str) -> String {
        let prefix: String = session_id.chars().take(6).collect();
        format!("cbox_{}", prefix)
    }

    /// Create veth pair and move one end into the child's network namespace.
    pub fn create_veth_pair(
        host_name: &str,
        child_pid: u32,
        subnet_index: u8,
    ) -> Result<(), NetworkError> {
        // Use a temp name for the peer to avoid conflicts with host interfaces
        let peer_tmp = format!("{}_p", host_name);
        let host_ip = format!("10.200.{}.1/30", subnet_index);

        run_cmd("ip", &[
            "link", "add", host_name, "type", "veth", "peer", "name", &peer_tmp,
        ])?;
        // Move peer into child's netns, then rename to eth0 inside
        run_cmd("ip", &[
            "link", "set", &peer_tmp, "netns", &child_pid.to_string(),
        ])?;
        // Rename inside the child's netns
        run_cmd("nsenter", &[
            &format!("--net=/proc/{}/ns/net", child_pid),
            "ip", "link", "set", &peer_tmp, "name", "eth0",
        ])?;
        run_cmd("ip", &["addr", "add", &host_ip, "dev", host_name])?;
        run_cmd("ip", &["link", "set", host_name, "up"])?;

        info!(
            "veth pair created: {} (host) <-> eth0 (sandbox pid {})",
            host_name, child_pid
        );
        Ok(())
    }

    /// Configure network inside the sandbox (called from child process).
    pub fn configure_child_network(subnet_index: u8, dns_servers: &[String]) -> Result<(), NetworkError> {
        let ip = format!("10.200.{}.2/30", subnet_index);
        let gateway = format!("10.200.{}.1", subnet_index);

        run_cmd("ip", &["addr", "add", &ip, "dev", "eth0"])?;
        run_cmd("ip", &["link", "set", "eth0", "up"])?;
        run_cmd("ip", &["link", "set", "lo", "up"])?;
        run_cmd("ip", &["route", "add", "default", "via", &gateway])?;

        let resolv_content: String = dns_servers
            .iter()
            .map(|s| format!("nameserver {}", s))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write("/etc/resolv.conf", resolv_content + "\n")
            .map_err(|e| NetworkError::Config(format!("write resolv.conf: {}", e)))?;

        info!("child network configured: ip={}, gw={}", ip, gateway);
        Ok(())
    }

    /// Enable IP forwarding with reference counting.
    ///
    /// Multiple concurrent cbox sessions may each need ip_forward=1. We save
    /// the original value on first enable and track how many sessions hold a
    /// reference. Only when the last session releases does the original value
    /// get restored.
    ///
    /// State files live in the cbox data dir alongside sessions:
    ///   ip_forward.orig  — original value before cbox touched it
    ///   ip_forward.count — number of active sessions using forwarding
    pub fn enable_ip_forward() {
        let dir = Self::ip_forward_state_dir();
        let _ = std::fs::create_dir_all(&dir);
        let orig_path = dir.join("ip_forward.orig");
        let count_path = dir.join("ip_forward.count");

        // Save original value only if this is the first session
        if !orig_path.exists() {
            if let Ok(val) = std::fs::read_to_string("/proc/sys/net/ipv4/ip_forward") {
                let _ = std::fs::write(&orig_path, val.trim());
            }
        }

        // Increment reference count
        let count = std::fs::read_to_string(&count_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let _ = std::fs::write(&count_path, (count + 1).to_string());

        if let Err(e) = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1") {
            eprintln!("warning: failed to enable IP forwarding: {}", e);
        }
    }

    /// Release one reference to IP forwarding. When the last reference is
    /// released, restore the original value.
    pub fn release_ip_forward() {
        let dir = Self::ip_forward_state_dir();
        let orig_path = dir.join("ip_forward.orig");
        let count_path = dir.join("ip_forward.count");

        let count = std::fs::read_to_string(&count_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1);

        if count <= 1 {
            // Last session — restore original value and clean up state files
            if let Ok(orig) = std::fs::read_to_string(&orig_path) {
                if let Err(e) = std::fs::write("/proc/sys/net/ipv4/ip_forward", orig.trim()) {
                    eprintln!("warning: failed to restore IP forwarding: {}", e);
                }
            }
            let _ = std::fs::remove_file(&orig_path);
            let _ = std::fs::remove_file(&count_path);
        } else {
            let _ = std::fs::write(&count_path, (count - 1).to_string());
        }
    }

    fn ip_forward_state_dir() -> std::path::PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                std::path::PathBuf::from(home).join(".local/share")
            });
        data_dir.join("cbox")
    }

    /// Apply iptables rules for the sandbox.
    /// Returns the list of rules applied (for cleanup).
    pub fn apply_iptables_rules(
        host_veth: &str,
        config: &NetworkConfig,
    ) -> Result<Vec<String>, NetworkError> {
        let mut rules = Vec::new();

        match config.mode {
            NetworkMode::Allow => {
                let rule = format!(
                    "iptables -A FORWARD -i {} -j ACCEPT",
                    host_veth
                );
                run_iptables_rule(&rule)?;
                rules.push(rule);
            }
            NetworkMode::Deny => {
                for host in &config.allowed_hosts {
                    for ip in &host.ips {
                        let rule = if let Some(port) = host.port {
                            format!(
                                "iptables -A FORWARD -i {} -d {} -p tcp --dport {} -j ACCEPT",
                                host_veth, ip, port
                            )
                        } else {
                            format!(
                                "iptables -A FORWARD -i {} -d {} -j ACCEPT",
                                host_veth, ip
                            )
                        };
                        run_iptables_rule(&rule)?;
                        rules.push(rule);
                    }
                }

                for dns in &config.dns_servers {
                    let rule = format!(
                        "iptables -A FORWARD -i {} -d {} -p udp --dport 53 -j ACCEPT",
                        host_veth, dns
                    );
                    run_iptables_rule(&rule)?;
                    rules.push(rule);
                }

                let drop_rule = format!(
                    "iptables -A FORWARD -i {} -j DROP",
                    host_veth
                );
                run_iptables_rule(&drop_rule)?;
                rules.push(drop_rule);
            }
        }

        let subnet = format!("10.200.{}.0/30", config.subnet_index);
        let nat_rule = format!(
            "iptables -t nat -A POSTROUTING -s {} -j MASQUERADE",
            subnet
        );
        run_iptables_rule(&nat_rule)?;
        rules.push(nat_rule);

        Self::enable_ip_forward();

        info!("iptables: {} rules applied for {}", rules.len(), host_veth);
        Ok(rules)
    }

    /// Remove iptables rules that were applied for a session.
    pub fn cleanup_iptables(rules: &[String]) -> Result<(), NetworkError> {
        for rule in rules.iter().rev() {
            let delete_rule = rule.replace(" -A ", " -D ");
            if let Err(e) = run_iptables_rule(&delete_rule) {
                warn!("failed to remove iptables rule: {} ({})", delete_rule, e);
            }
        }
        Ok(())
    }

    /// Delete a veth interface (also removes peer automatically).
    pub fn delete_veth(host_name: &str) -> Result<(), NetworkError> {
        if let Err(e) = run_cmd("ip", &["link", "delete", host_name]) {
            warn!("failed to delete veth {}: {}", host_name, e);
        }
        Ok(())
    }

    /// Allocate a subnet index that doesn't conflict with existing sessions.
    pub fn allocate_subnet_index(sessions: &[Session]) -> u8 {
        let used: std::collections::HashSet<u8> = sessions
            .iter()
            .filter_map(|s| s.subnet_index)
            .collect();
        for i in 1..=254 {
            if !used.contains(&i) {
                return i;
            }
        }
        1
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<(), NetworkError> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| NetworkError::Command {
            cmd: format!("{} {}", cmd, args.join(" ")),
            reason: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NetworkError::Command {
            cmd: format!("{} {}", cmd, args.join(" ")),
            reason: stderr.to_string(),
        });
    }
    Ok(())
}

fn run_iptables_rule(rule: &str) -> Result<(), NetworkError> {
    let parts: Vec<&str> = rule.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }
    run_cmd(parts[0], &parts[1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_veth_name() {
        assert_eq!(NetworkSetup::veth_host_name("abcdef12"), "cbox_abcdef");
        assert_eq!(NetworkSetup::veth_host_name("ab"), "cbox_ab");
        // Empty and exact-6 edge cases
        assert_eq!(NetworkSetup::veth_host_name(""), "cbox_");
        assert_eq!(NetworkSetup::veth_host_name("abcdef"), "cbox_abcdef");
        assert_eq!(NetworkSetup::veth_host_name("a"), "cbox_a");
        // Full UUID-style input
        assert_eq!(NetworkSetup::veth_host_name("550e8400-e29b-41d4-a716-446655440000"), "cbox_550e84");
    }

    #[test]
    fn test_resolve_whitelist_ip() {
        let hosts = vec!["1.2.3.4:443".to_string()];
        let resolved = NetworkSetup::resolve_whitelist(&hosts).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].port, Some(443));
    }

    #[test]
    fn test_subnet_allocation() {
        let sessions = vec![];
        assert_eq!(NetworkSetup::allocate_subnet_index(&sessions), 1);
    }
}
