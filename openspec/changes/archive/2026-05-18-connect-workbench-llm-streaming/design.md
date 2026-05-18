## Context

The current workbench chat WebSocket validates and persists lane-scoped user messages as `HumanFeedbackReceived` events, broadcasts them on a workbench-local event bus, acknowledges the client, and then sends `AssistantStreamUnavailable` with the message `LLM streaming is not connected to the workbench runtime yet.` The UI renders that server message as a system log row, so accepted chat input cannot produce live assistant output.

`OrganisationRuntime` in `crates/coordinator/src/runtime.rs` already owns the runtime event bus, event store hydration/replay, scheduler, roles, memory store, artefact store, and lifecycle events. The workbench currently sits beside that runtime instead of attaching to it. Fixing the UI error should therefore connect workbench chat to a long-lived runtime boundary, not create a parallel workbench-only LLM path that bypasses runtime ownership.

The repository also contains an OpenAI-compatible LLM client with a streaming API, persisted events through `mmat-db`, lane-scoped `EventContext`, and transcript projection from semantic events. The missing piece is an embeddable runtime/workbench boundary that accepts persisted lane input, exposes runtime events to WebSocket clients, streams assistant deltas, and persists completed assistant output once.

## Goals / Non-Goals

**Goals:**

- Replace the hard-coded unavailable response with a real assistant stream connected through `OrganisationRuntime` or a narrow runtime facade owned by the coordinator crate.
- Use one runtime event bus boundary for workbench chat input and runtime output instead of the current workbench-local bus as the integration point.
- Preserve persistence ordering and avoid duplicate writes: accepted user input and completed assistant output must be appended exactly once before being treated as durable.
- Add durable semantic representation for assistant replies so reloads and lane projections match live WebSocket output.
- Keep the WebSocket protocol explicit about assistant stream start, delta, completion, cancellation, and failure states.
- Make missing or failing runtime/LLM configuration a recoverable chat error, not a fake assistant reply.

**Non-Goals:**

- Implement full multi-role task orchestration or tool-call execution from arbitrary chat prompts in this first pass.
- Convert every existing role to token-streaming output.
- Change lane creation/archive behaviour or System lane rules.
- Store partial assistant deltas durably before completion.
- Build a new front-end chat architecture beyond consuming the additional stream messages.

## Decisions

1. Add a long-lived runtime facade for workbench integration.

   The workbench server should initialise or receive a shared `OrganisationRuntime`-backed handle during startup. That handle should expose the runtime event bus subscription surface, durable append/publish helpers, and an assistant streaming entry point for accepted lane chat messages. This keeps the workbench attached to the same bus and stores used by scheduler/roles while avoiding the mistake of constructing a fresh `OrganisationRuntime` per WebSocket connection or per message.

   Alternative considered: call `mmat_llm::OpenAiClient::complete_streaming` directly from `chat.rs`. That would remove the UI error quickly, but it would leave the workbench bypassing `OrganisationRuntime`, duplicate runtime concerns in the workbench crate, and make future role/scheduler integration harder.

2. Treat direct LLM streaming as a runtime adapter, not a workbench service.

   The first implementation can still use the existing streaming LLM client to produce assistant deltas, but that code should live behind the coordinator-owned runtime facade. The facade can later route the same request through a role, scheduler task, or richer orchestration path without changing the workbench WebSocket contract.

   Alternative considered: publish a `TaskAssigned` event and wait for role output immediately. That aligns with the eventual multi-role architecture, but current roles use non-streaming `LlmClient::complete` and do not expose token deltas to the workbench. A narrow runtime adapter is the pragmatic bridge.

3. Make event persistence ownership explicit.

   `OrganisationRuntime::run` already has a persistence subscriber that appends events published on the runtime bus. The workbench path must avoid both manually appending an event and publishing it to a persistence subscriber that appends it again. The runtime facade should provide one operation for durable workbench-originated input: append first, then publish to runtime subscribers as already-durable, or publish through a runtime persistence path that can acknowledge durability before the WebSocket user acknowledgement.

   Alternative considered: keep `chat.rs` manual append and publish on its local bus. That preserves current behaviour but does not connect the runtime and keeps the root cause in place.

4. Introduce a semantic assistant message event.

   Persist completed assistant replies as a new lane-scoped semantic event rather than overloading `HumanFeedbackRequested`. Human feedback requests have action-request semantics and may render as pending inline decisions; normal assistant replies need a durable transcript event with reply provenance and content.

   Alternative considered: store the final assistant message only in the WebSocket client state. That would regress replayable projections and make reloads lose assistant output.

5. Extend the chat WebSocket protocol with stream lifecycle messages.

   Replace `AssistantStreamUnavailable` in the successful path with messages equivalent to started, delta, completed, and failed. The client can create one assistant row on start, append deltas by `message_id`, mark completion when final persistence succeeds, and show recoverable errors without duplicating user messages.

   Alternative considered: send only final assistant messages. This would be simpler but would not satisfy the streaming UI problem and would waste the existing streaming LLM client support.

6. Persist only completed assistant replies in the first pass.

   Deltas remain live WebSocket updates. On completion, append a single assistant message event containing the assembled content, `reply_to_message_id`, lane context, and source role. If the stream fails, the client receives a failure message and no assistant event is persisted unless some later design adds partial transcript recovery.

   Alternative considered: append an event per delta. That would increase event volume and projection complexity without adding durable value for the current workbench transcript.

7. Use explicit runtime LLM configuration.

   Runtime/workbench startup should configure API key, base URL, model, and timeout for the assistant streaming adapter. If configuration is absent, sending a lane message should return a recoverable `Error` or `AssistantStreamFailed` explaining the missing configuration. The old hard-coded unavailable message should not be used as the normal path.

## Risks / Trade-offs

- Runtime bus persistence currently appends published events asynchronously -> define an acknowledged durable publish/append path for workbench-originated messages before sending user or assistant completion acknowledgements.
- LLM stream succeeds but assistant event persistence fails -> send a stream failure after deltas and do not mark the assistant row complete; Postgres remains the source of truth.
- WebSocket disconnects during streaming -> cancel the in-flight runtime stream task and rely on already persisted user input; do not persist incomplete assistant output.
- Adding a new semantic event requires projection and serialization updates -> cover constructor, event type mapping, transcript projection, and database replay tests.
- A narrow runtime adapter may not exercise full role orchestration yet -> keep it behind the coordinator-owned facade so later routing changes do not alter the workbench contract.
- Missing runtime/LLM settings could look like the old bug -> expose a specific configuration error so users can distinguish setup failure from unimplemented streaming.

## Migration Plan

1. Add the semantic assistant message event type and transcript projection support.
2. Add or adapt a coordinator-owned runtime facade/handle that exposes durable append/publish, runtime event subscription, assistant streaming, and cancellation for the workbench.
3. Wire workbench server startup to initialise and share the runtime handle instead of relying on the workbench-local `WORKBENCH_BUS` as the integration boundary.
4. Add runtime LLM configuration for API key, base URL, model, and timeout using workspace dependencies.
5. Add chat WebSocket stream lifecycle messages and update the Dioxus chat UI reducer to merge deltas by assistant message ID.
6. Dispatch accepted lane chat messages through the runtime facade after validation and use the facade for durable user-message acknowledgement, assistant streaming, final assistant persistence, and runtime event broadcast.
7. Add tests for event projection, runtime facade sequencing, WebSocket message sequencing, missing configuration, stream failure, cancellation, and successful final persistence.
8. Roll back by disabling the runtime facade call and leaving validation/persistence paths unchanged; no migration is needed for existing events because the new event is additive.

## Open Questions

- Should the runtime facade append workbench-originated events before publishing, or should `OrganisationRuntime`'s persistence subscriber grow an acknowledgement path for durable publish?
- Which exact default model should the runtime use when configuration omits a model value?
- Should the first implementation use the existing OpenCode key setting as an LLM API key, or add a separate LLM-specific configuration field?
