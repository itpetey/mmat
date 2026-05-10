## 1. Configuration

- [ ] 1.1 Replace workbench `.mmat/workbench` path setup with `DATABASE_URL` loading and validation
- [ ] 1.2 Add clear startup errors for missing or unreachable Postgres
- [ ] 1.3 Ensure runtime configuration used by workbench does not populate SQLite store paths

## 2. Projection Replay

- [ ] 2.1 Replay workbench projection state from Postgres event store
- [ ] 2.2 Preserve initial prompt seeding only for empty project conversations
- [ ] 2.3 Remove `file://` assumptions from workbench persistence paths

## 3. Verification

- [ ] 3.1 Add tests for missing `DATABASE_URL` failure
- [ ] 3.2 Add restart/resume test using a temporary Postgres schema
- [ ] 3.3 Update README and run instructions
- [ ] 3.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
