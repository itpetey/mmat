## Why

MMAT currently has no interactive web frontend. The local websocket server in `src/ws` emits structured events but there is no browser UI to consume them. Users need a way to interact with MMAT workflows through a browser: submit the initial prompt, answer clarification questions, approve proposals and contracts, and watch planning/execution logs stream in real time.

## What Changes

- Replace the manual websocket protocol server in `src/ws/server.rs` with a Dioxus LiveView host that serves the UI and manages websocket connections
- Add a shared `UiState` / `UiBus` that captures workflow events, pending prompts, and a bounded event transcript
- Create `/web` as a Rust subcrate containing the Dioxus LiveView UI components
- Retarget `FrontendEvent` and `WsLayer` to push into shared UI state instead of broadcasting JSON over websockets
- Change `run_interactive()` to keep the server alive until Ctrl+C instead of shutting down after workflow completion
- Remove the custom `WsEvent` / `WsClientMessage` wire protocol types once LiveView replaces them
- Use plain CSS for styling (no Tailwind build step)

## Capabilities

### New Capabilities

- `liveview-host`: Dioxus LiveView server hosting the browser UI with websocket transport
- `ui-state-bus`: Shared in-memory state for workflow events, pending prompts, and bounded event history
- `web-chat-ui`: Browser-based chat interface with prompt/reply flow and log streaming

### Modified Capabilities

- `interactive-runtime`: The interactive runtime entrypoint changes from websocket-event-broadcast to LiveView-hosted UI with shared state

## Impact

- `src/ws/server.rs`: Rewritten to host Dioxus LiveView instead of custom websocket protocol
- `src/ws/event.rs`: Wire protocol types removed after migration
- `src/ws/layer.rs`: Retargeted to push into `UiState` instead of websocket broadcast
- `src/runtime.rs`: Prompt/input plumbing adapted to shared state model
- `src/main.rs`: Interactive entrypoint updated for LiveView lifecycle
- `Cargo.toml`: Dioxus dependencies added, unused websocket deps removed
- `/web/`: New Rust subcrate for Dioxus UI components
- `README.md`: Updated for browser-first usage
