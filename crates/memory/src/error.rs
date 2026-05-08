//! Error types for the memory crate.

use mmat_event_stream::event_store::EventStoreError;
use thiserror::Error;

/// Convenience type alias for results with the crate's [`enum@Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur in memory operations.
#[derive(Error, Debug)]
pub enum Error {
    /// The provided string does not correspond to a known [`crate::types::MemoryType`].
    #[error("Invalid memory type: {0}")]
    InvalidMemoryType(String),

    /// The provided string does not correspond to a known [`crate::types::MemoryScope`].
    #[error("Invalid memory scope: {0}")]
    InvalidMemoryScope(String),

    /// The provided string does not correspond to a known [`crate::types::Authority`].
    #[error("Invalid authority: {0}")]
    InvalidAuthority(String),

    /// The confidence value is outside the range `0.0..=1.0`.
    #[error("Invalid confidence value: {0}")]
    InvalidConfidence(f64),

    /// The provided string does not correspond to a known [`crate::types::DecayPolicy`].
    #[error("Invalid decay policy: {0}")]
    InvalidDecayPolicy(String),

    /// A [`crate::types::Memory`] could not be built from its builder fields.
    #[error("Build error: {0}")]
    BuildError(String),

    /// Generic store-level error.
    #[error("Store error: {0}")]
    Store(String),

    /// An error occurred in the Qdrant vector-memory backend.
    #[error("Qdrant error: {0}")]
    Qdrant(String),

    /// An error occurred when communicating with an LLM.
    #[error("LLM error: {0}")]
    Llm(String),

    /// An error occurred in the event bus.
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// An error occurred in the SQLite database layer.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// An error occurred in the Postgres database layer.
    #[error("Postgres error: {0}")]
    Postgres(#[from] sqlx::Error),

    /// An error occurred during JSON serialisation or deserialisation.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// An error occurred in the event store.
    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),

    /// The code is not running inside a Tokio runtime.
    #[error("Not in a Tokio runtime: {0}")]
    Runtime(String),
}
