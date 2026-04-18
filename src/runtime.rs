use std::{env, path::Path};

use futures::future::LocalBoxFuture;
use naaf_llm::{HumanAnswer, HumanIO, HumanQuestion, Tool, ToolSpec, WebSearchTool, repository};
use naaf_tui::{EventSender, TuiEvent};
use serde_json::Value;
use tokio::sync::oneshot;
use tracing::Level;

use crate::error::AppError;

pub(crate) struct AppWebSearchTool {
    inner: WebSearchTool<AppRuntime>,
}

pub(crate) struct AppReadFileTool {
    inner: repository::ReadFileTool<AppRuntime>,
}

pub(crate) struct AppGlobPathsTool {
    inner: repository::GlobPathsTool<AppRuntime>,
}

pub(crate) struct AppSearchFilesTool {
    inner: repository::SearchFilesTool<AppRuntime>,
}

#[derive(Clone, Debug)]
pub(crate) struct AppRuntime {
    tui: EventSender,
    project_root: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct WebSearchConfig {
    pub(crate) url: String,
    pub(crate) api_key: Option<String>,
}

impl AppWebSearchTool {
    pub(crate) fn new(config: &WebSearchConfig) -> Self {
        let mut inner = WebSearchTool::new(config.url.clone());
        if let Some(api_key) = &config.api_key {
            inner = inner.with_api_key(api_key.clone());
        }
        Self { inner }
    }
}

impl AppReadFileTool {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self {
            inner: repository::ReadFileTool::new(root),
        }
    }
}

impl AppGlobPathsTool {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self {
            inner: repository::GlobPathsTool::new(root),
        }
    }
}

impl AppSearchFilesTool {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self {
            inner: repository::SearchFilesTool::new(root),
        }
    }
}

impl AppRuntime {
    pub(crate) fn new(tui: EventSender, project_root: std::path::PathBuf) -> Self {
        Self { tui, project_root }
    }

    fn send_event(&self, event: TuiEvent) -> Result<(), AppError> {
        self.tui.send(event).map_err(|_| AppError::TuiClosed)
    }

    fn log(&self, level: Level, message: impl Into<String>) -> Result<(), AppError> {
        self.send_event(TuiEvent::Log {
            level,
            target: "mmat".to_string(),
            message: message.into(),
        })
    }

    pub(crate) fn log_info(&self, message: impl Into<String>) -> Result<(), AppError> {
        self.log(Level::INFO, message)
    }

    pub(crate) fn log_warn(&self, message: impl Into<String>) -> Result<(), AppError> {
        self.log(Level::WARN, message)
    }

    pub(crate) fn log_error(&self, message: impl Into<String>) -> Result<(), AppError> {
        self.log(Level::ERROR, message)
    }

    pub(crate) fn project_root(&self) -> &Path {
        &self.project_root
    }
}

impl HumanIO for AppRuntime {
    type Error = AppError;

    fn ask<'a>(
        &'a self,
        question: HumanQuestion,
    ) -> LocalBoxFuture<'a, Result<HumanAnswer, Self::Error>> {
        Box::pin(async move {
            let (reply_tx, reply_rx) = oneshot::channel();
            self.send_event(TuiEvent::HumanPrompt {
                question: question.question,
                choices: question.choices.unwrap_or_default(),
                reply: reply_tx,
            })?;

            let answer = reply_rx.await.map_err(|_| AppError::PromptClosed)?;
            Ok(HumanAnswer { content: answer })
        })
    }
}

impl WebSearchConfig {
    pub(crate) fn from_env() -> Option<Self> {
        let url = env::var("MMAT_WEB_SEARCH_URL")
            .ok()
            .or_else(|| env::var("WEB_SEARCH_URL").ok())?;
        let api_key = env::var("MMAT_WEB_SEARCH_API_KEY")
            .ok()
            .or_else(|| env::var("WEB_SEARCH_API_KEY").ok());

        Some(Self { url, api_key })
    }
}

impl Tool for AppWebSearchTool {
    type Runtime = AppRuntime;
    type Error = AppError;

    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    fn call<'a>(
        &'a self,
        runtime: &'a Self::Runtime,
        arguments: Value,
    ) -> LocalBoxFuture<'a, Result<Value, Self::Error>> {
        Box::pin(async move {
            self.inner
                .call(runtime, arguments)
                .await
                .map_err(AppError::from)
        })
    }
}

impl Tool for AppReadFileTool {
    type Runtime = AppRuntime;
    type Error = AppError;

    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    fn call<'a>(
        &'a self,
        runtime: &'a Self::Runtime,
        arguments: Value,
    ) -> LocalBoxFuture<'a, Result<Value, Self::Error>> {
        Box::pin(async move {
            let result: Result<Value, std::convert::Infallible> =
                self.inner.call(runtime, arguments).await;
            result.map_err(|never| match never {})
        })
    }
}

impl Tool for AppGlobPathsTool {
    type Runtime = AppRuntime;
    type Error = AppError;

    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    fn call<'a>(
        &'a self,
        runtime: &'a Self::Runtime,
        arguments: Value,
    ) -> LocalBoxFuture<'a, Result<Value, Self::Error>> {
        Box::pin(async move {
            let result: Result<Value, std::convert::Infallible> =
                self.inner.call(runtime, arguments).await;
            result.map_err(|never| match never {})
        })
    }
}

impl Tool for AppSearchFilesTool {
    type Runtime = AppRuntime;
    type Error = AppError;

    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    fn call<'a>(
        &'a self,
        runtime: &'a Self::Runtime,
        arguments: Value,
    ) -> LocalBoxFuture<'a, Result<Value, Self::Error>> {
        Box::pin(async move {
            let result: Result<Value, std::convert::Infallible> =
                self.inner.call(runtime, arguments).await;
            result.map_err(|never| match never {})
        })
    }
}
