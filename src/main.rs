use std::env;

mod error;
mod models;
mod parsing;
mod prompts;
mod runtime;
mod workflow;

use error::AppError;
use naaf_tui::TuiAppBuilder;
use runtime::AppRuntime;
use workflow::run_mmat;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let (sender, handle, instruction_rx) = TuiAppBuilder::default()
        .title("MMAT")
        .with_input_screen("Describe the work you want planned")
        .install_tracing_layer()
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
