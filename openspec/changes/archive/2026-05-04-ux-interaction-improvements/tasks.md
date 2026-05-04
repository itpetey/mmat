## 1. International English Directive

- [x] 1.1 Add `ENGLISH_DIRECTIVE` constant to `src/plan/mod.rs` with International English instruction text
- [x] 1.2 Append `ENGLISH_DIRECTIVE` to system prompts in `src/plan/discovery.rs` (`SYSTEM_PROMPT`, `DIVERGENT_SYSTEM_PROMPT`, and their builder functions)
- [x] 1.3 Append `ENGLISH_DIRECTIVE` to system prompts in `src/plan/solutions.rs` (`BRANCH_SYSTEM_PROMPT`, `COLLECT_SYSTEM_PROMPT`)
- [x] 1.4 Append `ENGLISH_DIRECTIVE` to system prompt in `src/plan/knowledge.rs` (`SYSTEM_PROMPT`)
- [x] 1.5 Append `ENGLISH_DIRECTIVE` to system prompt in `src/plan/architect.rs` (`SYSTEM_PROMPT`)
- [x] 1.6 Append `ENGLISH_DIRECTIVE` to system prompts in `src/deliver/engine.rs` (planning, implementation, peer review, contract validation, final review)

## 2. Step Interrupt — Backend

- [x] 2.1 Add `CancellationToken` field to `UiState` (using `tokio_util::sync::CancellationToken` or `watch<bool>`)
- [x] 2.2 Add `InterruptStep` variant to `FrontendEvent` enum in `src/liveview/event.rs`
- [x] 2.3 Add `interrupt_step()` method to `UiState` that sets the cancellation token
- [x] 2.4 Add `step_interrupted()` method to `UiState` that returns whether the token is cancelled
- [x] 2.5 Add `reset_interrupt()` method to `UiState` that resets the cancellation token for the next step
- [x] 2.6 Wire the cancellable token into the workflow loop in `src/bin/frontend.rs` — check token between LLM turns in `run_workflow_when_prompted`
- [x] 2.7 Add `StepInterrupted` status to `RunSummary` and emit it via `FrontendEvent::RunSummary` when interrupted
- [x] 2.8 Handle interrupt in the event translator (`src/liveview/translator.rs`) to update composer mode back to `Reply`

## 3. Step Interrupt — Frontend

- [x] 3.1 Add `interrupt_pending: bool` and `interrupt_pressed_at: Option<Instant>` fields to the Composer component state in `src/liveview/components.rs`
- [x] 3.2 Add `onkeydown` handler to the composer that detects Escape presses: first press shows "Esc again to interrupt", second press within 3 seconds sends `InterruptStep`
- [x] 3.3 Add interrupt confirmation message rendering in the Composer component ("Esc again to interrupt")
- [x] 3.4 Render interrupted-step indicator in conversation when `ComposerMode` is `Reply` after an interrupt
- [x] 3.5 Clear `interrupt_pending` state on any non-Escape keypress or after 3-second timeout

## 4. Message Queue — Backend

- [x] 4.1 Add `message_queue: VecDeque<String>` to `ProjectUiState` in `src/liveview/state.rs`
- [x] 4.2 Add `message_queue_count: usize` to `UiSnapshot` struct
- [x] 4.3 Add `queue_message(project_id, text)` method to `UiState` that pushes to `message_queue`
- [x] 4.4 Add `drain_message_queue(project_id) -> Vec<String>` method to `UiState` that returns and clears the queue
- [x] 4.5 Update `snapshot()` to include `message_queue_count` from the active project's queue
- [x] 4.6 Add `MessageQueued` variant to `FrontendEvent` so the translator can record queued messages in conversation history
- [x] 4.7 Add `ConversationEntry::QueuedUserMessage { text: String }` variant for displaying queued messages distinctly
- [x] 4.8 Wire queue flushing in `run_workflow_when_prompted`: after an `AssistantResponseCompleted` event, drain the queue and concatenate messages into a user turn

## 5. Message Queue — Frontend

- [x] 5.1 Update Composer component to remain interactive when `ComposerMode == Working` and route submit to `queue_message` instead of `send_pending_prompt`
- [x] 5.2 Render queued message count indicator near the composer (e.g., "2 messages queued")
- [x] 5.3 Render `ConversationEntry::QueuedUserMessage` entries in the conversation view with dimmed styling
- [x] 5.4 On step completion (mode transitions from `Working` to `Reply`), clear the queue indicator

## 6. First-Boot Project Creation

- [x] 6.1 Add `is_first_boot: bool` to `UiSnapshot` derived from whether the project list is empty
- [x] 6.2 Add `NewProjectForm` component to `src/liveview/components.rs` with fields for project name and root directory
- [x] 6.3 Add form validation: project name must be non-empty and contain only alphanumeric/underscore characters
- [x] 6.4 Wire form submission to `UiState::register_project` and transition to composer on success
- [x] 6.5 Conditionally render `NewProjectForm` instead of `Composer` in `RootApp` when `is_first_boot` is true
- [x] 6.6 Remove `ensure_default_project` call from `src/bin/frontend.rs` startup
- [x] 6.7 Update `UiState::with_projects` and `UiState::with_projects_and_conversation_store` to handle empty project lists gracefully (no default project fallback)

## 7. Integration and Polish

- [x] 7.1 Run `cargo fmt --all` and `cargo clippy -- -D warnings`
- [x] 7.2 Run `cargo test` and fix any failures
- [x] 7.3 Verify Escape interrupt works end-to-end: start a long step, press Escape twice, confirm step stops
- [x] 7.4 Verify message queue: start a step, queue 2 messages, confirm they flush after step completes
- [x] 7.5 Verify first-boot: delete the project registry, restart, confirm creation form appears