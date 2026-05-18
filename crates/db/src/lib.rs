use diesel::{QueryResult, prelude::*};
use diesel_async::{
    AsyncConnection, SimpleAsyncConnection,
    pooled_connection::{AsyncDieselConnectionManager, PoolError},
};
use mmat_event_stream::event::EventId;
use thiserror::Error;

pub use diesel_async::{
    AsyncPgConnection,
    pooled_connection::bb8::{Pool, PooledConnection, RunError},
};

pub mod artefact;
pub mod event;
pub mod lane;
pub mod memory;
pub mod models;
pub mod project;
pub mod schema;

pub use project::{insert_project, load_projects};

type Result<T, E = DbError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database connection error: {0}")]
    DbConnection(#[from] ConnectionError),

    #[error("database error: {0}")]
    Diesel(#[from] diesel::result::Error),

    #[error("pool error: {0}")]
    Pool(String),

    #[error("event JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("transaction error: {0}")]
    Transaction(String),

    #[error("duplicate event id: {0}")]
    DuplicateEventId(EventId),
}

impl From<PoolError> for DbError {
    fn from(e: PoolError) -> Self {
        DbError::Pool(e.to_string())
    }
}

pub async fn connect(url: &str) -> Result<AsyncPgConnection> {
    Ok(AsyncPgConnection::establish(url).await?)
}

pub async fn new_pool(url: &str) -> Result<Pool<AsyncPgConnection>, PoolError> {
    let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(url);
    Pool::builder().build(config).await
}

/// Execute a raw SQL statement (e.g. schema setup).
pub async fn execute_sql(connection: &mut AsyncPgConnection, sql: &str) -> QueryResult<()> {
    SimpleAsyncConnection::batch_execute(connection, sql).await
}

pub async fn begin_transaction(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    execute_sql(connection, "BEGIN").await
}

pub async fn commit_transaction(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    execute_sql(connection, "COMMIT").await
}

pub async fn rollback_transaction(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    execute_sql(connection, "ROLLBACK").await
}

pub fn now_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
