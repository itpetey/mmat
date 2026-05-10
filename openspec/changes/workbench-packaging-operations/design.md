## Context

The workbench has grown from prototype toward MVP. Its operational contract should be explicit before more users or agents depend on it.

## Goals / Non-Goals

**Goals:**
- Make local startup predictable.
- Make configuration errors actionable.
- Package static assets and runtime dependencies reliably.
- Keep developer workflow simple.

**Non-Goals:**
- Full production deployment automation.
- Authentication, TLS, or multi-user operations.

## Decisions

- Treat `DATABASE_URL` as required once Postgres-only persistence lands.
- Keep `MMAT_WORKBENCH_ADDR` for bind override.
- Add clear diagnostics for port conflicts and missing assets.
- Promote terminology from prototype to MVP only after static assets and Postgres-only persistence land.

## Risks / Trade-offs

- More setup requirements can slow first run. Mitigation: provide Docker Compose and copy-paste commands.
- Helper scripts can drift. Mitigation: keep scripts thin wrappers around documented cargo commands.
