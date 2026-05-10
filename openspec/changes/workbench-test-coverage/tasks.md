## 1. Test Infrastructure

- [ ] 1.1 Expose app construction for integration tests without binding a real port
- [ ] 1.2 Add reusable test state/runtime builders
- [ ] 1.3 Add optional temporary Postgres test setup for workbench persistence

## 2. API And SSE Tests

- [ ] 2.1 Add `/api/state` and `/api/messages` tests
- [ ] 2.2 Add notification/action acknowledgement tests
- [ ] 2.3 Add bounded `/events` SSE tests
- [ ] 2.4 Add static asset route tests

## 3. Projection Tests

- [ ] 3.1 Add replay/resume projection tests
- [ ] 3.2 Add lane filtering tests
- [ ] 3.3 Add action request resolution tests
- [ ] 3.4 Add artefact loading and DAG construction tests

## 4. Verification

- [ ] 4.1 Document required test environment variables
- [ ] 4.2 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
