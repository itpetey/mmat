## 1. Asset Split

- [ ] 1.1 Create crate-local static asset directory
- [ ] 1.2 Move inline HTML into `index.html`
- [ ] 1.3 Move inline styles into CSS
- [ ] 1.4 Move inline JavaScript into JS

## 2. Server Routes

- [ ] 2.1 Add static asset routes with correct content types
- [ ] 2.2 Keep existing API and SSE routes unchanged
- [ ] 2.3 Decide whether assets are loaded from disk or embedded at compile time

## 3. Verification

- [ ] 3.1 Add route tests or smoke checks for `/`, CSS, and JS
- [ ] 3.2 Verify release-mode asset serving
- [ ] 3.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
