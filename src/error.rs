use naaf_llm::{OpenAiError, WebSearchError};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum AppError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("the WS channel closed")]
    WsClosed,
    #[error("the active prompt closed before an answer was received")]
    PromptClosed,
    #[error(transparent)]
    OpenAi(#[from] OpenAiError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    WebSearch(#[from] WebSearchError),
    #[error("workflow error: {0}")]
    Workflow(String),
    #[error("workspace error: {0}")]
    Workspace(String),
}
