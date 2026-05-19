#[cfg(feature = "server")]
use std::sync::OnceLock;

pub mod chat;
pub mod projects;

#[cfg(feature = "server")]
type DbPool = mmat_db::Pool<mmat_db::AsyncPgConnection>;

#[cfg(feature = "server")]
static DB: OnceLock<DbPool> = OnceLock::new();

#[cfg(feature = "server")]
pub async fn db() -> dioxus::prelude::ServerFnResult<&'static DbPool> {
    if let Some(pool) = DB.get() {
        return Ok(pool);
    }

    let url = crate::cli::pg_dsn();
    let pool = mmat_db::new_pool(&url).await.map_err(|error| {
        dioxus::prelude::ServerFnError::new(format!("could not create database pool: {error}"))
    })?;

    if DB.set(pool).is_err() {
        // Another request initialised the shared pool first.
    }

    DB.get()
        .ok_or_else(|| dioxus::prelude::ServerFnError::new("database pool was not initialised"))
}

#[cfg(feature = "server")]
pub fn db_connection_error(error: impl std::fmt::Display) -> dioxus::prelude::ServerFnError {
    dioxus::prelude::ServerFnError::new(format!("could not get database connection: {error}"))
}
