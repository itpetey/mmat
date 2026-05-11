//! LLM-callable tools for lane creation and action requests.

use async_trait::async_trait;
use mmat_event_stream::event::{RoleId, SemanticEvent};
use mmat_llm::tool::{Tool, ToolSpec};
use serde_json::Value;

use crate::tooling::RoleToolError;
use crate::tooling::RoleToolRuntime;

pub struct CreateLaneTool;

#[async_trait]
impl Tool<RoleToolRuntime, RoleToolError> for CreateLaneTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "create_lane".to_string(),
            description:
                "Create a new conversation lane to isolate attention for a topic or delivery stream"
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Short human-readable name for the lane"
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["discovery", "delivery", "conversation", "system"],
                        "description": "The kind of lane"
                    },
                    "purpose": {
                        "type": "string",
                        "description": "One-sentence description of what the lane is for"
                    },
                    "colour": {
                        "type": "string",
                        "description": "CSS colour string for the lane chip (e.g. #ff6b6b, blue, purple)"
                    },
                    "parent_lane_id": {
                        "type": "string",
                        "description": "Optional ID of the parent lane"
                    },
                    "related_lane_ids": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional related lane IDs"
                    },
                    "source_message_id": {
                        "type": "string",
                        "description": "Optional message ID that inspired this lane creation"
                    }
                },
                "required": ["name", "kind", "purpose"]
            }),
        }
    }

    async fn call(
        &self,
        runtime: &RoleToolRuntime,
        arguments: Value,
    ) -> Result<Value, RoleToolError> {
        let name = arguments["name"]
            .as_str()
            .ok_or_else(|| RoleToolError::Error("missing name".to_string()))?;
        let kind = arguments["kind"]
            .as_str()
            .ok_or_else(|| RoleToolError::Error("missing kind".to_string()))?;
        let purpose = arguments["purpose"]
            .as_str()
            .ok_or_else(|| RoleToolError::Error("missing purpose".to_string()))?;
        let colour = arguments["colour"]
            .as_str()
            .unwrap_or(&default_lane_colour(name))
            .to_string();
        let parent_lane_id = arguments["parent_lane_id"].as_str().map(|s| s.to_string());
        let related_lane_ids: Vec<String> = arguments["related_lane_ids"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let source_message_id = arguments["source_message_id"]
            .as_str()
            .map(|s| s.to_string());

        let lane_id = format!("lane-{}-{}", kind, uuid::Uuid::new_v4());

        let event = SemanticEvent::new_lane_created(
            RoleId::new("tool:create_lane"),
            &lane_id,
            name,
            kind,
            colour,
            purpose,
            parent_lane_id,
            related_lane_ids,
            source_message_id,
        );

        if let Some(bus) = &runtime.bus {
            bus.publish(event).map_err(|err| {
                RoleToolError::Error(format!("failed to publish LaneCreated event: {err}"))
            })?;
        }

        Ok(serde_json::json!({
            "lane_id": lane_id,
            "name": name,
        }))
    }
}

fn default_lane_colour(name: &str) -> String {
    let colours = [
        "#6366f1", "#ec4899", "#14b8a6", "#f59e0b", "#8b5cf6", "#06b6d4", "#f43f5e", "#10b981",
    ];
    let index = name
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_add(b as usize));
    colours[index % colours.len()].to_string()
}
