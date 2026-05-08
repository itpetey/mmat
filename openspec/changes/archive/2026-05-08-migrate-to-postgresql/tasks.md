## 1. Dependencies and Configuration

- [x] 1.1 Add `sqlx` (crate-local, not workspace) with `postgres`, `runtime-tokio` features to `event-stream/Cargo.toml`
- [x] 1.2 Keep `rusqlite` workspace dependency — **deferred removal**: both SQLite and Postgres coexist for now
- [x] 1.3 Add `database_url: Option<String>` and make `event_store_path`/`memory_store_path` `Option<PathBuf>` in `OrganisationConfig`
- [x] 1.4 Update `OrganisationRuntime::new` to select store variant based on `database_url` presence

## 2. Event Store Postgres Backend

- [x] 2.1 Create `PgEventStore` struct wrapping `sqlx::PgPool` in `event-store` crate
- [x] 2.2 Implement `CREATE TABLE IF NOT EXISTS events (...)` with `BIGSERIAL rowid`, `UUID event_id PK`, `TEXT variant`, `JSONB payload`, `BIGINT timestamp_ns`, `TEXT source_agent`
- [x] 2.3 Implement `insert()` — serialise event to JSONB, let `BIGSERIAL` auto-assign rowid
- [x] 2.4 Implement `replay(after_row, before_row)` — `SELECT WHERE rowid > $1 AND rowid <= $2 ORDER BY rowid ASC`
- [x] 2.5 Implement `query_by_variant(variant, after_row, before_row)`
- [x] 2.6 Implement `latest_row()` — `SELECT MAX(rowid) FROM events`
- [x] 2.7 Implement `row_for_event_id()` — `SELECT rowid FROM events WHERE event_id = $1`
- [x] 2.8 Implement `get_by_event_id()`
- [x] 2.9 Add `EventStore::new(database_url)` constructor (runs migration); keep `EventStore::open(path)` for SQLite backward compat
- [x] 2.10 Refactor `EventStore` to an enum dispatch (`Sqlite`/`Postgres` variants) instead of `Arc<dyn EventStore>`

## 3. Memory Store Postgres Backend

- [x] 3.1 Create `PgMemoryStore` struct wrapping `sqlx::PgPool` in `memory` crate
- [x] 3.2 Implement `CREATE TABLE IF NOT EXISTS memories (...)` — UUID id, TIMESTAMPTZ replaced with TEXT to match SQLite serialisation, same indexes as SQLite
- [x] 3.3 Implement `insert()` — structured column insert, nullable UUIDs for supersedes/superseded_by
- [x] 3.4 Implement `get_by_id(MemoryId)` retrieval
- [x] 3.5 Implement `query_by_type()`, `query_by_scope()`, `query_by_authority()`, `query_decayed()` — all filtering `WHERE superseded_by IS NULL`
- [x] 3.6 Implement `supersede(old_id, new_id)` — transactional UPDATE of both rows via `pool.begin()`
- [x] 3.7 Implement `get_supersession_chain()` — iterative traversal (same approach as SQLite)
- [x] 3.8 Implement `insert_with_embedding()` — Postgres transaction + Qdrant upsert with rollback on failure
- [x] 3.9 Implement `search_similar()` — delegates to `VectorMemoryBackend`, filters superseded in memory
- [x] 3.10 Implement `update_content()` and `update_last_accessed()` methods
- [x] 3.11 Extract `VectorMemoryBackend` trait to its own `vector_backend` module; Qdrant already wrapped behind it

## 4. Artefact Store (JSONB in Postgres)

- [x] 4.1 Create `ArtefactStore` struct wrapping `sqlx::PgPool` in `memory` crate
- [x] 4.2 Implement `CREATE TABLE IF NOT EXISTS artefacts (...)` with `TEXT artefact_id PK`, `TEXT artefact_type`, `TEXT content_hash`, `JSONB payload`, `TEXT producer_role`, `TEXT created_at`
- [x] 4.3 Implement `store_artefact(artefact_type, payload) -> StoredArtefactRef` — inserts row, returns new-style `db://` URI
- [x] 4.4 Implement `get_payload(storage_uri) -> Option<String>` — fetches payload by URI (handles both `db://` and `file://`)
- [x] 4.5 Implement `read_artefact_payload(storage_uri)` — handles both `db://artefacts/` and legacy `file://` URIs
- [x] 4.6 Remove dead `store_artefact_blob()` from `roles/src/artefacts.rs` — no callers remain
- [x] 4.7 Add `ArtefactStore` to `OrganisationRuntime` and pass to all roles that produce artefacts (IntentLead, Scholar, ProjectManager, Auditor, Worker)
- [x] 4.8 Implement transactional `store_and_publish_event()` — artefact INSERT + event INSERT in same Postgres transaction

## 5. SQLite to Postgres Migration Tool

- [x] 5.1 Create `crates/migration/src/main.rs` that reads from SQLite `events` and `memories` databases and writes to Postgres
- [x] 5.2 Handle `storage_uri` rewrite: relocate `.mmat/artefacts/` blobs into Postgres `artefacts` table, update URIs from `file://` to `db://`
- [x] 5.3 Add `--dry-run` flag for validation before execution

## 6. Test Infrastructure

- [x] 6.1 Add Postgres service + `DATABASE_URL` support for CI-backed Postgres tests
- [x] 6.2 Update `event-stream/tests/integration.rs` — add Postgres store tests with unique schema per test
- [x] 6.3 Update `memory/tests/integration.rs` — add Postgres memory store tests with unique schema per test
- [x] 6.4 Update `roles/tests/auditor_tests.rs` and `role_flow_tests.rs` — update artefact reads to use new URI format
- [x] 6.5 Add `PgEventStore` unit tests: concurrent writes, replay, variant filtering, empty store
- [x] 6.6 Add `PgMemoryStore` unit tests: CRUD, supersession chains, decay queries, authority filtering
- [x] 6.7 Add `ArtefactStore` unit tests: store/retrieve, `db://` URI handling, transactional commit/rollback
- [x] 6.8 Add `docker-compose.yml` with Postgres + pgvector image for local development

## 7. CI and Documentation

- [x] 7.1 Update GitHub Actions workflow — add `services.postgres` with `postgres:16` + pgvector image
- [x] 7.2 Update `.env.example` or equivalent with `DATABASE_URL` connection string
- [x] 7.3 Add `**/.mmat/` removal note — directory no longer created (unless running legacy SQLite mode)
- [x] 7.4 Run `cargo fmt --all`, `cargo clippy -- -D warnings`, `cargo test` — fix any issues
