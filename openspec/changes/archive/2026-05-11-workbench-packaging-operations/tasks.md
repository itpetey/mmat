## 1. Documentation

- [x] 1.1 Document required and optional workbench environment variables
- [x] 1.2 Document Docker Compose/Postgres startup path
- [x] 1.3 Document dev and release run commands
- [x] 1.4 Align prototype/MVP wording with implemented capabilities

## 2. Diagnostics

- [x] 2.1 Improve missing configuration errors
- [x] 2.2 Improve bind/port conflict errors
- [x] 2.3 Improve missing static asset errors (compile-time embedding via `include_str!()` — missing file produces a compiler error with the file path; no runtime asset directory needed)
- [x] 2.4 Improve persistence initialisation errors

## 3. Packaging

- [x] 3.1 Verify release build serves UI/API/assets
- [x] 3.2 Add thin dev helper script or cargo alias if useful
- [x] 3.3 Ensure packaging docs mention static asset location/embedding decision

## 4. Verification

- [x] 4.1 Add smoke checks for documented run commands where practical
- [x] 4.2 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
