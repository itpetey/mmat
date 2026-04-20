use std::{env, io::BufRead, path::Path};

use futures::future::LocalBoxFuture;
use naaf_llm::{HumanAnswer, HumanIO, HumanQuestion, Tool, ToolSpec, WebSearchTool, repository};
use naaf_tui::{EventSender, TuiEvent};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::oneshot;
use tracing::Level;

use crate::{
    artifacts::RunArtifact,
    error::AppError,
    models::{ImplementationItemResult, RunSummary, TaskCard},
    run_store::RunStore,
};

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
pub(crate) enum RuntimeMode {
    Tui(EventSender),
    NonInteractive,
}

#[derive(Clone, Debug)]
pub(crate) struct AppRuntime {
    mode: RuntimeMode,
    project_root: std::path::PathBuf,
    run_store: RunStore,
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
    pub(crate) fn new(
        tui: EventSender,
        project_root: std::path::PathBuf,
    ) -> Result<Self, AppError> {
        let run_store = RunStore::create(&project_root)?;
        Ok(Self {
            mode: RuntimeMode::Tui(tui),
            project_root,
            run_store,
        })
    }

    pub(crate) fn new_non_interactive(project_root: std::path::PathBuf) -> Result<Self, AppError> {
        let run_store = RunStore::create(&project_root)?;
        Ok(Self {
            mode: RuntimeMode::NonInteractive,
            project_root,
            run_store,
        })
    }

    fn send_event(&self, event: TuiEvent) -> Result<(), AppError> {
        match &self.mode {
            RuntimeMode::Tui(sender) => sender.send(event).map_err(|_| AppError::TuiClosed),
            RuntimeMode::NonInteractive => {
                if let TuiEvent::Log {
                    level,
                    target,
                    message,
                } = event
                {
                    let level_str = match level {
                        Level::ERROR => "ERROR",
                        Level::WARN => "WARN ",
                        Level::INFO => "INFO ",
                        _ => "DEBUG",
                    };
                    eprintln!("[{level_str}] [{target}] {message}");
                }
                Ok(())
            }
        }
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

    pub(crate) fn run_id(&self) -> &str {
        self.run_store.run_id()
    }

    pub(crate) fn run_root(&self) -> &Path {
        self.run_store.run_root()
    }

    pub(crate) fn persist_artifact<T>(
        &self,
        artifact: RunArtifact,
        value: &T,
    ) -> Result<(), AppError>
    where
        T: Serialize + ?Sized,
    {
        self.run_store.write_json(artifact, value)
    }

    pub(crate) fn persist_run_summary(&self, summary: &RunSummary) -> Result<(), AppError> {
        self.persist_artifact(RunArtifact::RunSummary, summary)
    }

    pub(crate) fn persist_task_card(&self, task_card: &TaskCard) -> Result<(), AppError> {
        self.run_store.write_task_card(&task_card.id, task_card)
    }

    pub(crate) fn persist_task_result(
        &self,
        task_result: &ImplementationItemResult,
    ) -> Result<(), AppError> {
        self.run_store
            .write_task_result(&task_result.item_id, task_result)
    }
}

impl HumanIO for AppRuntime {
    type Error = AppError;

    fn ask<'a>(
        &'a self,
        question: HumanQuestion,
    ) -> LocalBoxFuture<'a, Result<HumanAnswer, Self::Error>> {
        Box::pin(async move {
            match &self.mode {
                RuntimeMode::Tui(sender) => {
                    let (reply_tx, reply_rx) = oneshot::channel();
                    sender
                        .send(TuiEvent::HumanPrompt {
                            question: question.question,
                            choices: question.choices.unwrap_or_default(),
                            reply: reply_tx,
                        })
                        .map_err(|_| AppError::TuiClosed)?;

                    let answer = reply_rx.await.map_err(|_| AppError::PromptClosed)?;
                    Ok(HumanAnswer { content: answer })
                }
                RuntimeMode::NonInteractive => {
                    eprintln!("\n{}", question.question);
                    let stdin = std::io::stdin();
                    let mut answer = String::new();
                    stdin.lock().read_line(&mut answer).map_err(|error| {
                        AppError::Config(format!("failed to read stdin: {error}"))
                    })?;
                    Ok(HumanAnswer {
                        content: answer.trim().to_string(),
                    })
                }
            }
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
