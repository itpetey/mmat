## 1. Data Model

- [ ] 1.1 Add lane structs and lane metadata to workbench projection/API state
- [ ] 1.2 Add addressable message IDs and lane tags to chat projection
- [ ] 1.3 Add action request projection model with pending/resolved/cancelled status

## 2. Lane UI

- [ ] 2.1 Add lane navigation with coloured chips
- [ ] 2.2 Add single-lane, multi-lane, and global chat filters
- [ ] 2.3 Add lane chips to messages and global views
- [ ] 2.4 Add archive/pause status handling for lanes

## 3. Tools And Routing

- [ ] 3.1 Add `create_lane` tool/event path with source message provenance
- [ ] 3.2 Add user lane creation control in chat
- [ ] 3.3 Add action-request rendering and typed reply handling

## 4. Notifications

- [ ] 4.1 Store notification targets as message/lane/DAG references
- [ ] 4.2 Implement notification click deep-linking to lane messages
- [ ] 4.3 Implement notification click deep-linking to DAG nodes

## 5. Verification

- [ ] 5.1 Add tests for lane filtering and global aggregation
- [ ] 5.2 Add tests for tool-created lane provenance
- [ ] 5.3 Add tests for action request resolution
- [ ] 5.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
