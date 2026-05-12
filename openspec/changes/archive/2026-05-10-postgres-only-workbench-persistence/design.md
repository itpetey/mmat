## Context

The active specs already define Postgres-backed event, memory, and artefact stores. The workbench was added quickly using the runtime's legacy SQLite configuration fields to get local replay working. That was useful for the prototype but conflicts with the intended persistence model.

## Goals / Non-Goals

**Goals:**
- Make Postgres the only supported workbench persistence backend.
- Remove workbench creation of `.mmat/workbench/events.db` and `.mmat/workbench/memory.db`.
- Hydrate UI projection from Postgres event replay.
- Surface configuration failures before starting the HTTP server.

**Non-Goals:**
- Migrating arbitrary legacy `.mmat` databases automatically.
- Removing every SQLite test helper in crates unrelated to the workbench.
- Changing the event schema beyond what existing Postgres specs require.

## Decisions

- Use `MMAT_DB_URL` as the workbench persistence source. Alternative: introduce `MMAT_WORKBENCH_MMAT_DB_URL`; rejected because runtime and stores already use `MMAT_DB_URL`.
- Fail fast when `MMAT_DB_URL` is absent. Alternative: silently fall back to SQLite; rejected because it preserves the confusing split-brain state.
- Replay the event store directly into the UI projection before seeding first-run prompts. Alternative: keep projection ephemeral; rejected because restarts must preserve visible project context.
- Treat `.mmat` as legacy data only. The workbench may document migration/export commands later, but it must not write new `.mmat` state.

## Risks / Trade-offs

- Local onboarding requires Postgres. Mitigation: provide clear Docker Compose/run instructions and startup diagnostics.
- Existing `.mmat/workbench` prototype sessions will not resume automatically. Mitigation: document this as a breaking change and provide a future migration task only if needed.
- Tests need isolated Postgres databases. Mitigation: reuse the existing Postgres test utilities and skip gracefully when Postgres is unavailable where appropriate.

## Migration Plan

1. Update workbench runtime configuration to require `MMAT_DB_URL`.
2. Remove `.mmat/workbench` creation and replay from SQLite.
3. Update smoke tests and docs.
4. Leave ignored `.mmat` paths untouched for archived/legacy data until a separate cleanup removes them.
