use std::sync::Arc;
use std::time::Duration;

use mmat_event_stream::event::SemanticEvent;
use mmat_event_stream::event_bus::EventBus;
use mmat_memory::artefact_store::ArtefactStore;
use mmat_workbench::{AppState, build_app_router, spawn_projection_task};
use tokio::net::TcpListener;

/// Create a test AppState with no events (in-memory, no Postgres).
pub fn test_app_state() -> AppState {
    let bus = EventBus::new(16);
    let store = Arc::new(ArtefactStore::new());
    AppState::with_events(bus, &[], store)
}

/// Create a test AppState pre-seeded with the given events.
pub fn test_app_state_with_events(events: &[SemanticEvent]) -> AppState {
    let bus = EventBus::new(16);
    let store = Arc::new(ArtefactStore::new());
    AppState::with_events(bus, events, store)
}

/// Spawn a test HTTP server and return the base URL.
pub async fn spawn_test_server(state: AppState) -> String {
    spawn_projection_task(state.clone());
    let app = build_app_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    base_url
}

/// Create a temporary Postgres schema for a test and return the
/// (database_url_with_schema, admin_pool, schema_name).
/// Returns `None` when `DATABASE_URL` is not set.
pub async fn postgres_test_database(prefix: &str) -> Option<(String, sqlx::PgPool, String)> {
    let base_url = std::env::var("DATABASE_URL").ok()?;

    let schema = format!(
        "{}_{}",
        prefix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let admin_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&base_url)
        .await
        .ok()?;

    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin_pool)
        .await
        .ok()?;

    let separator = if base_url.contains('?') { '&' } else { '?' };
    let database_url = format!("{base_url}{separator}options=-c%20search_path%3D{schema}");

    Some((database_url, admin_pool, schema))
}

/// Drop a temporary Postgres schema created by `postgres_test_database`.
pub async fn drop_postgres_schema(pool: &sqlx::PgPool, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(pool)
        .await
        .unwrap();
}
