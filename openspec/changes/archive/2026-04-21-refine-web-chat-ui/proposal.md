## Why

The current LiveView UI exposes the internal event stream more directly than the conversation, which makes prompt-and-reply interactions harder to follow. The composer also swaps out to a passive "Working..." state, does not support reply editing ergonomically, and leaves the browser transcript feeling unlike a usable chat surface.

## What Changes

- Rework the browser UI so the primary transcript is a conversation view that shows user submissions and completed assistant messages in chronological order
- Move raw workflow logs behind collapsed, secondary containers so debugging detail stays available without dominating the main interface
- Keep a persistent composer visible while work is in progress, clearing submitted text and reflecting busy state through the submit button instead of replacing the whole input area
- Fix reply entry behaviour so multiline input works consistently and prompt/reply submissions are added back into the conversation view
- Stop streaming partial LLM response tokens into the visible UI and only render stable, completed assistant output

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-chat-ui`: Change the browser transcript from log-first rendering to conversation-first rendering, add persistent composer behaviour, and define raw-log disclosure behaviour
- `ui-state-bus`: Adjust UI state expectations so submitted prompts/replies and completed assistant messages can be rendered as conversation turns without token-by-token streaming

## Impact

- `src/ws/server.rs`: Chat layout, composer state, keyboard handling, and event rendering logic will change
- `src/ws/ui_state.rs`: Snapshot and event/state shaping will need to support conversation turns separately from raw logs
- `src/ws/event.rs` and `src/ws/translator.rs`: Frontend events may need refinement so completed assistant output can be distinguished from raw logs
- `web/style.css` or inline styling in the LiveView host: Conversation-first presentation and collapsed log containers will require updated styling
