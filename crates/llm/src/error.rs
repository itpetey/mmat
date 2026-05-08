//! Error types for the LLM client crate.

use reqwest::StatusCode;
use thiserror::Error;

/// Alias for [`Result`](std::result::Result) with the crate's [`LlmError`].
pub type Result<T> = std::result::Result<T, LlmError>;

/// Errors that may occur when interacting with an LLM provider.
#[derive(Error, Debug)]
pub enum LlmError {
    /// An HTTP transport error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON serialisation or deserialisation error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The API returned a non-success status code.
    #[error("API error {status}: {message}")]
    Api { status: StatusCode, message: String },

    /// An error encountered while processing a streaming response.
    #[error("Stream error: {0}")]
    Stream(String),
}
