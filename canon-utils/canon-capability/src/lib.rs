pub mod context;
pub mod registry;
pub mod result;
pub mod r#trait;
//
pub use context::CapabilityExecutionContext;
pub use registry::CapabilityRegistry;
pub use result::CapabilityExecutionResult;
pub use r#trait::CapabilityHandler;
