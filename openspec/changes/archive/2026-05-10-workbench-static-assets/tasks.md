## 1. Asset Split

- [x] 1.1 Create crate-local static asset directory
- [x] 1.2 Move inline HTML into `index.html`
- [x] 1.3 Move inline styles into CSS
- [x] 1.4 Move inline JavaScript into JS

## 2. Server Routes

- [x] 2.1 Add static asset routes with correct content types
- [x] 2.2 Keep existing API and SSE routes unchanged
- [x] 2.3 Decide whether assets are loaded from disk or embedded at compile time

## 3. Verification

- [x] 3.1 Add route tests or smoke checks for `/`, CSS, and JS
- [x] 3.2 Verify release-mode asset serving
- [x] 3.3 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test`
