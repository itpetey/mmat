## 1. Routing

- [x] 1.1 Replace generic mention routing with role-specific event builders
- [x] 1.2 Add reviewer routing for `ReviewRequested` and missing-context guidance
- [x] 1.3 Add tests for every supported mention target

## 2. Librarian Service

- [x] 2.1 Start Librarian alongside runtime using shared bus and stores
- [x] 2.2 Project `MemoryAccepted`, `MemoryRejected`, and `MemorySuperseded` into Librarian UI state
- [x] 2.3 Add tests for Librarian-visible memory lifecycle events

## 3. Runtime Projection

- [x] 3.1 Expand DAG projection for `TaskStarted`, `TaskFailed`, `ReviewRequested`, and `EscalationRequested`
- [x] 3.2 Link scheduler task state to DAG step state where available
- [x] 3.3 Show auto-dispatch handoffs in chat and DAG

## 4. Verification

- [x] 4.1 Add integration tests for mention-to-event routing
- [x] 4.2 Add runtime smoke test with Librarian enabled
- [x] 4.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
