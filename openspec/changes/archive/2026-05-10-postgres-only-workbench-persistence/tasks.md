## 1. Configuration

- [x] 1.1 Replace workbench `.mmat/workbench` path setup with `MMAT_DB_URL` loading and validation
- [x] 1.2 Add clear startup errors for missing or unreachable Postgres
- [x] 1.3 Ensure runtime configuration used by workbench does not populate SQLite store paths

## 2. Projection Replay

- [x] 2.1 Replay workbench projection state from Postgres event store
- [x] 2.2 Preserve initial prompt seeding only for empty project conversations
- [x] 2.3 Remove `file://` assumptions from workbench persistence paths

## 3. Verification

- [x] 3.1 Add tests for missing `MMAT_DB_URL` failure
- [x] 3.2 Add restart/resume test using a temporary Postgres schema
- [x] 3.3 Update README and run instructions
- [x] 3.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
