//! LLM-callable tools for lane creation and action requests.

use async_trait::async_trait;
use mmat_event_stream::event::{EventContext, EventId, RoleId, SemanticEvent};
use mmat_llm::tool::{Tool, ToolSpec};
use serde_json::Value;

use crate::{tooling::RoleToolError, tooling::RoleToolRuntime};

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
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to attach to the lane creation event"
                    },
                    "current_lane_id": {
                        "type": "string",
                        "description": "Current lane ID that is being branched from"
                    },
                    "source_event_id": {
                        "type": "string",
                        "description": "Optional event ID that inspired this lane creation"
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
        let project_id = arguments["project_id"].as_str();
        let current_lane_id = arguments["current_lane_id"].as_str();
        let source_event_id = arguments["source_event_id"]
            .as_str()
            .map(parse_event_id)
            .transpose()?;
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

        let mut event = SemanticEvent::new_lane_created(
            RoleId::new("tool:create_lane"),
            &lane_id,
            name,
            kind,
            colour,
            purpose,
            parent_lane_id,
            related_lane_ids,
            source_event_id,
            source_message_id,
        );
        if let Some(project_id) = project_id {
            let mut context = EventContext::new(
                "default-organisation",
                "default-workspace",
                project_id,
                "default-run",
            );
            if let Some(current_lane_id) = current_lane_id {
                context = context.with_lane_id(current_lane_id.to_string());
            }
            event = event.with_context(context);
        }

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

fn parse_event_id(raw: &str) -> Result<EventId, RoleToolError> {
    uuid::Uuid::parse_str(raw)
        .map(EventId::from)
        .map_err(|error| RoleToolError::Error(format!("invalid source_event_id: {error}")))
}

#[cfg(test)]
mod tests {
    use mmat_event_stream::event::SemanticEvent;
    use mmat_llm::tool::Tool;

    use super::*;

    #[tokio::test]
    async fn create_lane_tool_publishes_branch_provenance() {
        let bus = mmat_event_stream::event_bus::EventBus::new(16);
        let mut receiver = bus.subscribe(&[]);
        let runtime = RoleToolRuntime::with_bus(bus);
        let source_event_id = mmat_event_stream::event::EventId::new();

        let result = CreateLaneTool
            .call(
                &runtime,
                serde_json::json!({
                    "name": "Child lane",
                    "kind": "conversation",
                    "purpose": "Explore a branch",
                    "project_id": "project-1",
                    "current_lane_id": "lane-parent",
                    "parent_lane_id": "lane-parent",
                    "source_event_id": source_event_id.to_string(),
                    "source_message_id": "message-1"
                }),
            )
            .await
            .unwrap();
        assert!(
            result["lane_id"]
                .as_str()
                .unwrap()
                .starts_with("lane-conversation-")
        );

        let event = receiver.recv().await.unwrap();
        match event.as_ref() {
            SemanticEvent::LaneCreated {
                parent_lane_id,
                source_event_id: actual_source_event_id,
                source_message_id,
                ..
            } => {
                assert_eq!(event.context().project_id, "project-1");
                assert_eq!(event.context().lane_id.as_deref(), Some("lane-parent"));
                assert_eq!(parent_lane_id.as_deref(), Some("lane-parent"));
                assert_eq!(*actual_source_event_id, Some(source_event_id));
                assert_eq!(source_message_id.as_deref(), Some("message-1"));
            }
            other => panic!("expected LaneCreated, got {}", other.variant_name()),
        }
    }
}
