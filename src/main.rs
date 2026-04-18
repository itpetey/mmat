use std::{env, path::PathBuf};

use clap::Parser;
use error::AppError;
use naaf_tui::TuiAppBuilder;
use runtime::AppRuntime;
use workflow::run_mmat;

mod error;
mod models;
mod parsing;
mod prompts;
mod runtime;
mod workflow;

#[derive(Debug, Parser)]
#[command(name = "mmat", about = "Make Me A Thing")]
struct Cli {
    /// Write TUI debug events and state snapshots to this log file
    #[arg(long, value_name = "PATH")]
    debug_log: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let cli = Cli::parse();

    let mut builder = TuiAppBuilder::default()
        .title("MMAT")
        .with_input_screen("What are we building?")
        .install_tracing_layer();

    if let Some(path) = cli.debug_log {
        builder = builder.debug_log_path(path);
    }

    let (sender, handle, instruction_rx) = builder
        .spawn_with_input()
        .map_err(|error| AppError::Config(format!("failed to start TUI: {error}")))?;

    let project_root = env::current_dir()
        .map_err(|error| AppError::Config(format!("failed to read current directory: {error}")))?;
    let runtime = AppRuntime::new(sender, project_root);
    runtime.log_info("MMAT is ready. Enter a project prompt to begin.")?;

    let result = async {
        let instruction = instruction_rx.await.map_err(|_| AppError::TuiClosed)?;
        runtime.log_info("Prompt received. Starting discovery.")?;
        run_mmat(&runtime, instruction).await
    }
    .await;

    match result {
        Ok(outcome) => {
            runtime.log_info(format!(
                "Workflow status: {}. {}",
                outcome.status, outcome.next_step
            ))?;
        }
        Err(error) => {
            runtime.log_error(format!("Workflow failed: {error}"))?;
        }
    }

    runtime.log_info("Workflow complete. Press q to exit.")?;
    handle
        .shutdown()
        .await
        .map_err(|error| AppError::Config(format!("failed to shut down TUI: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::Cli;

    #[test]
    fn parses_debug_log_flag() {
        let cli = Cli::try_parse_from(["mmat", "--debug-log", "target/tui-debug.log"])
            .expect("debug log flag should parse");

        assert_eq!(cli.debug_log, Some("target/tui-debug.log".into()));
    }

    #[test]
    fn help_lists_debug_log_flag() {
        let help = Cli::command().render_help().to_string();

        assert!(help.contains("--debug-log <PATH>"));
    }
}
