## 1. Semantic Events and Projection

- [x] 1.1 Add `AssistantMessageProduced` to `SemanticEvent`, `EventType`, constructors, variant-name mapping, context accessors, and serialization tests.
- [x] 1.2 Project `AssistantMessageProduced` into workbench transcripts with assistant speaker, message kind, lane filtering, and stable event IDs.
- [x] 1.3 Add tests proving assistant message events serialize, replay, and appear only in their matching lane transcript.

## 2. Runtime Integration Boundary

- [x] 2.1 Add or adapt a coordinator-owned runtime facade/handle around `OrganisationRuntime` for workbench use.
- [x] 2.2 Expose durable append/publish operations that make workbench-originated events durable exactly once before acknowledgement.
- [x] 2.3 Expose runtime event subscription for workbench WebSocket connections without using the workbench-local bus as the authoritative integration point.
- [x] 2.4 Expose assistant stream start and cancellation operations through the runtime facade.
- [x] 2.5 Add runtime/workbench configuration fields for LLM API key, base URL, model, and timeout using workspace dependencies and environment/config-file loading.
- [x] 2.6 Return a recoverable chat error when required runtime or LLM configuration is missing or invalid.

## 3. WebSocket Streaming Contract

- [x] 3.1 Replace the successful-path `AssistantStreamUnavailable` protocol with assistant stream started, delta, completed, and failed server messages.
- [x] 3.2 Update the Dioxus chat reducer to create assistant rows on start, append deltas by assistant message ID, mark completion, and show stream failures without duplicating persisted updates.
- [x] 3.3 Track in-flight assistant streams by message ID so cancellation routes through the runtime facade, stops further deltas, and acknowledges the cancellation.

## 4. Runtime Streaming Flow

- [x] 4.1 Dispatch accepted lane chat messages through the runtime facade after workbench validation succeeds.
- [x] 4.2 Use the runtime facade to persist and publish `HumanFeedbackReceived` exactly once before sending `UserMessageAccepted`.
- [x] 4.3 Forward runtime assistant streaming content deltas to the socket while assembling the final assistant text in memory.
- [x] 4.4 Persist the final assistant message event through the runtime facade, broadcast it after durability is established, and send completion only after persistence succeeds.
- [x] 4.5 Send a stream failure and avoid completed assistant persistence when the runtime stream, socket, cancellation, or final durable append fails.

## 5. Verification

- [x] 5.1 Add unit tests for runtime facade durable publish sequencing and duplicate-persistence prevention.
- [x] 5.2 Add unit tests for assistant stream message sequencing, missing configuration, cancellation, failed stream, and failed final persistence.
- [x] 5.3 Add UI reducer tests or component-level coverage for streamed assistant row merging and duplicate suppression.
- [x] 5.4 Run `cargo fmt --all`.
- [x] 5.5 Run `cargo clippy -- -D warnings`.
- [x] 5.6 Run `cargo test`.
