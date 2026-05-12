## Why

The workbench currently creates `.mmat/workbench/*.db` SQLite stores even though MMAT persistence has moved to Postgres. This creates two persistence models, stale documentation, and a confusing restart story.

## What Changes

- **BREAKING**: The workbench requires `MMAT_DB_URL` and will not create or use `.mmat/workbench` SQLite files.
- Runtime-backed workbench event replay hydrates from Postgres event rows.
- Workbench memory and artefact access uses the Postgres stores already specified by `event-store`, `memory-store`, and `artefact-store`.
- Startup errors clearly explain missing or invalid Postgres configuration.
- Documentation removes `.mmat/workbench` as an active persistence path.

## Capabilities

### New Capabilities
- `workbench-persistence`: Postgres-only persistence and replay behaviour for `mmat-workbench`.

### Modified Capabilities
- `runtime`: Runtime configuration MUST reject legacy SQLite paths for workbench usage and require Postgres-backed stores.

## Impact

- Affects `crates/workbench`, `crates/coordinator`, README/run documentation, and tests that currently assume SQLite fallback paths.
- Existing `.mmat/workbench` data becomes legacy-only and is not used by the workbench.
