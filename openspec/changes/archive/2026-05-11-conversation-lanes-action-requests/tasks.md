## 1. Data Model

- [x] 1.1 Add lane structs and lane metadata to workbench projection/API state
- [x] 1.2 Add addressable message IDs and lane tags to chat projection
- [x] 1.3 Add action request projection model with pending/resolved/cancelled status

## 2. Lane UI

- [x] 2.1 Add lane navigation with coloured chips
- [x] 2.2 Add single-lane, multi-lane, and global chat filters
- [x] 2.3 Add lane chips to messages and global views
- [x] 2.4 Add archive/pause status handling for lanes

## 3. Tools And Routing

- [x] 3.1 Add `create_lane` tool/event path with source message provenance
- [x] 3.2 Add user lane creation control in chat
- [x] 3.3 Add action-request rendering and typed reply handling

## 4. Notifications

- [x] 4.1 Store notification targets as message/lane/DAG references
- [x] 4.2 Implement notification click deep-linking to lane messages
- [x] 4.3 Implement notification click deep-linking to DAG nodes

## 5. Verification

- [x] 5.1 Add tests for lane filtering and global aggregation
- [x] 5.2 Add tests for tool-created lane provenance
- [x] 5.3 Add tests for action request resolution
- [x] 5.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
