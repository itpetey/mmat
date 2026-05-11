//! Tool registry and runtime types shared across roles.

use mmat_event_stream::event_bus::EventBus;
use mmat_llm::tool::ToolRegistry;
use serde_json::Value;
use thiserror::Error;

/// Alias for a tool registry parameterised for role-level tools.
pub type RoleToolRegistry = ToolRegistry<RoleToolRuntime, RoleToolError>;

/// Runtime context for role tools, providing access to the event bus.
pub struct RoleToolRuntime {
    pub bus: Option<EventBus>,
}

impl RoleToolRuntime {
    pub fn new() -> Self {
        Self { bus: None }
    }

    pub fn with_bus(bus: EventBus) -> Self {
        Self { bus: Some(bus) }
    }
}

impl Default for RoleToolRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during role tool execution.
#[derive(Debug, Error)]
pub enum RoleToolError {
    /// A general tool error.
    #[error("{0}")]
    Error(String),
}

/// Produces an empty tool result (JSON null).
pub fn empty_tool_result() -> Value {
    Value::Null
}
