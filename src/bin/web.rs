use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use clap::Parser;
use mmat::{
    build::{BuildQueueStore, BuildWorkerHandle, spawn_project_worker},
    liveview::{
        FrontendEvent, LiveViewAppBuilder, RunSummaryEvent, UiState, init_liveview_tracing,
        spawn_event_translator,
    },
    project::{ProjectConfig, ProjectId, ProjectRegistryStore},
    workflow,
};
use naaf_llm::{AssistantMessage, ChannelHumanIO, HumanAnswer, OpenAiStreamObserver};

type WorkerMap = BTreeMap<ProjectId, (Arc<BuildQueueStore>, BuildWorkerHandle)>;

#[derive(Debug, Parser)]
#[command(name = "mmat-web", about = "Run the MMAT LiveView web server")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: SocketAddr,
}

struct UiStreamObserver {
    event_tx: mmat::liveview::EventSender,
    project_id: ProjectId,
}

impl UiStreamObserver {
    fn new(event_tx: mmat::liveview::EventSender, project_id: ProjectId) -> Self {
        Self {
            event_tx,
            project_id,
        }
    }
}

impl<R> OpenAiStreamObserver<R> for UiStreamObserver {
    fn on_content_delta(&self, _runtime: &R, delta: &str) {
        let _ = send_project_event(
            &self.event_tx,
            &self.project_id,
            FrontendEvent::AssistantMessageDelta {
                delta: delta.to_string(),
            },
        );
    }

    fn on_reasoning_delta(&self, _runtime: &R, delta: &str) {
        let _ = send_project_event(
            &self.event_tx,
            &self.project_id,
            FrontendEvent::AssistantReasoningDelta {
                delta: delta.to_string(),
            },
        );
    }

    fn on_response_complete(&self, _runtime: &R, message: &AssistantMessage) {
        let _ = send_project_event(
            &self.event_tx,
            &self.project_id,
            FrontendEvent::AssistantResponseCompleted {
                message: message.content.clone(),
            },
        );
    }
}

async fn forward_human_questions(
    mut pending_questions: tokio::sync::mpsc::Receiver<naaf_llm::PendingQuestion>,
    event_tx: mmat::liveview::EventSender,
    project_id: ProjectId,
) {
    while let Some(pending) = pending_questions.recv().await {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if !send_project_event(
            &event_tx,
            &project_id,
            FrontendEvent::HumanPrompt {
                question: pending.question.question,
                choices: pending.question.choices.unwrap_or_default(),
                reply: reply_tx,
            },
        ) {
            break;
        }

        let Ok(answer) = reply_rx.await else {
            break;
        };

        let _ = pending.reply.send(HumanAnswer { content: answer });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let registry_store = Arc::new(ProjectRegistryStore::open_default()?);
    let current_dir = std::env::current_dir()?;
    registry_store.ensure_default_project(current_dir)?;
    let projects = registry_store.list_projects()?;
    let ui_state = Arc::new(UiState::with_projects(
        projects.clone(),
        Some(registry_store.clone()),
    ));
    let workers = start_project_workers(&projects, ui_state.as_ref())?;
    let (event_tx, ready_handle, instruction_rx, event_rx) = LiveViewAppBuilder::default()
        .addr(cli.addr)
        .with_ui_state(ui_state.clone())
        .spawn_with_input()?;
    init_liveview_tracing(event_tx.clone());
    let translator = spawn_event_translator(event_rx, ui_state.clone());
    let handle = ready_handle.wait_for_ready().await?;

    println!("MMAT LiveView server listening on http://{}", cli.addr);

    let workflow = run_workflow_when_prompted(
        instruction_rx,
        event_tx.clone(),
        ui_state.clone(),
        registry_store,
        workers,
    );
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

fn refresh_project_queue(
    ui_state: &UiState,
    project_id: &ProjectId,
    queue_store: &BuildQueueStore,
) {
    match queue_store.list_for_project(project_id) {
        Ok(jobs) => ui_state.set_project_queue(project_id, jobs),
        Err(error) => ui_state.push_project_event(
            project_id,
            mmat::liveview::UiEvent::Log {
                level: "ERROR".to_string(),
                message: format!("Queue refresh failed: {error}"),
            },
        ),
    }
}

async fn run_workflow_when_prompted(
    instruction_rx: mmat::liveview::InstructionReceiver,
    event_tx: mmat::liveview::EventSender,
    ui_state: Arc<UiState>,
    registry_store: Arc<ProjectRegistryStore>,
    workers: WorkerMap,
) {
    let Ok(project_prompt) = instruction_rx.await else {
        return;
    };
    let prompt = project_prompt.prompt;
    let project_id = project_prompt.project_id;
    let project = match registry_store.get_project(&project_id) {
        Ok(project) => project,
        Err(error) => {
            send_log(
                &event_tx,
                &project_id,
                tracing::Level::ERROR,
                format!("Project lookup failed: {error}"),
            );
            return;
        }
    };

    send_log(
        &event_tx,
        &project_id,
        tracing::Level::INFO,
        "Prompt received. Starting workflow.",
    );
    send_summary(&event_tx, &project, &prompt, "running", "discovery", None);

    let (runtime, pending_questions) = ChannelHumanIO::new(1024 * 512);
    let human_bridge = tokio::spawn(forward_human_questions(
        pending_questions,
        event_tx.clone(),
        project_id.clone(),
    ));

    let stream_observer: Arc<dyn OpenAiStreamObserver<ChannelHumanIO>> =
        Arc::new(UiStreamObserver::new(event_tx.clone(), project_id.clone()));
    let result =
        workflow::greenfield_for_project(prompt.clone(), runtime, Some(stream_observer), &project)
            .await;
    human_bridge.abort();

    match result {
        Ok(report) => {
            if let Some(handoff) = report.design_handoff()
                && let Some((queue_store, worker)) = workers.get(&project_id)
            {
                match queue_store.enqueue(&project_id, handoff) {
                    Ok(_) => {
                        refresh_project_queue(ui_state.as_ref(), &project_id, queue_store.as_ref());
                        worker.notify();
                        let ui_state = ui_state.clone();
                        let project_id = project_id.clone();
                        let queue_store = queue_store.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                            refresh_project_queue(
                                ui_state.as_ref(),
                                &project_id,
                                queue_store.as_ref(),
                            );
                        });
                    }
                    Err(error) => send_log(
                        &event_tx,
                        &project_id,
                        tracing::Level::ERROR,
                        format!("Build enqueue failed: {error}"),
                    ),
                }
            }
            send_summary(
                &event_tx,
                &project,
                &prompt,
                "completed",
                "knowledge-planning",
                Some(format!("Workflow run {} completed.", report.run_id())),
            );
            send_log(
                &event_tx,
                &project_id,
                tracing::Level::INFO,
                format!(
                    "Workflow completed as {} after {} step attempt(s).",
                    report.outcome_label(),
                    report.attempt_count()
                ),
            );
        }
        Err(error) => {
            send_summary(
                &event_tx,
                &project,
                &prompt,
                "failed",
                "workflow",
                Some(error.to_string()),
            );
            send_log(
                &event_tx,
                &project_id,
                tracing::Level::ERROR,
                format!("Workflow failed: {error}"),
            );
        }
    }
}

fn send_log(
    event_tx: &mmat::liveview::EventSender,
    project_id: &ProjectId,
    level: tracing::Level,
    message: impl Into<String>,
) {
    let _ = send_project_event(
        event_tx,
        project_id,
        FrontendEvent::Log {
            level,
            target: "mmat::web".to_string(),
            message: message.into(),
        },
    );
}

fn send_project_event(
    event_tx: &mmat::liveview::EventSender,
    project_id: &ProjectId,
    event: FrontendEvent,
) -> bool {
    event_tx
        .send(FrontendEvent::ProjectScoped {
            project_id: project_id.clone(),
            event: Box::new(event),
        })
        .is_ok()
}

fn send_summary(
    event_tx: &mmat::liveview::EventSender,
    project: &ProjectConfig,
    prompt: &str,
    status: &str,
    current_stage: &str,
    next_step: Option<String>,
) {
    let _ = send_project_event(
        event_tx,
        &project.id,
        FrontendEvent::RunSummary(RunSummaryEvent {
            project_id: project.id.clone(),
            run_id: "liveview".to_string(),
            project_root: project.root.display().to_string(),
            run_root: project.data_dir.display().to_string(),
            prompt: prompt.to_string(),
            status: status.to_string(),
            current_stage: current_stage.to_string(),
            next_step,
        }),
    );
}

fn start_project_workers(
    projects: &[ProjectConfig],
    ui_state: &UiState,
) -> Result<WorkerMap, Box<dyn std::error::Error>> {
    let mut workers = BTreeMap::new();
    for project in projects.iter().filter(|project| project.enabled) {
        let queue_store = Arc::new(BuildQueueStore::for_project(project)?);
        queue_store.recover_stale_running(&project.id)?;
        refresh_project_queue(ui_state, &project.id, queue_store.as_ref());
        let worker = spawn_project_worker(project.clone(), queue_store.clone());
        workers.insert(project.id.clone(), (queue_store, worker));
    }
    Ok(workers)
}
