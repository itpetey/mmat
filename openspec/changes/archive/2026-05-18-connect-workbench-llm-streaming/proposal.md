## Why

Workbench chat currently accepts lane-scoped human input, persists it through workbench code, and then immediately reports `LLM streaming is not connected to the workbench runtime yet.` The workbench is using a local event bus instead of a shared `OrganisationRuntime` boundary, so UI chat does not exercise the runtime-backed assistant flow that the existing workbench/runtime integration specs require.

## What Changes

- Replace the placeholder `AssistantStreamUnavailable` response with an assistant stream connected through a shared `OrganisationRuntime` or narrow runtime facade for normal lane chat messages.
- Move accepted lane-scoped chat input onto the runtime event bus boundary instead of relying on the workbench-local bus as the integration point.
- Persist assistant output as semantic events so reloads and lane projections show the same response history as live clients.
- Forward live assistant deltas/completion/errors over the existing workbench chat WebSocket using stable message IDs tied to the originating user message.
- Keep validation behaviour for blank messages, missing lanes, archived lanes, and persistence failures unchanged.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `workbench-runtime-integration`: Workbench chat must dispatch accepted lane-scoped input to the runtime/LLM path instead of reporting that streaming is unavailable.
- `workbench-event-stream`: Workbench projections must include persisted assistant responses produced by runtime-backed chat streaming.
- `semantic-event-types`: Semantic events must include a durable assistant message event suitable for lane-scoped runtime replies.
- `runtime`: `OrganisationRuntime` must expose an embeddable workbench boundary for publishing lane-scoped input, subscribing to runtime events, and coordinating assistant stream lifecycle.

## Impact

- Affects `crates/workbench/src/api/chat.rs` WebSocket command handling and transcript projection.
- Affects `crates/coordinator/src/runtime.rs` or adjacent coordinator APIs to provide a long-lived runtime handle/facade for the workbench server.
- Requires replacing the workbench-local event bus integration point with the runtime-owned event bus or an adapter around it, while avoiding duplicate event persistence.
- Extends chat server/client message contracts to represent assistant stream lifecycle updates.
- Adds tests around successful assistant streaming, persistence/projection, and runtime/LLM error reporting.
