pub mod chat;
pub mod projects;

#[cfg(feature = "server")]
use std::sync::OnceLock;

#[cfg(feature = "server")]
static DB: OnceLock<tokio::sync::Mutex<mmat_db::AsyncPgConnection>> = OnceLock::new();

#[cfg(feature = "server")]
type DbGuard = tokio::sync::MutexGuard<'static, mmat_db::AsyncPgConnection>;

#[cfg(feature = "server")]
pub async fn db() -> dioxus::prelude::ServerFnResult<DbGuard> {
    if let Some(connection) = DB.get() {
        return Ok(connection.lock().await);
    }

    let url = crate::cli::pg_dsn();
    let connection = mmat_db::connect(&url).await.map_err(|error| {
        dioxus::prelude::ServerFnError::new(format!("could not connect to database: {error}"))
    })?;

    if DB.set(tokio::sync::Mutex::new(connection)).is_err() {
        // Another request initialised the shared connection first.
    }

    let connection = DB.get().ok_or_else(|| {
        dioxus::prelude::ServerFnError::new("database connection was not initialised")
    })?;

    Ok(connection.lock().await)
}
