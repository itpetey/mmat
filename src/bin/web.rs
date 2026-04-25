use std::{net::SocketAddr, sync::Arc};

use clap::Parser;
use mmat::{
    liveview::{
        FrontendEvent, LiveViewAppBuilder, RunSummaryEvent, UiState, init_liveview_tracing,
        spawn_event_translator,
    },
    workflow,
};
use naaf_llm::{AssistantMessage, ChannelHumanIO, HumanAnswer, OpenAiStreamObserver};

#[derive(Debug, Parser)]
#[command(name = "mmat-web", about = "Run the MMAT LiveView web server")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let ui_state = Arc::new(UiState::new());
    let (event_tx, ready_handle, instruction_rx, event_rx) = LiveViewAppBuilder::default()
        .addr(cli.addr)
        .with_ui_state(ui_state.clone())
        .spawn_with_input()?;
    init_liveview_tracing(event_tx.clone());
    let translator = spawn_event_translator(event_rx, ui_state);
    let handle = ready_handle.wait_for_ready().await?;

    println!("MMAT LiveView server listening on http://{}", cli.addr);

    let workflow = run_workflow_when_prompted(instruction_rx, event_tx.clone());
    tokio::pin!(workflow);

    let workflow_finished = tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result?;
            false
        }
        _ = &mut workflow => true,
    };

    if workflow_finished {
        tokio::signal::ctrl_c().await?;
    }

    translator.abort();
    handle.shutdown().await?;
    Ok(())
}

async fn run_workflow_when_prompted(
    instruction_rx: mmat::liveview::InstructionReceiver,
    event_tx: mmat::liveview::EventSender,
) {
    let Ok(prompt) = instruction_rx.await else {
        return;
    };

    send_log(
        &event_tx,
        tracing::Level::INFO,
        "Prompt received. Starting workflow.",
    );
    send_summary(&event_tx, &prompt, "running", "discovery", None);

    let (runtime, pending_questions) = ChannelHumanIO::new(1024 * 512);
    let human_bridge = tokio::spawn(forward_human_questions(pending_questions, event_tx.clone()));

    let stream_observer: Arc<dyn OpenAiStreamObserver<ChannelHumanIO>> =
        Arc::new(UiStreamObserver::new(event_tx.clone()));
    let result = workflow::greenfield(prompt.clone(), runtime, Some(stream_observer)).await;
    human_bridge.abort();

    match result {
        Ok(report) => {
            send_summary(
                &event_tx,
                &prompt,
                "completed",
                "knowledge-planning",
                Some(format!("Workflow run {} completed.", report.run_id())),
            );
            send_log(
                &event_tx,
                tracing::Level::INFO,
                format!("Workflow completed with {} node(s).", report.nodes().len()),
            );
        }
        Err(error) => {
            send_summary(
                &event_tx,
                &prompt,
                "failed",
                "workflow",
                Some(error.to_string()),
            );
            send_log(
                &event_tx,
                tracing::Level::ERROR,
                format!("Workflow failed: {error}"),
            );
        }
    }
}

async fn forward_human_questions(
    mut pending_questions: tokio::sync::mpsc::Receiver<naaf_llm::PendingQuestion>,
    event_tx: mmat::liveview::EventSender,
) {
    while let Some(pending) = pending_questions.recv().await {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if event_tx
            .send(FrontendEvent::HumanPrompt {
                question: pending.question.question,
                choices: pending.question.choices.unwrap_or_default(),
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }

        let Ok(answer) = reply_rx.await else {
            break;
        };

        let _ = pending.reply.send(HumanAnswer { content: answer });
    }
}

fn send_summary(
    event_tx: &mmat::liveview::EventSender,
    prompt: &str,
    status: &str,
    current_stage: &str,
    next_step: Option<String>,
) {
    let cwd = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let _ = event_tx.send(FrontendEvent::RunSummary(RunSummaryEvent {
        run_id: "liveview".to_string(),
        project_root: cwd.clone(),
        run_root: cwd,
        prompt: prompt.to_string(),
        status: status.to_string(),
        current_stage: current_stage.to_string(),
        next_step,
    }));
}

fn send_log(
    event_tx: &mmat::liveview::EventSender,
    level: tracing::Level,
    message: impl Into<String>,
) {
    let _ = event_tx.send(FrontendEvent::Log {
        level,
        target: "mmat::web".to_string(),
        message: message.into(),
    });
}

struct UiStreamObserver {
    event_tx: mmat::liveview::EventSender,
}

impl UiStreamObserver {
    fn new(event_tx: mmat::liveview::EventSender) -> Self {
        Self { event_tx }
    }
}

impl<R> OpenAiStreamObserver<R> for UiStreamObserver {
    fn on_content_delta(&self, _runtime: &R, delta: &str) {
        let _ = self.event_tx.send(FrontendEvent::AssistantMessageDelta {
            delta: delta.to_string(),
        });
    }

    fn on_reasoning_delta(&self, _runtime: &R, delta: &str) {
        let _ = self.event_tx.send(FrontendEvent::AssistantReasoningDelta {
            delta: delta.to_string(),
        });
    }

    fn on_response_complete(&self, _runtime: &R, message: &AssistantMessage) {
        let _ = self
            .event_tx
            .send(FrontendEvent::AssistantResponseCompleted {
                message: message.content.clone(),
            });
    }
}
