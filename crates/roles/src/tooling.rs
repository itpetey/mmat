use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Default)]
pub struct RoleToolRuntime;

#[derive(Debug, Error)]
pub enum RoleToolError {
    #[error("{0}")]
    Error(String),
}

pub type RoleToolRegistry = llm::tool::ToolRegistry<RoleToolRuntime, RoleToolError>;

pub fn empty_tool_result() -> Value {
    Value::Null
}
