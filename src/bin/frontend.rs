use std::{net::SocketAddr, process::Child, sync::Arc};

use clap::Parser;
use ipc_channel::ipc::IpcOneShotServer;
use mmat::{
    deliver::ipc::{DeliveryHandshake, DeliveryToFrontend, FrontendSender, FrontendToDelivery},
    liveview::{
        ConversationHistoryStore, DomainNodeUiSnapshot, FrontendEvent, LiveViewAppBuilder,
        RunSummaryEvent, UiState, init_liveview_tracing, spawn_event_translator,
    },
    plan,
    project::{ProjectConfig, ProjectId, ProjectRegistryStore},
};
use naaf_llm::{AssistantMessage, ChannelHumanIO, HumanAnswer, OpenAiStreamObserver};

#[derive(Debug, Parser)]
#[command(name = "frontend", about = "Run the MMAT LiveView frontend")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: SocketAddr,
}

struct DeliveryClient {
    sender: FrontendSender,
    child: Child,
    listener: std::thread::JoinHandle<()>,
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
        for tool_call in &message.tool_calls {
            let _ = send_project_event(
                &self.event_tx,
                &self.project_id,
                FrontendEvent::ToolCallStarted {
                    name: tool_call.tool_name.clone(),
                    arguments: tool_call.arguments.to_string(),
                },
            );
        }
        let _ = send_project_event(
            &self.event_tx,
            &self.project_id,
            FrontendEvent::AssistantResponseCompleted {
                message: message.content.clone(),
            },
        );
    }
}

fn default_project_root() -> std::path::PathBuf {
    mmat::project::default_project_path()
}

fn delivery_binary_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("MMAT_DELIVERY_BIN") {
        return Ok(path.into());
    }

    let mut path = std::env::current_exe()?;
    path.set_file_name("delivery");
    Ok(path)
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

fn handle_delivery_event(
    event_tx: &mmat::liveview::EventSender,
    ui_state: &UiState,
    event: DeliveryToFrontend,
) {
    match event {
        DeliveryToFrontend::Ready => {}
        DeliveryToFrontend::QueueSnapshot { project_id, jobs } => {
            ui_state.set_project_queue(&project_id, jobs);
        }
        DeliveryToFrontend::Log {
            project_id,
            level,
            message,
        } => {
            send_log(event_tx, &project_id, level.as_tracing_level(), message);
        }
        DeliveryToFrontend::JobStarted { project_id, job_id } => {
            send_log(
                event_tx,
                &project_id,
                tracing::Level::INFO,
                format!("Delivery job {job_id} started."),
            );
        }
        DeliveryToFrontend::JobFinished {
            project_id,
            job_id,
            status,
            error,
        } => {
            let level = if error.is_some() {
                tracing::Level::ERROR
            } else {
                tracing::Level::INFO
            };
            let suffix = error.map(|error| format!(": {error}")).unwrap_or_default();
            send_log(
                event_tx,
                &project_id,
                level,
                format!(
                    "Delivery job {job_id} finished as {}{suffix}.",
                    status.as_str()
                ),
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let registry_store = Arc::new(
        ProjectRegistryStore::open_default()
            .map_err(|e| format!("Failed to open project registry: {e}"))?,
    );

    registry_store
        .ensure_default_project(default_project_root())
        .map_err(|e| format!("Failed to ensure default project: {e}"))?;

    let projects = registry_store
        .list_projects()
        .map_err(|e| format!("Failed to list projects: {e}"))?;
    let conversation_store = Arc::new(
        ConversationHistoryStore::open_default()
            .map_err(|e| format!("Failed to open conversation history: {e}"))?,
    );

    let ui_state = Arc::new(UiState::with_projects_and_conversation_store(
        projects.clone(),
        Some(registry_store.clone()),
        Some(conversation_store),
    ));

    let (event_tx, ready_handle, instruction_rx, event_rx) = LiveViewAppBuilder::default()
        .addr(cli.addr)
        .with_ui_state(ui_state.clone())
        .spawn_with_input()
        .map_err(|e| format!("Failed to spawn LiveView server: {e}"))?;

    init_liveview_tracing(event_tx.clone());

    let translator = spawn_event_translator(event_rx, ui_state.clone());

    let mut delivery = start_delivery_process(&projects, event_tx.clone(), ui_state.clone())
        .map_err(|e| format!("Failed to start delivery process: {e}"))?;

    let handle = ready_handle
        .wait_for_ready()
        .await
        .map_err(|e| format!("LiveView server failed to become ready: {e}"))?;

    println!("MMAT LiveView server listening on http://{}", cli.addr);

    let plan = run_workflow_when_prompted(
        instruction_rx,
        event_tx.clone(),
        ui_state.clone(),
        registry_store,
        delivery.sender.clone(),
    );
    tokio::pin!(plan);

    let workflow_finished = tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result.map_err(|e| format!("Failed to listen for Ctrl-C: {e}"))?;
            false
        }
        _ = &mut plan => true,
    };

    if workflow_finished {
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| format!("Failed to listen for Ctrl-C: {e}"))?;
    }

    translator.abort();
    let _ = delivery.sender.send(FrontendToDelivery::Shutdown);
    let _ = delivery.child.kill();
    let _ = delivery.listener.join();
    handle
        .shutdown()
        .await
        .map_err(|e| format!("Failed to shut down LiveView server: {e}"))?;
    Ok(())
}

async fn run_workflow_when_prompted(
    instruction_rx: mmat::liveview::InstructionReceiver,
    event_tx: mmat::liveview::EventSender,
    ui_state: Arc<UiState>,
    registry_store: Arc<ProjectRegistryStore>,
    delivery_sender: FrontendSender,
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
        "Prompt received. Starting plan.",
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
        plan::greenfield_for_project(prompt.clone(), runtime, Some(stream_observer), &project)
            .await;
    human_bridge.abort();

    match result {
        Ok(report) => {
            let tree_nodes: Vec<DomainNodeUiSnapshot> = report
                .tree
                .nodes
                .values()
                .map(|node| DomainNodeUiSnapshot {
                    node_id: node.id,
                    name: node.name.clone(),
                    status: format!("{:?}", node.status).to_ascii_lowercase(),
                    phase: match node.status {
                        crate::plan::domain_map::DomainNodeStatus::Ready
                        | crate::plan::domain_map::DomainNodeStatus::KnowledgeMaterialised
                        | crate::plan::domain_map::DomainNodeStatus::SolutionsCollected
                        | crate::plan::domain_map::DomainNodeStatus::SolutionChosen
                        | crate::plan::domain_map::DomainNodeStatus::ArchitectComplete
                        | crate::plan::domain_map::DomainNodeStatus::Delivering
                        | crate::plan::domain_map::DomainNodeStatus::Complete => {
                            "Ready".to_string()
                        }
                        _ => "Discovery".to_string(),
                    },
                    depth: node.depth,
                })
                .collect();

            ui_state.set_project_domain_tree_nodes(&project_id, tree_nodes);
            ui_state.set_project_delivery_graph(&project_id, Some(report.delivery_graph));

            for (node_id, handoff) in &report.node_handoffs {
                if let Err(error) = delivery_sender.send(FrontendToDelivery::Enqueue {
                    project_id: project_id.clone(),
                    domain_node_id: Some(*node_id),
                    handoff: handoff.clone(),
                }) {
                    send_log(
                        &event_tx,
                        &project_id,
                        tracing::Level::ERROR,
                        format!("Delivery enqueue failed: {error}"),
                    );
                }
            }

            send_summary(
                &event_tx,
                &project,
                &prompt,
                "completed",
                "knowledge-planning",
                Some(format!("Domain-mapped run {} completed.", report.run_id)),
            );
            send_log(
                &event_tx,
                &project_id,
                tracing::Level::INFO,
                format!(
                    "Plan completed with {} sub-domain handoff(s).",
                    report.node_handoffs.len()
                ),
            );
        }
        Err(error) => {
            send_summary(
                &event_tx,
                &project,
                &prompt,
                "failed",
                "plan",
                Some(error.to_string()),
            );
            send_log(
                &event_tx,
                &project_id,
                tracing::Level::ERROR,
                format!("Plan failed: {error}"),
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
            target: "mmat::frontend".to_string(),
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

fn start_delivery_process(
    projects: &[ProjectConfig],
    event_tx: mmat::liveview::EventSender,
    ui_state: Arc<UiState>,
) -> Result<DeliveryClient, Box<dyn std::error::Error>> {
    let (server, server_name) = IpcOneShotServer::<DeliveryHandshake>::new()?;
    let child = std::process::Command::new(delivery_binary_path()?)
        .arg("--ipc-server")
        .arg(server_name)
        .spawn()?;
    let (_, handshake) = server.accept()?;
    let sender = handshake.frontend_tx;
    let receiver = handshake.delivery_rx;
    sender.send(FrontendToDelivery::RegisterProjects(projects.to_vec()))?;

    let listener = std::thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            handle_delivery_event(&event_tx, ui_state.as_ref(), event);
        }
    });

    Ok(DeliveryClient {
        sender,
        child,
        listener,
    })
}
