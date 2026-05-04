## Context

MMAT is a multi-stage AI planning and delivery tool with a LiveView web frontend (Dioxus). The frontend communicates with backend workflow stages via an event channel (`FrontendEvent`) and instruction channel (`ProjectPrompt`). Currently:

- There is no mechanism to interrupt a running LLM step — the only option is terminating the process.
- User input is blocked while a step runs; the composer enters `Working` mode and all keystrokes are ignored until the step completes.
- On startup, `ensure_default_project` silently creates a "default" project. Users are never given the opportunity to name or configure their first project.
- System prompts across `plan/` and `deliver/` stages are inconsistent about language, sometimes producing American English.

The `ChannelHumanIO` from `naaf_llm` provides an `OpenAiStreamObserver` for streaming events but no cancellation or mid-turn message injection. The `FrontendEvent` enum carries state from backend to frontend only — there is no reverse channel for interrupt or queued-message signals beyond `PendingPrompt.reply`.

## Goals / Non-Goals

**Goals:**
- Allow users to cancel/abort a running step with a deliberate double-press of Escape
- Enable user steering by queuing messages during a running step and flushing them on next LLM turn
- Prompt users to create a project on first boot instead of silently creating a default one
- Enforce International English across all LLM system prompts

**Non-Goals:**
- Throttling or debouncing Escape for any purpose other than step interruption
- Streaming partial cancellation messages to the LLM mid-request
- Persisting queued messages across browser tab reloads
- Supporting project import or migration from other tools
- Changing the Oxford/International English rules themselves (they are dictated by the AGENTS.md convention)

## Decisions

### 1. Double-press Escape with 3-second window

**Decision**: Track Escape presses in frontend state. On first press, show "Esc again to interrupt". On second press within 3 seconds, send an `InterruptStep` signal to the backend. Reset the timer after 3 seconds of no second press.

**Rationale**: Single-press Escape is prone to accidental triggers during normal typing. A double-press with a time window gives confidence the user intended cancellation. 3 seconds is long enough for a deliberate second press but short enough that a stale first press doesn't linger.

**Alternative considered**: `Ctrl+C` — conflicts with terminal copy. Dedicated button — requires mouse interaction; keyboard-first users benefit from Escape.

### 2. Interrupt via `tokio::sync::watch` cancellation channel

**Decision**: Add a `CancellationToken` (from `tokio_util` or a custom `watch<bool>`) to the `UiState`. When the frontend sends an `InterruptStep` instruction, the state sets the token. The running workflow polls this token between LLM turns. When cancelled, the workflow step returns early with a `StepInterrupted` result.

**Rationale**: Using a shared cancellation token avoids coupling the frontend directly to the workflow runtime. The workflow already runs in a `tokio::spawn` context (see `run_workflow_when_prompted` in `bin/frontend.rs`). Tokens are checkable between turns without modifying the streaming API.

**Alternative considered**: `JoinHandle::abort()` — too aggressive; leaves conversation state inconsistent. Sending a special `FrontendEvent` — events flow backend→frontend, not reverse.

### 3. Message queue as `VecDeque<String>` on `UiState`

**Decision**: Add a `message_queue: Mutex<VecDeque<String>>` to `ProjectUiState`. While a step is running (`composer_mode == Working`), the composer accepts text and on submit pushes to this queue instead of sending immediately. When a step completes (frontend receives `AssistantResponseCompleted` or tool call completes), the queue is flushed: queued messages are concatenated and sent as the next user turn to the LLM.

**Rationale**: VecDeque is simple, bounded, and already used for conversation history. Flushing on step completion (not mid-stream) avoids disrupting in-progress token streams.

**Alternative considered**: Per-message individual flushing — adds complexity when multiple messages arrive in quick succession; grouping produces a coherent turn.

### 4. First-boot project creation form replaces `ensure_default_project`

**Decision**: On startup, if the project registry is empty, the frontend renders a `NewProjectForm` component instead of the composer. The form collects a project name and root directory path. On submit, `UiState::register_project` creates the project, then the normal composer flow begins.

**Rationale**: Removing `ensure_default_project` from the startup path and replacing it with an explicit user action gives the user control from the very first interaction. The form reuses the existing `register_project` path on `UiState`.

**Alternative considered**: Modal dialog over the normal view — adds unnecessary overlay complexity for a one-time action.

### 5. International English directive appended to every system prompt

**Decision**: Add a constant `ENGLISH_DIRECTIVE: &str` containing the International English instruction and append it to every system prompt constant in `plan/discovery.rs`, `plan/solutions.rs`, `plan/knowledge.rs`, `plan/architect.rs`, and `deliver/engine.rs`.

**Rationale**: A single shared constant keeps the wording consistent. Appending (not prepending) preserves prompt structure and keeps the domain-specific instructions first.

**Alternative considered**: Separate system message — adds token overhead; LLMs weight the first system message most heavily.

## Risks / Trade-offs

- **[Accidental double-Escape]** → Mitigation: 3-second window is short; "Esc again to interrupt" text gives visual feedback before committing.
- **[Race condition on interrupt]** → Mitigation: The cancellation token is checked between turns, not mid-token. If a step has already finished by the time the interrupt arrives, it is a no-op.
- **[Queue flush creates very long user turns]** → Mitigation: Flush concatenates with a separator; LLM context windows are large enough for multi-paragraph steering messages. If queue size becomes a concern, a `MAX_QUEUE_SIZE` cap can be added later.
- **[First-boot form blocks workflow on headless/embedded use]** → Mitigation: The `CLI_PROJECT` env var or `--project` flag can still bypass the form for non-interactive use. The form is only shown when no projects exist in the registry.
- **[System prompt token budget]** → The English directive is ~50 tokens; negligible impact on context window.