//! Tool trait and registry for tool-using LLM interactions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A callable tool that can be invoked by the LLM.
#[async_trait::async_trait]
pub trait Tool<Runtime, Error>: Send + Sync {
    /// Return the specification for this tool.
    fn spec(&self) -> ToolSpec;

    /// Invoke the tool with the given runtime reference and arguments.
    async fn call(&self, runtime: &Runtime, arguments: Value) -> Result<Value, Error>;
}

/// Specification describing a tool to the LLM.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// The unique name of the tool.
    pub name: String,
    /// A human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub input_schema: Value,
}

/// Errors that may occur when registering a tool.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// A tool with this name is already registered.
    #[error("Tool already registered: {0}")]
    DuplicateName(String),
}

/// A collection of registered [`Tool`] implementations.
pub struct ToolRegistry<Runtime, Error> {
    tools: HashMap<String, Box<dyn Tool<Runtime, Error>>>,
}

impl ToolSpec {
    /// Convert this spec into the OpenAI function-calling JSON format.
    pub fn to_openai_tool(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.input_schema,
            }
        })
    }
}

impl<Runtime, Error> ToolRegistry<Runtime, Error> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool, returning an error if the name is already taken.
    pub fn register(&mut self, tool: Box<dyn Tool<Runtime, Error>>) -> Result<(), RegistryError> {
        let spec = tool.spec();
        if self.tools.contains_key(&spec.name) {
            return Err(RegistryError::DuplicateName(spec.name));
        }
        self.tools.insert(spec.name, tool);
        Ok(())
    }

    /// Return the specifications of all registered tools.
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    /// Return all registered tools in the OpenAI function-calling format.
    pub fn openai_tools(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| t.spec().to_openai_tool())
            .collect()
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool<Runtime, Error>> {
        self.tools.get(name).map(|t| t.as_ref())
    }
}

impl<Runtime, Error> Default for ToolRegistry<Runtime, Error> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thiserror::Error;

    #[derive(Error, Debug)]
    #[error("tool error")]
    struct TestErr;

    #[test]
    fn tool_spec_to_openai_format() {
        let spec = ToolSpec {
            name: "search".into(),
            description: "Search the web".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        };
        let openai = spec.to_openai_tool();
        assert_eq!(openai["type"], "function");
        assert_eq!(openai["function"]["name"], "search");
        assert_eq!(openai["function"]["description"], "Search the web");
        assert_eq!(openai["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn registry_rejects_duplicate_names() {
        struct DummyTool;
        #[async_trait::async_trait]
        impl Tool<(), TestErr> for DummyTool {
            fn spec(&self) -> ToolSpec {
                ToolSpec {
                    name: "dummy".into(),
                    description: "test".into(),
                    input_schema: serde_json::json!({}),
                }
            }
            async fn call(&self, _runtime: &(), _arguments: Value) -> Result<Value, TestErr> {
                Ok(Value::Null)
            }
        }

        let mut registry = ToolRegistry::<(), TestErr>::new();
        registry.register(Box::new(DummyTool)).unwrap();
        let result = registry.register(Box::new(DummyTool));
        assert!(result.is_err());
    }

    #[test]
    fn registry_openai_tools() {
        struct DummyTool;
        #[async_trait::async_trait]
        impl Tool<(), TestErr> for DummyTool {
            fn spec(&self) -> ToolSpec {
                ToolSpec {
                    name: "dummy".into(),
                    description: "test".into(),
                    input_schema: serde_json::json!({}),
                }
            }
            async fn call(&self, _runtime: &(), _arguments: Value) -> Result<Value, TestErr> {
                Ok(Value::Null)
            }
        }

        let mut registry = ToolRegistry::<(), TestErr>::new();
        registry.register(Box::new(DummyTool)).unwrap();
        let tools = registry.openai_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "dummy");
    }
}
