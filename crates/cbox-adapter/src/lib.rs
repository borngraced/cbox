pub mod adapter;
pub mod claude;
pub mod error;
pub mod generic;
pub mod registry;

pub use adapter::{AgentAdapter, SandboxCommand};
pub use claude::ClaudeCodeAdapter;
pub use error::AdapterError;
pub use generic::GenericAdapter;
pub use registry::AdapterRegistry;
