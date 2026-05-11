## 1. Readiness Model

- [x] 1.1 Define role capability/readiness structs
- [x] 1.2 Add provider/tool/fallback checks to role setup
- [x] 1.3 Add readiness state to workbench projection/API

## 2. UI

- [x] 2.1 Add compact role readiness badges
- [x] 2.2 Add expandable readiness detail per role
- [x] 2.3 Show capability warnings in task dispatch/chat output

## 3. Contracts And Safety

- [x] 3.1 Replace generic mention contracts with role-specific contract builders
- [x] 3.2 Add Worker safety context to dispatch flow
- [x] 3.3 Add tests for fallback and missing-provider projections

## 4. Verification

- [x] 4.1 Add tests for readiness states
- [x] 4.2 Add tests for Scholar and Worker contract contents
- [x] 4.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
