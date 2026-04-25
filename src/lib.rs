use naaf_llm::{OpenAiError, WebSearchError};
use thiserror::Error;

pub mod liveview;
pub mod workflow;

#[derive(Debug, Error)]
pub enum MmatError {
    #[error("configuration error: {0}")]
    Config(String),
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
