## 1. Dependencies and Configuration

- [ ] 1.1 Add `sqlx` with `postgres`, `runtime-tokio`, `json`, `uuid` features to workspace `Cargo.toml`
- [ ] 1.2 Remove `rusqlite` workspace dependency
- [ ] 1.3 Add `database_url: String` to `OrganisationConfig` replacing `event_store_path` and `memory_store_path`
- [ ] 1.4 Update `OrganisationRuntime::new` to connect Postgres pool from config

## 2. Event Store Postgres Backend

- [ ] 2.1 Create `PgEventStore` struct wrapping `sqlx::PgPool` in `event-store` crate
- [ ] 2.2 Implement `CREATE TABLE IF NOT EXISTS events (...)` with `BIGSERIAL rowid`, `UUID event_id PK`, `TEXT variant`, `JSONB payload`, `BIGINT timestamp_ns`, `TEXT source_agent`
- [ ] 2.3 Implement `insert()` — serialise event to JSONB, `INSERT INTO events ... RETURNING rowid`
- [ ] 2.4 Implement `replay(after_row, before_row)` — `SELECT WHERE rowid > $1 AND rowid <= $2 ORDER BY rowid ASC`
- [ ] 2.5 Implement `query_by_variant(variant, after_row, before_row)`
- [ ] 2.6 Implement `latest_row()` — `SELECT MAX(rowid) FROM events`
- [ ] 2.7 Implement `row_for_event_id()` — `SELECT rowid FROM events WHERE event_id = $1`
- [ ] 2.8 Implement `get_by_event_id()`
- [ ] 2.9 Add `EventStore::new(database_url)` constructor; keep `EventStore::open(path)` for SQLite backward compat
- [ ] 2.10 Update `EventBus` to accept either store variant behind `Arc<dyn EventStore>`

## 3. Memory Store Postgres Backend

- [ ] 3.1 Create `PgMemoryStore` struct wrapping `sqlx::PgPool` in `memory` crate
- [ ] 3.2 Implement `CREATE TABLE IF NOT EXISTS memories (...)` with all `Memory` fields plus optional `embedding vector(64)` (conditionally created when pgvector extension available)
- [ ] 3.3 Implement `insert()` — structured column insert with optional embedding
- [ ] 3.4 Implement `get_by_id(MemoryId)` retrieval
- [ ] 3.5 Implement `query_by_type()`, `query_by_scope()`, `query_by_authority()`, `query_decayed()` — all filtering `WHERE superseded_by IS NULL`
- [ ] 3.6 Implement `supersede(old_id, new_id)` — transactional UPDATE of both rows
- [ ] 3.7 Implement `get_supersession_chain()` — recursive CTE query
- [ ] 3.8 Implement `insert_with_embedding()` — coordinates Postgres INSERT + vector backend (pgvector or Qdrant), rolls back on failure
- [ ] 3.9 Implement `search_similar()` — when pgvector enabled, use `ORDER BY embedding <=> $1 LIMIT $2`; otherwise delegate to `VectorMemoryBackend`
- [ ] 3.10 Add `update_content()` and `update_last_accessed()` methods
- [ ] 3.11 Extract `VectorMemoryBackend` trait if not already fully abstracted; wrap Qdrant and pgvector behind it

## 4. Artefact Store (JSONB in Postgres)

- [ ] 4.1 Create `ArtefactStore` struct wrapping `sqlx::PgPool` in `roles` crate
- [ ] 4.2 Implement `CREATE TABLE IF NOT EXISTS artefacts (...)` with `UUID artefact_id PK`, `TEXT artefact_type`, `TEXT content_hash`, `JSONB payload`, `TEXT producer_role`, `TIMESTAMPTZ created_at`
- [ ] 4.3 Implement `store_artefact(artefact_type, payload) -> StoredArtefactRef` — inserts row, returns new-style `db://` URI
- [ ] 4.4 Implement `get_artefact(artefact_id) -> Option<String>` — fetches payload by ID
- [ ] 4.5 Implement `read_artefact_payload(storage_uri)` — handles both `db://artefacts/` and legacy `file://` URIs
- [ ] 4.6 Replace `store_artefact_blob()` in `roles/src/artefacts.rs` to use `ArtefactStore` instead of `std::fs`
- [ ] 4.7 Add `ArtefactStore` to `OrganisationRuntime` and pass to all roles that produce artefacts (IntentLead, Scholar, ProjectManager, Auditor, Worker)
- [ ] 4.8 Implement transactional `store_and_publish_event()` — artefact INSERT + event INSERT in same Postgres transaction

## 5. SQLite to Postgres Migration Tool

- [ ] 5.1 Create `bin/sqlite_to_postgres.rs` that reads from SQLite `events` and `memories` databases and writes to Postgres
- [ ] 5.2 Handle `storage_uri` rewrite: relocate `.mmat/artefacts/` blobs into Postgres `artefacts` table, update URIs from `file://` to `db://`
- [ ] 5.3 Add `--dry-run` flag for validation before execution

## 6. Test Infrastructure

- [ ] 6.1 Add `sqlx::test` or `testcontainers` dependency for Postgres in CI
- [ ] 6.2 Update `event-stream/tests/integration.rs` — rewrite SQLite store tests to use Postgres (unique schema per test)
- [ ] 6.3 Update `memory/tests/integration.rs` — rewrite memory store tests for Postgres backend
- [ ] 6.4 Update `roles/tests/auditor_tests.rs` and `role_flow_tests.rs` — update artefact reads to use new URI format
- [ ] 6.5 Add `PgEventStore` unit tests: concurrent writes, replay, variant filtering, empty store
- [ ] 6.6 Add `PgMemoryStore` unit tests: CRUD, supersession chains, decay queries, authority filtering
- [ ] 6.7 Add `ArtefactStore` unit tests: store/retrieve, JSONB query, `db://` URI handling, transactional commit/rollback
- [ ] 6.8 Add `docker-compose.yml` with Postgres + pgvector image for local development

## 7. CI and Documentation

- [ ] 7.1 Update GitHub Actions workflow — add `services.postgres` with `postgres:16` + pgvector image
- [ ] 7.2 Update `.env.example` or equivalent with `DATABASE_URL` connection string
- [ ] 7.3 Add `**/.mmat/` removal note — directory no longer created (unless running legacy SQLite mode)
- [ ] 7.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, `cargo test` — fix any issues
