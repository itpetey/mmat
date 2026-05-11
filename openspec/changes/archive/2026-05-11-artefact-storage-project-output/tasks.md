## 1. Storage Routing

- [x] 1.1 Route non-code artefacts through Postgres artefact store
- [x] 1.2 Route generated code outputs through project repository/worktree paths
- [x] 1.3 Remove workbench-local artefact storage assumptions

## 2. Events And Projection

- [x] 2.1 Extend artefact projection to distinguish blob artefacts from code outputs
- [x] 2.2 Include repository path/worktree metadata for code artefacts
- [x] 2.3 Link evidence refs from artefacts to event details

## 3. UI

- [x] 3.1 Add blob artefact renderer backed by Postgres fetch
- [x] 3.2 Add code output renderer with path/diff/validation summary
- [x] 3.3 Add error states for missing artefact blobs or repository paths

## 4. Verification

- [x] 4.1 Add tests for blob artefact retrieval
- [x] 4.2 Add tests for code output repository metadata
- [x] 4.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
