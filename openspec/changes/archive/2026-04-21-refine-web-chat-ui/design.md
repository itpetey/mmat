## Context

The current LiveView UI in `src/ws/server.rs` renders `UiState.event_history` directly as monospace transcript rows and swaps the entire composer area between an initial prompt card, a reply card, and a passive "Working..." placeholder. `UiState` only models raw events plus pending prompt state, so the browser view has no first-class distinction between conversation turns and debugging output.

The requested change is cross-cutting because it affects the LiveView rendering layer, the UI snapshot/state model, and the event translation path that currently forwards logs directly into the visible transcript.

## Goals / Non-Goals

**Goals:**
- Make the main transcript conversation-first instead of log-first
- Keep raw logs available in a collapsed debug view
- Keep the composer mounted while work is in progress and reflect busy state through button state rather than placeholder-only rendering
- Clear the textarea after submit and make prompt/reply submission behaviour consistent
- Avoid showing partial LLM output in the main transcript

**Non-Goals:**
- No change to the underlying workflow orchestration or human approval semantics
- No persisted browser session history beyond the existing in-memory process lifetime
- No attempt to build a multi-user chat system or concurrent session model
- No rich markdown rendering or syntax-highlighting beyond light conversation formatting in this change

## Decisions

### Separate conversation turns from raw workflow logs

`UiState` will grow a conversation-oriented history alongside the existing raw event history. The conversation history will contain user submissions, pending human questions, and any completed assistant-visible messages that the UI should present as chat turns. Raw `UiEvent` entries remain available for inspection, but they no longer define the main transcript.

This avoids trying to infer conversation structure from plain log strings during rendering and gives the UI a stable model for pretty-printing prompt/reply exchanges.

**Alternatives considered:**
- Derive conversation turns from `event_history` at render time: rejected because it depends on parsing display strings and cannot reliably distinguish user-visible messages from debug logs.
- Hide logs purely in CSS while still treating them as transcript rows: rejected because the main transcript would remain semantically log-first.

### Use a persistent composer with prompt-aware state

The composer will stay mounted regardless of whether MMAT is waiting for user input or actively working. The pending question, if any, will appear in the conversation transcript as an assistant/system turn. The submit button will reflect the current state (`Start`, `Reply`, `Working...`) and be disabled when the workflow is not ready to accept input.

Submitted text will be appended to the conversation transcript before the oneshot sender is fulfilled, and the textarea will be cleared immediately after a successful submit.

**Alternatives considered:**
- Keep the current card-swapping approach and only restyle it: rejected because it preserves the abrupt input replacement the user explicitly asked to remove.

### Standardise multiline keyboard behaviour

The composer will use standard chat-style keyboard handling: `Enter` submits when submission is available, and `Shift+Enter` inserts a newline. Button submission remains available for all prompt types.

**Alternatives considered:**
- Keep `Shift+Enter` as the submit shortcut: rejected because it is non-standard and is already causing reply-entry confusion.

### Keep raw logs collapsed and non-streaming in the main conversation view

Raw logs will render in a dedicated collapsed container, using the existing `UiEvent` history as the source of truth for diagnostics. The conversation transcript will only show stable entries; partial token or delta-style output must not be appended incrementally to visible chat messages.

This keeps debugging detail accessible without letting tracing noise dominate the primary interaction model.

**Alternatives considered:**
- Continue streaming every log into the main transcript: rejected because it makes the conversation hard to follow.
- Remove raw logs entirely: rejected because they remain useful for debugging and implementation visibility.

## Risks / Trade-offs

[Two transcript representations can drift] -> Mitigation: centralise all conversation entry writes inside `UiState` helper methods and keep raw logs append-only.

[Not all current backend output maps cleanly to assistant chat messages] -> Mitigation: scope the conversation transcript to user submissions, human questions, and explicit assistant-visible summary messages; leave everything else in collapsed raw logs.

[Keyboard handling may differ across Dioxus LiveView/browser combinations] -> Mitigation: implement explicit key handling in the textarea and cover both initial-input and reply flows in manual testing.

[Collapsed logs reduce immediate visibility during debugging] -> Mitigation: keep the disclosure in the main page, default it closed, and preserve level-based styling inside the expanded log view.

## Migration Plan

No data migration is required. The change is local to the in-process LiveView UI.

Implementation should proceed in this order:
1. Extend `UiState` with conversation-oriented snapshot data and helper methods for recording user/assistant-visible turns.
2. Update the event translator and runtime submission paths to populate conversation history without exposing token-by-token output.
3. Rework `RootApp`, `InitialInputCard`, and `PromptCard` into a persistent composer plus conversation transcript layout.
4. Move raw log rendering into a collapsed disclosure and keep existing level styling there.

Rollback is straightforward: revert the UI state additions and restore the current log-first rendering path.

## Open Questions

- Which existing backend events should become completed assistant-visible messages beyond prompt questions and user replies?
- Should the collapsed raw-log container be a single global disclosure or grouped per workflow phase once the first version is in place?
