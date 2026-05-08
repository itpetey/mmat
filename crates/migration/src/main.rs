use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mmat_event_stream::event::stable_content_hash;
use rusqlite::Connection;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::{info, warn};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.contains(&"--dry-run".to_string());

    let sqlite_events = get_arg(&args, "--sqlite-events").unwrap_or_else(|| "events.db".into());
    let sqlite_memory = get_arg(&args, "--sqlite-memory").unwrap_or_else(|| "memory.db".into());
    let artefacts_dir =
        get_arg(&args, "--artefacts-dir").unwrap_or_else(|| ".mmat/artefacts".into());
    let database_url = get_arg(&args, "--database-url").expect("--database-url is required");

    if dry_run {
        info!("DRY RUN — no changes will be made");
    }

    info!("Opening SQLite databases...");
    let events_conn = Connection::open(&sqlite_events)
        .unwrap_or_else(|e| panic!("Cannot open SQLite events db at {sqlite_events}: {e}"));
    let memory_conn = Connection::open(&sqlite_memory)
        .unwrap_or_else(|e| panic!("Cannot open SQLite memory db at {sqlite_memory}: {e}"));

    info!("Connecting to Postgres...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_lazy(&database_url)
        .unwrap_or_else(|e| panic!("Cannot connect to Postgres at {database_url}: {e}"));

    if !dry_run {
        run_schema_migration(&pool).await;
    }

    let artefacts_dir = PathBuf::from(&artefacts_dir);

    migrate_events(&events_conn, &pool, dry_run).await;
    migrate_memories(&memory_conn, &pool, dry_run).await;
    let uri_map = migrate_artefacts(&artefacts_dir, &pool, dry_run).await;
    rewrite_event_uris(&pool, &uri_map, dry_run).await;

    if !dry_run {
        info!("Migration complete");
    } else {
        info!("DRY RUN complete — no changes were made");
    }
}

fn get_arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

async fn run_schema_migration(pool: &PgPool) {
    info!("Running schema migration...");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS events (
            event_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            rowid BIGSERIAL NOT NULL UNIQUE,
            variant TEXT NOT NULL,
            payload JSONB NOT NULL,
            timestamp_ns BIGINT NOT NULL,
            source_agent TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_events_rowid ON events(rowid);
        CREATE INDEX IF NOT EXISTS idx_events_variant ON events(variant);",
    )
    .execute(pool)
    .await
    .expect("Failed to create events table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS memories (
            id UUID PRIMARY KEY,
            memory_type TEXT NOT NULL,
            content TEXT NOT NULL,
            scope TEXT NOT NULL,
            authority TEXT NOT NULL,
            confidence DOUBLE PRECISION NOT NULL,
            decay_policy TEXT NOT NULL,
            evidence_refs TEXT NOT NULL DEFAULT '[]',
            supersedes UUID,
            superseded_by UUID,
            created_at TEXT NOT NULL,
            last_accessed_at TEXT NOT NULL,
            source_agent TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
        CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
        CREATE INDEX IF NOT EXISTS idx_memories_authority ON memories(authority);
        CREATE INDEX IF NOT EXISTS idx_memories_superseded_by ON memories(superseded_by);
        CREATE INDEX IF NOT EXISTS idx_memories_decay ON memories(decay_policy, created_at);",
    )
    .execute(pool)
    .await
    .expect("Failed to create memories table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS artefacts (
            artefact_id TEXT PRIMARY KEY,
            artefact_type TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            payload JSONB NOT NULL,
            producer_role TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_artefacts_type ON artefacts(artefact_type);",
    )
    .execute(pool)
    .await
    .expect("Failed to create artefacts table");

    info!("Schema migration complete");
}

async fn migrate_events(sqlite: &Connection, pool: &PgPool, dry_run: bool) {
    info!("Migrating events...");

    let mut stmt = sqlite
        .prepare(
            "SELECT event_id, variant, payload, timestamp_ns, source_agent
             FROM events ORDER BY rowid ASC",
        )
        .expect("Failed to prepare events select");

    let rows: Vec<(String, String, String, i64, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .expect("Failed to query events")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("Failed to read an event row");

    info!("  Found {} events to migrate", rows.len());

    if dry_run {
        return;
    }

    let mut count = 0u64;
    for (event_id, variant, payload, timestamp_ns, source_agent) in &rows {
        sqlx::query(
            "INSERT INTO events (event_id, variant, payload, timestamp_ns, source_agent)
             VALUES ($1::uuid, $2, $3::jsonb, $4, $5)
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(event_id)
        .bind(variant)
        .bind(payload)
        .bind(timestamp_ns)
        .bind(source_agent)
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("Failed to insert event {event_id}: {e}"));

        count += 1;
        #[allow(clippy::manual_is_multiple_of)]
        if count % 1000 == 0 {
            info!("    Migrated {count} events...");
        }
    }

    info!("  Migrated {count} events");
}

async fn migrate_memories(sqlite: &Connection, pool: &PgPool, dry_run: bool) {
    info!("Migrating memories...");

    let mut stmt = sqlite
        .prepare(
            "SELECT id, memory_type, content, scope, authority, confidence,
                    decay_policy, evidence_refs, supersedes, superseded_by,
                    created_at, last_accessed_at, source_agent
             FROM memories",
        )
        .expect("Failed to prepare memories select");

    struct MemoryRow {
        id: String,
        memory_type: String,
        content: String,
        scope: String,
        authority: String,
        confidence: f64,
        decay_policy: String,
        evidence_refs: String,
        supersedes: Option<String>,
        superseded_by: Option<String>,
        created_at: String,
        last_accessed_at: String,
        source_agent: String,
    }

    let rows: Vec<MemoryRow> = stmt
        .query_map([], |row| {
            Ok(MemoryRow {
                id: row.get(0)?,
                memory_type: row.get(1)?,
                content: row.get(2)?,
                scope: row.get(3)?,
                authority: row.get(4)?,
                confidence: row.get(5)?,
                decay_policy: row.get(6)?,
                evidence_refs: row.get(7)?,
                supersedes: row.get(8)?,
                superseded_by: row.get(9)?,
                created_at: row.get(10)?,
                last_accessed_at: row.get(11)?,
                source_agent: row.get(12)?,
            })
        })
        .expect("Failed to query memories")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("Failed to read a memory row");

    info!("  Found {} memories to migrate", rows.len());

    if dry_run {
        return;
    }

    let mut count = 0u64;
    for row in &rows {
        sqlx::query(
            "INSERT INTO memories (id, memory_type, content, scope, authority, confidence,
                                   decay_policy, evidence_refs, supersedes, superseded_by,
                                   created_at, last_accessed_at, source_agent)
             VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9::uuid, $10::uuid, $11, $12, $13)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&row.id)
        .bind(&row.memory_type)
        .bind(&row.content)
        .bind(&row.scope)
        .bind(&row.authority)
        .bind(row.confidence)
        .bind(&row.decay_policy)
        .bind(&row.evidence_refs)
        .bind(&row.supersedes)
        .bind(&row.superseded_by)
        .bind(&row.created_at)
        .bind(&row.last_accessed_at)
        .bind(&row.source_agent)
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("Failed to insert memory {}: {e}", row.id));

        count += 1;
        #[allow(clippy::manual_is_multiple_of)]
        if count % 1000 == 0 {
            info!("    Migrated {count} memories...");
        }
    }

    info!("  Migrated {count} memories");
}

async fn migrate_artefacts(
    artefacts_dir: &PathBuf,
    pool: &PgPool,
    dry_run: bool,
) -> Arc<HashMap<String, String>> {
    info!("Migrating artefacts from {}...", artefacts_dir.display());

    if !artefacts_dir.exists() {
        info!("  Artefacts directory does not exist, skipping");
        return Arc::new(HashMap::new());
    }

    let dir_entries: Vec<_> = match std::fs::read_dir(artefacts_dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(e) => {
            warn!("  Cannot read artefacts directory: {e}");
            return Arc::new(HashMap::new());
        }
    };

    if dir_entries.is_empty() {
        info!("  No artefact files found");
        return Arc::new(HashMap::new());
    }

    info!("  Found {} artefact files", dir_entries.len());

    let mut map_inner = HashMap::new();
    let mut count = 0u64;

    for entry in &dir_entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let artefact_id = filename.to_string();
        let artefact_type = filename.split('-').next().unwrap_or("unknown").to_string();

        let payload = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("  Cannot read {}: {e}", path.display());
                continue;
            }
        };

        let content_hash = stable_content_hash(&payload);
        let now_ts = now_iso8601();

        if !dry_run {
            sqlx::query(
                "INSERT INTO artefacts (artefact_id, artefact_type, content_hash, payload, created_at)
                 VALUES ($1, $2, $3, $4::jsonb, $5)
                 ON CONFLICT (artefact_id) DO NOTHING",
            )
            .bind(&artefact_id)
            .bind(&artefact_type)
            .bind(&content_hash)
            .bind(&payload)
            .bind(&now_ts)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("Failed to insert artefact {artefact_id}: {e}"));
        }

        let old_uri = format!("file://{}", path.display());
        let new_uri = format!("db://artefacts/{artefact_id}");
        map_inner.insert(old_uri, new_uri);

        count += 1;
    }

    info!("  Migrated {count} artefacts");
    Arc::new(map_inner)
}

async fn rewrite_event_uris(pool: &PgPool, uri_map: &HashMap<String, String>, dry_run: bool) {
    if uri_map.is_empty() {
        info!("No artefact URIs to rewrite");
        return;
    }

    info!("Rewriting event payload URIs...");

    if dry_run {
        info!(
            "  Would rewrite up to {} artefact URI mappings",
            uri_map.len()
        );
        return;
    }

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT event_id::text, payload::text FROM events WHERE payload::text LIKE '%file://%'",
    )
    .fetch_all(pool)
    .await
    .expect("Failed to query events with file URIs");

    info!("  Found {} events with file:// URIs", rows.len());

    let mut rewritten = 0u64;
    for (event_id, payload_json) in &rows {
        let mut payload: serde_json::Value = serde_json::from_str(payload_json)
            .unwrap_or_else(|e| panic!("Failed to parse event {event_id} payload: {e}"));

        let storage_uri_opt = payload
            .as_object()
            .and_then(|obj| obj.get("storage_uri"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let Some(old_uri) = storage_uri_opt
            && let Some(new_uri) = uri_map.get(&old_uri)
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert(
                "storage_uri".to_string(),
                serde_json::Value::String(new_uri.clone()),
            );
            let updated = serde_json::to_string(&payload)
                .unwrap_or_else(|e| panic!("Failed to serialize event {event_id}: {e}"));

            sqlx::query("UPDATE events SET payload = $1::jsonb WHERE event_id = $2::uuid")
                .bind(&updated)
                .bind(event_id)
                .execute(pool)
                .await
                .unwrap_or_else(|e| panic!("Failed to update event {event_id}: {e}"));

            rewritten += 1;
        }
    }

    info!("  Rewrote {rewritten} event payload URIs");
}
