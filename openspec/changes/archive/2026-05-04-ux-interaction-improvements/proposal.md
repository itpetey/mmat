## Why

Users currently have no way to interrupt a running LLM step once it has started. The only option is to kill the entire process. Additionally, during a running step any user input is locked out until the step completes, preventing real-time steering of the LLM. On first boot, a default project is silently created instead of prompting the user to name and configure one. Finally, LLM system prompts are inconsistent about language conventions, producing American English output when International English is required.

## What Changes

- Add a double-press Escape key binding that interrupts the currently running step. The first press shows "Esc again to interrupt"; a second press within 3 seconds stops the running step.
- Allow user messages to be queued while a step is running. As soon as the LLM API response completes (even mid-tool-call), the queued messages are flushed into the next LLM turn. This enables user steering during long-running steps.
- Replace the default project creation on first boot with a prompt that asks the user to create a new project (name and root directory) before any workflow starts.
- Add an International English directive to all system prompts so LLMs output International English exclusively (Oxford spelling, `ise`/`isation` on suffixes, `colour`/`favour`/`metre` spellings, etc.).

## Capabilities

### New Capabilities
- `step-interrupt`: Escape-key double-press mechanism to interrupt running workflow steps from the UI
- `message-queue`: Queuing and flushing of user messages mid-step for LLM steering

### Modified Capabilities
- `liveview-ui`: Replace default project creation with a first-boot project creation prompt; the composer and state management must support the new interrupt and message-queue interactions

## Impact

- **UI layer** (`liveview/components.rs`): New keyboard event handler for Escape, interrupt status display in the composer, queued message indicator, first-boot project creation form
- **State layer** (`liveview/state.rs`): New fields on `UiState`/`UiSnapshot` for interrupt state, message queue, and first-boot flag; new methods for queuing messages and signalling interrupts
- **Event layer** (`liveview/event.rs`): New `FrontendEvent` variants for interrupt signals and message flush triggers
- **Frontend bin** (`bin/frontend.rs`): Removal of `ensure_default_project` call; replacement with first-boot flow; interrupt handler wired to `ChannelHumanIO` cancellation
- **LLM layer** (`plan/discovery.rs`, `plan/solutions.rs`, `plan/knowledge.rs`, `plan/architect.rs`, `deliver/engine.rs`): All system prompt constants receive an International English directive