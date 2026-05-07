use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl ToolSpec {
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

#[async_trait::async_trait]
pub trait Tool<Runtime, Error>: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, runtime: &Runtime, arguments: Value) -> Result<Value, Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Tool already registered: {0}")]
    DuplicateName(String),
}

pub struct ToolRegistry<Runtime, Error> {
    tools: HashMap<String, Box<dyn Tool<Runtime, Error>>>,
}

impl<Runtime, Error> Default for ToolRegistry<Runtime, Error> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Runtime, Error> ToolRegistry<Runtime, Error> {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool<Runtime, Error>>) -> Result<(), RegistryError> {
        let spec = tool.spec();
        if self.tools.contains_key(&spec.name) {
            return Err(RegistryError::DuplicateName(spec.name));
        }
        self.tools.insert(spec.name, tool);
        Ok(())
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub fn openai_tools(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| t.spec().to_openai_tool())
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool<Runtime, Error>> {
        self.tools.get(name).map(|t| t.as_ref())
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
