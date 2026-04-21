## 1. Add Dioxus dependencies and scaffold `/web` crate

- [x] 1.1 Add `dioxus` (with `liveview`, `server`, `web` features) and `dioxus-liveview` to root `Cargo.toml` workspace dependencies
- [x] 1.2 Create `/web/Cargo.toml` as a path-dependency crate with `dioxus` (LiveView/server features)
- [x] 1.3 Create `/web/src/lib.rs` with a basic Dioxus `App` component stub
- [x] 1.4 Add `/web` as a `path` dependency in root `Cargo.toml` under `[dependencies]`
- [x] 1.5 Verify `cargo check` passes with the new crate structure

## 2. Define `UiState` and `UiEvent` types

- [x] 2.1 Create `src/ws/ui_state.rs` with `UiEvent` enum mirroring `FrontendEvent` variants (Log, StepStarted, StepCompleted, StepFailed, ComponentStarted, ComponentCompleted, ComponentFailed, PlanningTriggered)
- [x] 2.2 Define `UiState` struct with `event_history: Mutex<VecDeque<UiEvent>>`, `pending_initial_input: Mutex<Option<oneshot::Sender<String>>>`, `pending_prompt: Mutex<Option<PendingPrompt>>`, `run_summary: Mutex<Option<RunSummary>>`, and `planning_started: Mutex<bool>`
- [x] 2.3 Define `PendingPrompt` struct with `question: String`, `choices: Option<Vec<String>>`, `reply: oneshot::Sender<String>`
- [x] 2.4 Implement `UiState::push_event()` with bounded cap of 256 entries
- [x] 2.5 Implement `UiState::snapshot()` method returning a clone of current state for LiveView rendering
- [x] 2.6 Re-export `UiState` and `UiEvent` from `src/ws/mod.rs`

## 3. Retarget `WsLayer` to push into `UiState`

- [x] 3.1 Keep `WsLayer` unchanged (still feeds `FrontendEvent` into the channel) and add a separate `spawn_event_translator` task
- [x] 3.2 Create a background task that receives `FrontendEvent` from the existing channel and translates each variant into `UiState::push_event()` calls
- [x] 3.3 Map `FrontendEvent::StepStarted { task_name: "planning", .. }` to also set `planning_started = true`
- [x] 3.4 Tracing events from `workflow::logging.rs` and `workflow::execution.rs` flow into `UiState` via the existing `WsLayer` → channel → translator pipeline

## 4. Rewrite `src/ws/server.rs` as LiveView host

- [x] 4.1 Remove the current `WsEvent`/`WsClientMessage` websocket handler and broadcast logic
- [x] 4.2 Replace with `dioxus_liveview::LiveViewRouter` setup using Axum adapter
- [x] 4.3 Add a `with_ui_state()` method to `WsAppBuilder` accepting `Arc<UiState>`
- [x] 4.4 Register the LiveView route at `/` using `dioxus_liveview::interpreter_glue("/")` via `with_virtual_dom`
- [x] 4.5 CSS is inlined via `document::Style` in the Dioxus root component (no separate route needed)
- [x] 4.6 Keep the `WsHandle` and shutdown semantics compatible with existing usage

## 5. Build the Dioxus LiveView UI in `src/ws/server.rs`

- [x] 5.1 Create `RootApp` component that reads `UiState` via props
- [x] 5.2 Implement the chat layout component: full-height flex column with scrollable transcript area and fixed bottom composer
- [x] 5.3 Implement transcript rendering that iterates over `UiState.event_history` and renders each `UiEvent` variant
- [x] 5.4 Implement initial prompt card: renders when `pending_initial_input` is `Some`, with text input and submit button
- [x] 5.5 Implement human prompt card: renders when `pending_prompt` is `Some`, with question text and reply input
- [x] 5.6 Implement log entry rendering with level-appropriate text formatting
- [x] 5.7 Implement step event rendering as compact status markers
- [x] 5.8 Connection status is implicit via LiveView websocket (browser shows disconnection natively)
- [x] 5.9 Wire up submit actions to complete the stored oneshot senders in `UiState`

## 6. Add plain CSS stylesheet

- [x] 6.1 Create `/web/style.css` with basic chat layout styles
- [x] 6.2 CSS classes for log levels (inlined via `document::Style`)
- [x] 6.3 CSS classes for step markers (inlined via `document::Style`)
- [x] 6.4 CSS classes for prompt cards (inlined via `document::Style`)
- [x] 6.5 CSS is inlined into the LiveView HTML page via `document::Style` component

## 7. Adapt `AppRuntime` prompt/input plumbing

- [x] 7.1 Modify `AppRuntime` to accept a `UiState` reference alongside `EventSender`
- [x] 7.2 `AppRuntime::ask()` still sends `FrontendEvent::HumanPrompt` through the channel; the translator writes it into `UiState.pending_prompt`
- [x] 7.3 Initial input flow uses `UiState.pending_initial_input` set by `spawn_server_with_input`
- [x] 7.4 `write_run_summary()` also updates `UiState.run_summary`

## 8. Update `run_interactive()` in `src/main.rs`

- [x] 8.1 Create shared `UiState` instance at the start of `run_interactive()`
- [x] 8.2 Pass `UiState` to both the LiveView host builder and the `AppRuntime`
- [x] 8.3 Start the event translation task (FrontendEvent -> UiState) before running the workflow
- [x] 8.4 If no `--prompt` flag, store the initial input oneshot in `UiState` (done by `spawn_with_input`)
- [x] 8.5 Print the server URL to stdout
- [x] 8.6 Run the workflow as before
- [x] 8.7 Replace the immediate shutdown with `tokio::signal::ctrl_c()` wait
- [x] 8.8 On Ctrl+C, send `FrontendEvent::Quit` and await graceful server shutdown

## 9. Remove obsolete websocket protocol code

- [x] 9.1 Keep `src/ws/event.rs` but remove `WsEvent` and `WsClientMessage` types (only `FrontendEvent` remains)
- [x] 9.2 Remove `WsEvent` and `WsClientMessage` imports from `src/ws/server.rs`
- [x] 9.3 Remove JSON serialization/deserialization logic for the old protocol
- [x] 9.4 Clean up dead code paths in `src/ws/mod.rs`

## 10. Update docs and CLI help

- [x] 10.1 Update `README.md` to describe browser-first usage and remove TUI references
- [x] 10.2 Update CLI help text in `src/main.rs` to reflect LiveView UI instead of TUI
- [x] 10.3 Error messages already reference the browser UI (no TUI references remain in error paths)

## 11. Verify end to end

- [x] 11.1 Run `cargo fmt --all` and verify formatting passes
- [x] 11.2 Run `cargo clippy -- -D warnings` and fix all warnings
- [x] 11.3 Run `cargo test` and ensure all tests pass
- [ ] 11.4 Manual test: `cargo run` with no prompt, open browser, submit initial prompt, answer clarification/approval prompts, confirm planning/execution logs stream
- [ ] 11.5 Manual test: refresh browser tab and confirm transcript recovers from bounded history
- [ ] 11.6 Manual test: Ctrl+C terminates server cleanly
