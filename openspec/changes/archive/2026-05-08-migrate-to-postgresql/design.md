## Context

MMAT currently uses two SQLite databases (`event_store.db`, `memory.db`) and a filesystem blob store (`.mmat/artefacts/`). SQLite limits concurrent writers to one, which blocks future parallelism (multiple roles publishing events simultaneously). `.mmat/` blobs are not durable across runs — no transactional guarantee ties event creation to blob persistence. A single Postgres database unifies all three stores, adds concurrent access via connection pooling, and enables replication and backup.

## Goals / Non-Goals

**Goals:**
- Single Postgres database replaces EventStore (SQLite), MemoryStore (SQLite + Qdrant), and `.mmat/artefacts/` filesystem
- All existing public API signatures remain unchanged (trait methods, struct fields, event types)
- Connection pooling for concurrent role execution
- Transactional consistency when inserting events + artefacts together
- CI tests run against a real or containerised Postgres

**Non-Goals:**
- Replacing the in-memory `tokio::sync::broadcast` event bus (stays as-is; only persistence changes)
- Full Qdrant replacement in this change (pgvector is added alongside, migration optional later)
- Schema migrations beyond the three tables — no ORM, no Flyway-style migration framework (simple `CREATE TABLE IF NOT EXISTS` + version check)

## Decisions

**Decision 1: `sqlx` over `diesel` / `tokio-postgres`**
- `sqlx` is async-native, compile-time checked queries, minimal ORM overhead, and already the dominant Rust Postgres library.
- `diesel` adds an ORM layer that doesn't fit the existing direct-SQL patterns (`CREATE TABLE`, `SELECT`, `INSERT`, `UPDATE`).
- **Consequence**: All existing query patterns (direct SQL strings) translate nearly 1:1 to `sqlx::query!()`.

**Decision 2: pgvector alongside Qdrant (not replacement, yet)**
- Qdrant is a production dependency already wired into `MemoryStore`. Replacing it with pgvector increases this change's scope and risk.
- We add a `pgvector` column (`embedding vector(64)`) to the `memories` table and implement an `EmbeddingBackend` trait that wraps either provider.
- The existing `QdrantMemoryBackend` stays as the default; `PgVectorMemoryBackend` is the alternative. Both implement the same trait.
- **Consequence**: Future change can make pgvector the default and remove Qdrant once validated. This change only adds the option.

**Decision 3: No ORM, no migration framework**
- `CREATE TABLE IF NOT EXISTS` + a `schema_version` row checked on startup. This matches the existing SQLite approach.
- If schema changes are needed later, a simple numeric version migrate-up function handles it.
- **Consequence**: Zero new dependencies beyond `sqlx`. Simple, auditable migrations.

**Decision 4: `artefacts` table as JSONB, not separate `bytea` column**
- Artefact payloads are always serialised JSON (ADR, audit report, evidence pack, patch). JSONB allows `SELECT payload->>'artefact_type'` queries and `jsonb_path_exists` for future policy checks.
- **Consequence**: Payloads >8KB use Postgres TOAST storage transparently — same as filesystem for large blobs, no special handling.

**Decision 5: Testcontainers for CI, `sqlx::test` for local**
- `testcontainers` spins up a disposable Postgres container per test session. `sqlx::test` uses a connection string from env (defaulting to a local Postgres).
- Tests parallelise by schema — each test creates its own schema and drops it on teardown.
- **Consequence**: CI needs Docker or a `services.postgres` block in GitHub Actions. No more `tempfile` + SQLite in persistence tests.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| CI requires Postgres container | Add `services.postgres` to GitHub Actions workflow; provide `docker-compose.yml` for local dev |
| pgvector extension not available on all Postgres hosts | Add optional compile-time feature `pgvector`; fall back to in-process cosine search if extension missing |
| Existing SQLite databases need migration | Provide one-shot `sqlite_to_postgres` binary (read from SQLite, write to Postgres DSN) |
| Connection pool exhausts Postgres connections | Default pool size = `num_cpus * 2`, configurable in `OrganisationConfig` |
| Test isolation — parallel tests writing to same DB | Each test creates a unique schema (`test_<uuid>`), drops on teardown via `DROP SCHEMA ... CASCADE` |

## Migration Plan

1. Add `sqlx` with `postgres` + `runtime-tokio` features to workspace dependencies; remove `rusqlite`
2. Implement `EventStore` new constructor (`new(database_url)`) and `EventStore::open(path)` — `open` still creates/opens SQLite, `new` creates/opens Postgres (dual-write or flag)
3. Implement `MemoryStore` Postgres backend with `pgvector` optional column
4. Implement `ArtefactStore` with `artefacts` table, replace `store_artefact_blob` + `read_artefact_payload` with store methods
5. Update `OrganisationConfig` to accept `database_url: String`
6. Provide `sqlite_to_postgres` migration tool
7. Update all tests to use Postgres (via `testcontainers` or env DSN)
8. Update CI workflow for Postgres service

## Open Questions

- Should we keep the hardware-dependent `rowid` concept or switch to Postgres `BIGSERIAL`? → **Decision: `BIGSERIAL`** — Postgres handles it identically to SQLite `rowid`.
- What schema name prefix for test isolation? → `test_<crate>_<test_name>` truncated to 63 chars (Postgres identifier limit).
