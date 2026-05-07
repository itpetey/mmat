use reqwest::StatusCode;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, LlmError>;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("API error {status}: {message}")]
    Api { status: StatusCode, message: String },

    #[error("Stream error: {0}")]
    Stream(String),
}
