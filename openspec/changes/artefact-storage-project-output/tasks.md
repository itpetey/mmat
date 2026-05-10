## 1. Storage Routing

- [ ] 1.1 Route non-code artefacts through Postgres artefact store
- [ ] 1.2 Route generated code outputs through project repository/worktree paths
- [ ] 1.3 Remove workbench-local artefact storage assumptions

## 2. Events And Projection

- [ ] 2.1 Extend artefact projection to distinguish blob artefacts from code outputs
- [ ] 2.2 Include repository path/worktree metadata for code artefacts
- [ ] 2.3 Link evidence refs from artefacts to event details

## 3. UI

- [ ] 3.1 Add blob artefact renderer backed by Postgres fetch
- [ ] 3.2 Add code output renderer with path/diff/validation summary
- [ ] 3.3 Add error states for missing artefact blobs or repository paths

## 4. Verification

- [ ] 4.1 Add tests for blob artefact retrieval
- [ ] 4.2 Add tests for code output repository metadata
- [ ] 4.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
