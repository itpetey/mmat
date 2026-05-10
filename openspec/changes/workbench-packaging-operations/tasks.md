## 1. Documentation

- [ ] 1.1 Document required and optional workbench environment variables
- [ ] 1.2 Document Docker Compose/Postgres startup path
- [ ] 1.3 Document dev and release run commands
- [ ] 1.4 Align prototype/MVP wording with implemented capabilities

## 2. Diagnostics

- [ ] 2.1 Improve missing configuration errors
- [ ] 2.2 Improve bind/port conflict errors
- [ ] 2.3 Improve missing static asset errors
- [ ] 2.4 Improve persistence initialisation errors

## 3. Packaging

- [ ] 3.1 Verify release build serves UI/API/assets
- [ ] 3.2 Add thin dev helper script or cargo alias if useful
- [ ] 3.3 Ensure packaging docs mention static asset location/embedding decision

## 4. Verification

- [ ] 4.1 Add smoke checks for documented run commands where practical
- [ ] 4.2 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
