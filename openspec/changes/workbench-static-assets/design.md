## Context

Inline assets were acceptable for the first prototype, but they make frontend iteration, caching, testing, and review difficult. Separating assets also prepares the workbench for richer lanes, notifications, and DAG interactions.

## Goals / Non-Goals

**Goals:**
- Split `index.html`, CSS, and JavaScript out of Rust source.
- Serve assets from `crates/workbench/static/` or a similar crate-local path.
- Ensure release builds include assets reliably.

**Non-Goals:**
- Introducing a full frontend build toolchain unless required.
- Rewriting the UI framework.

## Decisions

- Use simple static files first. Alternative: introduce Vite/React/etc.; rejected until there is a concrete need.
- Keep asset paths crate-local and package them with the Rust binary/crate.
- Add tests for `/`, CSS, and JS route availability.

## Risks / Trade-offs

- Runtime file serving can fail if assets are not packaged. Mitigation: use compile-time includes or packaging tests if deployment requires a single binary.
- Cache invalidation is basic. Mitigation: defer fingerprinting until assets are bundled or versioned.
