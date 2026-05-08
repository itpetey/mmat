## Why

SQLite + `.mmat/` files worked for a single-process prototype, but they create durability, concurrency, and operational problems as the system matures. Postgres gives us a single queryable store for events, memories, and artefact blobs — with JSONB, pgvector, replication, and transactional consistency across all three. Everything becomes one backup/restore target instead of three.

## What Changes

- **Merge EventStore, MemoryStore, and `.mmat/` artefacts into one Postgres database**
  - `events` table replaces `event_store.db` (SQLite)
  - `memories` table replaces `memory.db` (SQLite) + `memory_store.memories` vector field via `pgvector`
  - `artefacts` table replaces `.mmat/artefacts/` filesystem blobs
- **BREAKING**: `OrganisationConfig` storage paths replaced by a single `database_url` (Postgres connection string)
- **BREAKING**: `.mmat/artefacts/` directory removed — blobs live in Postgres `artefacts.payload`
- **NEW**: `sqlx` dependency replaces `rusqlite`; `pgvector` replaces or sits alongside `qdrant-client`
- Add connection pooling via `deadpool-postgres` or `sqlx` built-in pool
- All existing public trait/struct APIs remain stable — only the backend implementation changes

## Capabilities

### New Capabilities
- `artefact-store`: durable, queryable artefact blob storage in Postgres JSONB, replacing `.mmat/` filesystem

### Modified Capabilities
- `event-store`: backend changes from SQLite to Postgres; adds connection-pool, concurrent-writer, replication requirements
- `memory-store`: backend changes from SQLite+Qdrant to Postgres+pgvector; adds vector-index, concurrent-reader requirements

## Impact

| Area | Detail |
|------|--------|
| **Dependencies** | Add `sqlx` (postgres, runtime-tokio, json), `deadpool-postgres`; remove `rusqlite`; optionally remove `qdrant-client` if `pgvector` replaces it |
| **Configuration** | `OrganisationConfig` gains `database_url: String` (Postgres DSN); loses `event_store_path` and `memory_store_path` |
| **EventStore** | Rewrite `event_store.rs` to use `sqlx` + Postgres; adapt `row_for_event_id` / `replay` queries for serial/identity columns |
| **MemoryStore** | Rewrite `store.rs` to use `sqlx` + Postgres; add `pgvector` for `embedding` column; adapt `search_similar` for `pgvector` cosine |
| **Artefacts** | Replace `fs::write`/`fs::read_to_string` in `roles/src/artefacts.rs` with `INSERT`/`SELECT` on `artefacts` table |
| **Tests** | Use `testcontainers` (Postgres) or `sqlx::test` + temp databases instead of `tempfile` + SQLite |
| **CI** | Needs a Postgres instance (e.g. GitHub Actions `services.postgres`) |
