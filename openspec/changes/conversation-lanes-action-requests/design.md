## Context

The current chat is a single global stream. The desired UX is more like Slack channels/threads plus tag filters: lanes isolate attention in the UI but do not isolate project memory or runtime knowledge.

## Goals / Non-Goals

**Goals:**
- Represent lanes with identity, colour, kind, purpose, parent/related lanes, and origin message.
- Tag messages/events with primary and related lanes.
- Let the user view one lane, selected related lanes, or all project activity.
- Let LLM tools create lanes when a conversation scopes out a new feature or thread.
- Make notifications deep-link to lane-scoped messages or DAG nodes.

**Non-Goals:**
- Hard memory isolation per lane.
- Multi-user channel permissions.
- Requiring the Librarian or any role to validate every action with the user.

## Decisions

- Lanes are filters/provenance metadata, not silos.
- Each message gets an addressable `message_id` and optional `primary_lane_id` plus `related_lane_ids`.
- Notifications reference `message_id`, `lane_id`, or DAG node IDs instead of opening detached modal workflows.
- Add a `create_lane` LLM tool with name, kind, purpose, parent lane, related lanes, and source message fields.
- Use action requests for inline choices such as approval, clarification, memory validation, review decision, or destructive confirmation.

## Risks / Trade-offs

- Lane tagging may become noisy. Mitigation: default inherited lane from current view and make related tags optional.
- LLM-created lanes can proliferate. Mitigation: require purpose/source metadata and expose archive/merge controls later.
- Action requests can feel like forms. Mitigation: render them as chat messages with lightweight inline controls.
