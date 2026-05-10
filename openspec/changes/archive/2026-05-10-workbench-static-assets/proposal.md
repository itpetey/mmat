## Why

The workbench frontend is currently embedded as a large inline string in Rust. The UI is expected to grow, so HTML, CSS, and JavaScript should be separated now before iteration becomes painful.

## What Changes

- Move inline workbench HTML/CSS/JS into static asset files.
- Serve static assets through explicit routes with correct content types.
- Keep backend code focused on API, SSE, runtime wiring, and projection.
- Add tests/smoke coverage for static asset routes.

## Capabilities

### New Capabilities
- `workbench-static-assets`: Static asset serving and frontend/backend source separation for `mmat-workbench`.

### Modified Capabilities

## Impact

- Affects `crates/workbench` source layout, routes, build packaging, and tests.
