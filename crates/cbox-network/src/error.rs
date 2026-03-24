use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("failed to create veth pair: {0}")]
    VethCreation(String),

    #[error("failed to configure network: {0}")]
    Config(String),

    #[error("failed to apply iptables rules: {0}")]
    Iptables(String),

    #[error("failed to clean up network: {0}")]
    Cleanup(String),

    #[error("DNS resolution failed for {host}: {reason}")]
    DnsResolution { host: String, reason: String },

    #[error("command failed: {cmd}: {reason}")]
    Command { cmd: String, reason: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
