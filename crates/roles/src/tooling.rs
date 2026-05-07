use serde_json::Value;
use thiserror::Error;

pub type RoleToolRegistry = llm::tool::ToolRegistry<RoleToolRuntime, RoleToolError>;

#[derive(Debug, Default)]
pub struct RoleToolRuntime;

#[derive(Debug, Error)]
pub enum RoleToolError {
    #[error("{0}")]
    Error(String),
}

pub fn empty_tool_result() -> Value {
    Value::Null
}
