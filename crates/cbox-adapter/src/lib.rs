pub mod error;
pub mod adapter;
pub mod generic;
pub mod claude;
pub mod registry;

pub use adapter::{AgentAdapter, SandboxCommand};
pub use error::AdapterError;
pub use generic::GenericAdapter;
pub use claude::ClaudeCodeAdapter;
pub use registry::AdapterRegistry;
