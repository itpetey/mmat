use event_stream::event_store::EventStoreError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid memory type: {0}")]
    InvalidMemoryType(String),

    #[error("Invalid memory scope: {0}")]
    InvalidMemoryScope(String),

    #[error("Invalid authority: {0}")]
    InvalidAuthority(String),

    #[error("Invalid confidence value: {0}")]
    InvalidConfidence(f64),

    #[error("Invalid decay policy: {0}")]
    InvalidDecayPolicy(String),

    #[error("Build error: {0}")]
    BuildError(String),

    #[error("Store error: {0}")]
    Store(String),

    #[error("Qdrant error: {0}")]
    Qdrant(String),

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Event bus error: {0}")]
    EventBus(String),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),
}
