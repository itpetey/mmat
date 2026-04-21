## Context

MMAT currently has a local websocket server (`src/ws/server.rs`) that broadcasts `WsEvent` messages and accepts `WsClientMessage` replies. There is no browser UI yet. The runtime (`src/runtime.rs`) emits `FrontendEvent` through a tracing layer (`src/ws/layer.rs`) and oneshot channels for prompt replies. Interactive mode (`run_interactive()`) currently starts the websocket server, runs the workflow, then immediately shuts down the server.

The goal is to replace the custom websocket protocol with Dioxus LiveView, which runs the UI server-side and syncs DOM updates over a websocket. This eliminates the bespoke `WsEvent`/`WsClientMessage` wire protocol while keeping the internal event flow largely intact.

## Goals / Non-Goals

**Goals:**
- Provide a browser-based chat UI for MMAT workflows
- Replace the custom websocket protocol with Dioxus LiveView hosting
- Maintain the existing prompt/reply semantics (initial input, clarification, approval, contract)
- Stream logs, step events, and component events into the chat transcript
- Support bounded in-memory transcript recovery on reconnect
- Keep the server alive until Ctrl+C

**Non-Goals:**
- No Tailwind or Node-based CSS build; plain CSS only
- No persisted session restore beyond bounded in-memory history
- No server-side rendering or hydration; LiveView owns the rendering model
- No multi-user or multi-session support; single browser tab per MMAT process

## Decisions

### Dioxus LiveView as the UI transport

LiveView runs the Dioxus VirtualDom server-side and syncs DOM diffs over websocket. This replaces the custom `WsEvent`/`WsClientMessage` protocol entirely. The browser only loads a minimal HTML page with Dioxus's `interpreter_glue` script.

**Alternatives considered:**
- Dioxus web/WASM with manual websocket client: more boilerplate, less benefit over current approach
- Keep custom protocol + separate frontend: defeats the purpose of reducing transport complexity

### Shared `UiState` as the single source of truth

A `parking_lot::RwLock<UiState>` lives behind the LiveView host and is shared with:
- `WsLayer` receiver (pushes tracing events)
- `AppRuntime` prompt/input plumbing (writes pending prompts, reads replies)
- LiveView components (read state, trigger user actions)

`UiState` contains:
- `event_history`: bounded `VecDeque<UiEvent>` for transcript replay
- `pending_initial_input`: `Option<oneshot::Sender<String>>`
- `pending_prompt`: `Option<PendingPrompt>` with question, choices, and reply sender
- `run_summary`: latest `RunSummary` snapshot
- `planning_started`: flag for UI mode switching

**Alternatives considered:**
- `tokio::sync::broadcast` channel: loses history, harder to snapshot
- External store (SQLite): overkill for single-process, local-first use case

### `/web` as a Rust subcrate

The Dioxus UI lives in `/web` as a separate Cargo crate. MMAT's root `Cargo.toml` adds it as a path dependency. This keeps frontend source in `/web/` while remaining a pure Rust build.

### Retain `FrontendEvent` as internal event type

`FrontendEvent` stays as the internal event enum emitted by `WsLayer`. The receiver translates these into `UiEvent` entries pushed into `UiState`. This avoids rewriting the tracing layer.

### Bounded event history (cap: 256 events)

The `event_history` is a `VecDeque` capped at 256 entries. When a new LiveView session connects, it receives the current transcript. Older events are dropped. This is sufficient for a typical MMAT session and avoids unbounded memory growth.

### Server lifecycle: stay up until Ctrl+C

`run_interactive()` will no longer shut down the server after workflow completion. Instead it waits for `tokio::signal::ctrl_c()` and then sends `FrontendEvent::Quit` to gracefully terminate.

## Risks / Trade-offs

[LiveView session is tied to browser tab; refresh loses connection] → Mitigation: bounded in-memory transcript replay on reconnect. The `UiState` lives outside the LiveView session.

[Server-side UI state couples backend and presentation] → Mitigation: `UiState` is a clean boundary. Workflow code only pushes events; it does not render.

[Dioxus LiveView `VirtualDom` is `!Send`] → Mitigation: Dioxus provides `LiveViewPool` for spawning `!Send` VirtualDoms on websocket connections.

[Long-running workflows may outgrow bounded transcript] → Mitigation: 256 events is sufficient for typical sessions. If needed, the cap can be increased or made configurable.

[Dioxus 0.7 ecosystem maturity] → Mitigation: Dioxus 0.7 is stable with active development. The LiveView Axum adapter is well-tested.
