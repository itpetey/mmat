use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use clap::Parser;
use error::AppError;
use runtime::AppRuntime;
use workflow::run_mmat;
use ws::{UiState, WsAppBuilder, WsLayer, spawn_event_translator};

mod artifacts;
mod error;
mod models;
mod parsing;
mod prompts;
mod run_store;
mod runtime;
mod workflow;
mod ws;

#[derive(Debug, Parser)]
#[command(
    name = "mmat",
    about = "Make Me A Thing — interactive planning and implementation via browser UI"
)]
struct Cli {
    /// Write WS debug events and state snapshots to this log file
    #[arg(long, value_name = "PATH")]
    debug_log: Option<PathBuf>,

    /// Project prompt to start immediately (bypasses the browser input screen)
    #[arg(long, value_name = "PROMPT")]
    prompt: Option<String>,

    /// Override the project root directory
    #[arg(long, value_name = "DIR")]
    project_root: Option<PathBuf>,

    /// Resume a previous run from its run directory
    #[arg(long, value_name = "DIR")]
    resume: Option<PathBuf>,

    /// Print run artifact paths to stdout and exit after the workflow
    #[arg(long)]
    export_artifacts: bool,

    /// Address for the WebSocket server
    #[arg(long, default_value = "127.0.0.1:8080")]
    ws_addr: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let cli = Cli::parse();

    let project_root = cli
        .project_root
        .clone()
        .or_else(|| env::current_dir().ok())
        .ok_or_else(|| AppError::Config("failed to determine project root".to_string()))?;

    if cli.prompt.is_some() || cli.resume.is_some() {
        run_non_interactive(&cli, project_root).await
    } else {
        run_interactive(&cli, project_root).await
    }
}

async fn run_interactive(cli: &Cli, project_root: PathBuf) -> Result<(), AppError> {
    let ui_state = Arc::new(UiState::new());

    let builder = WsAppBuilder::default()
        .addr(cli.ws_addr)
        .with_ui_state(ui_state.clone());

    let (sender, handle, instruction_rx, event_rx) = builder
        .spawn_with_input()
        .map_err(|error| AppError::Config(format!("failed to start server: {error}")))?;

    let layer = WsLayer::new(sender.clone());
    let translator = spawn_event_translator(event_rx, ui_state.clone());
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().with(layer).init();

    let runtime = AppRuntime::new(sender.clone(), ui_state.clone(), project_root)?;
    runtime.log_info(format!(
        "Run `{}` artifacts will be written to `{}`.",
        runtime.run_id(),
        runtime.run_root().display()
    ))?;
    runtime.log_info(format!(
        "MMAT is ready. Open http://{addr} in your browser to begin.",
        addr = cli.ws_addr
    ))?;

    let result = async {
        let instruction = instruction_rx.await.map_err(|_| AppError::PromptClosed)?;
        runtime.log_info("Prompt received. Starting discovery.")?;
        run_mmat(&runtime, instruction).await
    }
    .await;

    let show_completion_hint = match result {
        Ok(outcome) => {
            runtime.log_info(format!(
                "Workflow status: {}. {}",
                outcome.status, outcome.next_step
            ))?;
            true
        }
        Err(AppError::PromptClosed) => false,
        Err(error) => {
            runtime.log_error(format!("Workflow failed: {error}"))?;
            true
        }
    };

    if show_completion_hint {
        runtime.log_info("Workflow complete. The server will remain active until Ctrl+C.")?;
    }

    tokio::signal::ctrl_c()
        .await
        .map_err(|error| AppError::Config(format!("failed to listen for shutdown: {error}")))?;

    let _ = sender.send(ws::FrontendEvent::Quit);
    translator.abort();
    handle
        .shutdown()
        .await
        .map_err(|error| AppError::Config(format!("failed to shut down server: {error}")))?;
    Ok(())
}

async fn run_non_interactive(cli: &Cli, project_root: PathBuf) -> Result<(), AppError> {
    let prompt = cli.prompt.clone().ok_or_else(|| {
        AppError::Config("--prompt is required when not running in interactive mode".to_string())
    })?;

    let runtime = AppRuntime::new_non_interactive(project_root)?;
    runtime.log_info(format!(
        "Run `{}` artifacts will be written to `{}`.",
        runtime.run_id(),
        runtime.run_root().display()
    ))?;
    runtime.log_info("MMAT is running in non-interactive mode.")?;

    let result = run_mmat(&runtime, prompt).await;

    match result {
        Ok(outcome) => {
            runtime.log_info(format!(
                "Workflow status: {}. {}",
                outcome.status, outcome.next_step
            ))?;
        }
        Err(error) => {
            runtime.log_error(format!("Workflow failed: {error}"))?;
            return Err(error);
        }
    }

    if cli.export_artifacts {
        for entry in std::fs::read_dir(runtime.run_root())
            .map_err(|error| AppError::Config(format!("failed to read run directory: {error}")))?
        {
            let entry = entry.map_err(|error| {
                AppError::Config(format!("failed to read directory entry: {error}"))
            })?;
            if entry.path().is_file() {
                println!("{}", entry.path().display());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::Cli;

    #[test]
    fn parses_debug_log_flag() {
        let cli = Cli::try_parse_from(["mmat", "--debug-log", "target/ws-debug.log"])
            .expect("debug log flag should parse");

        assert_eq!(cli.debug_log, Some("target/ws-debug.log".into()));
    }

    #[test]
    fn help_lists_debug_log_flag() {
        let help = Cli::command().render_help().to_string();

        assert!(help.contains("--debug-log <PATH>"));
    }

    #[test]
    fn parses_prompt_flag() {
        let cli = Cli::try_parse_from(["mmat", "--prompt", "build a todo app"])
            .expect("prompt flag should parse");

        assert_eq!(cli.prompt.as_deref(), Some("build a todo app"));
    }

    #[test]
    fn parses_project_root_flag() {
        let cli = Cli::try_parse_from(["mmat", "--project-root", "/tmp/my-project"])
            .expect("project root flag should parse");

        assert_eq!(
            cli.project_root,
            Some(std::path::PathBuf::from("/tmp/my-project"))
        );
    }

    #[test]
    fn parses_export_artifacts_flag() {
        let cli = Cli::try_parse_from(["mmat", "--export-artifacts"])
            .expect("export artifacts flag should parse");

        assert!(cli.export_artifacts);
    }

    #[test]
    fn parses_resume_flag() {
        let cli = Cli::try_parse_from(["mmat", "--resume", ".mmat/runs/run-1"])
            .expect("resume flag should parse");

        assert_eq!(
            cli.resume,
            Some(std::path::PathBuf::from(".mmat/runs/run-1"))
        );
    }

    #[test]
    fn parses_ws_addr_flag() {
        let cli = Cli::try_parse_from(["mmat", "--ws-addr", "0.0.0.0:9090"])
            .expect("ws addr flag should parse");

        assert_eq!(
            cli.ws_addr,
            std::net::SocketAddr::from(([0, 0, 0, 0], 9090))
        );
    }
}
