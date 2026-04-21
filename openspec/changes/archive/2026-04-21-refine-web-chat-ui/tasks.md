## 1. Extend shared UI state for conversation-first rendering

- [x] 1.1 Add conversation-oriented entry types and snapshot fields in `src/ws/ui_state.rs` alongside the existing raw `UiEvent` history
- [x] 1.2 Add helper methods in `UiState` for recording submitted user messages, assistant-visible prompt questions, and completed assistant-visible messages
- [x] 1.3 Stop relying on `planning_started` as a UI mode switch and remove or retire the associated state/update path

## 2. Update event and prompt plumbing

- [x] 2.1 Update `src/ws/translator.rs` so `FrontendEvent::HumanPrompt` records the pending question as a visible conversation entry while still storing the reply sender
- [x] 2.2 Ensure raw `FrontendEvent::Log` values continue to populate the raw log history without automatically becoming visible chat turns
- [x] 2.3 Update the initial prompt and pending reply submission paths so successful submits append the user message to conversation history and clear the composer draft
- [x] 2.4 Define how completed assistant-visible output is emitted into conversation history without token-by-token transcript updates

## 3. Rework the LiveView chat layout and composer

- [x] 3.1 Replace the log-first transcript rendering in `src/ws/server.rs` with a conversation-first transcript that shows user turns, assistant-visible turns, and status markers
- [x] 3.2 Keep the bottom composer mounted for initial input, replies, and in-progress workflow states instead of swapping in a standalone "Working..." placeholder
- [x] 3.3 Change the submit button state to reflect `Start`, `Reply`, and `Working...` modes while preserving the textarea layout
- [x] 3.4 Update keyboard handling so `Enter` submits when enabled and `Shift+Enter` inserts a newline for both initial prompts and replies

## 4. Add collapsed raw-log rendering and styling

- [x] 4.1 Render raw `UiEvent` logs inside a collapsed disclosure container that is secondary to the conversation transcript
- [x] 4.2 Preserve level-based styling for info, warning, and error logs inside the expanded raw-log view
- [x] 4.3 Update the chat styling so conversation messages are pretty-printed and user submissions visibly appear in the main transcript after each submit

## 5. Verify the refined browser flow

- [x] 5.1 Run `cargo fmt --all`
- [x] 5.2 Run `cargo clippy -- -D warnings`
- [x] 5.3 Run `cargo test`
- [ ] 5.4 Manually verify the initial prompt flow: submit text, confirm the composer clears, the user message appears in the conversation transcript, and the button enters a working state
- [ ] 5.5 Manually verify the reply flow: pending question appears in the conversation transcript, `Shift+Enter` inserts a newline, `Enter` submits, and the reply is added to the conversation transcript
- [ ] 5.6 Manually verify raw logs stay collapsed by default and no partial assistant/token output is streamed into the visible conversation transcript
