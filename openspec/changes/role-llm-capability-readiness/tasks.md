## 1. Readiness Model

- [ ] 1.1 Define role capability/readiness structs
- [ ] 1.2 Add provider/tool/fallback checks to role setup
- [ ] 1.3 Add readiness state to workbench projection/API

## 2. UI

- [ ] 2.1 Add compact role readiness badges
- [ ] 2.2 Add expandable readiness detail per role
- [ ] 2.3 Show capability warnings in task dispatch/chat output

## 3. Contracts And Safety

- [ ] 3.1 Replace generic mention contracts with role-specific contract builders
- [ ] 3.2 Add Worker safety context to dispatch flow
- [ ] 3.3 Add tests for fallback and missing-provider projections

## 4. Verification

- [ ] 4.1 Add tests for readiness states
- [ ] 4.2 Add tests for Scholar and Worker contract contents
- [ ] 4.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
