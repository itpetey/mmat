## Context

The current UI is intentionally minimal and event-native. As more runtime behaviour becomes live, users need stronger affordances to understand what is happening and what they can do next.

## Goals / Non-Goals

**Goals:**
- Make pending questions/action requests visually distinct.
- Add clear running/loading/error/reconnect states.
- Support mobile and keyboard-driven use.
- Improve accessibility and message rendering.

**Non-Goals:**
- Replacing the established dark visual language.
- Building a full design system in this change.

## Decisions

- Preserve the current Slack/Discord-inspired dark channel feel.
- Add progressive disclosure: summary first, raw event details on click.
- Treat mobile as first-class for chat; DAG may collapse into a stacked inspector.

## Risks / Trade-offs

- Markdown rendering can obscure exact payloads. Mitigation: keep raw event/artefact JSON available in inspectors.
- More controls can clutter chat. Mitigation: keep action controls inline and context-specific.
