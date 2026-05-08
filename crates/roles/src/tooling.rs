//! Tool registry and runtime types shared across roles.

use mmat_llm::tool::ToolRegistry;
use serde_json::Value;
use thiserror::Error;

/// Alias for a tool registry parameterised for role-level tools.
pub type RoleToolRegistry = ToolRegistry<RoleToolRuntime, RoleToolError>;

/// Unit struct representing the runtime context for role tools.
#[derive(Debug, Default)]
pub struct RoleToolRuntime;

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
