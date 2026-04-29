use std::{collections::BTreeMap, sync::Arc};

use clap::Parser;
use ipc_channel::ipc::{self, IpcSender};
use mmat::{
    deliver::{
        BuildQueueStore, BuildWorkerEvent, BuildWorkerHandle,
        ipc::{DeliveryHandshake, DeliveryLogLevel, DeliveryToFrontend, FrontendToDelivery},
        spawn_project_worker_with_events,
    },
    project::{ProjectConfig, ProjectId},
};

type WorkerMap = BTreeMap<ProjectId, (Arc<BuildQueueStore>, BuildWorkerHandle)>;

#[derive(Debug, Parser)]
#[command(name = "delivery", about = "Run the MMAT delivery worker")]
struct Cli {
    #[arg(long)]
    ipc_server: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let Some(server_name) = cli.ipc_server else {
        eprintln!("delivery requires --ipc-server when launched by frontend");
        return Ok(());
    };

    let (frontend_tx, frontend_rx) = ipc::channel::<FrontendToDelivery>()
        .map_err(|e| format!("Failed to create frontend IPC channel: {e}"))?;
    let (delivery_tx, delivery_rx) = ipc::channel::<DeliveryToFrontend>()
        .map_err(|e| format!("Failed to create delivery IPC channel: {e}"))?;
    let handshake = DeliveryHandshake {
        frontend_tx,
        delivery_rx,
    };
    IpcSender::connect(server_name.clone())
        .map_err(|e| format!("Failed to connect to IPC server '{server_name}': {e}"))?
        .send(handshake)
        .map_err(|e| format!("Failed to send handshake to frontend: {e}"))?;
    delivery_tx
        .send(DeliveryToFrontend::Ready)
        .map_err(|e| format!("Failed to send Ready signal to frontend: {e}"))?;

    run_delivery_loop(frontend_rx, delivery_tx).await
}

async fn run_delivery_loop(
    receiver: ipc::IpcReceiver<FrontendToDelivery>,
    sender: ipc::IpcSender<DeliveryToFrontend>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut workers = WorkerMap::new();

    loop {
        match receiver
            .recv()
            .map_err(|e| format!("Failed to receive frontend message: {e}"))?
        {
            FrontendToDelivery::RegisterProjects(projects) => {
                register_projects(projects, &mut workers, sender.clone())
                    .map_err(|e| format!("Failed to register projects: {e}"))?;
                refresh_queues(&workers, &sender)
                    .map_err(|e| format!("Failed to refresh queues after registration: {e}"))?;
            }
            FrontendToDelivery::Enqueue {
                project_id,
                handoff,
            } => {
                if let Some((store, worker)) = workers.get(&project_id) {
                    match store.enqueue(&project_id, handoff) {
                        Ok(_) => {
                            refresh_one_queue(&project_id, store, &sender).map_err(|e| {
                                format!("Failed to refresh queue for project {project_id}: {e}")
                            })?;
                            worker.notify();
                        }
                        Err(error) => sender
                            .send(DeliveryToFrontend::Log {
                                project_id: project_id.clone(),
                                level: DeliveryLogLevel::Error,
                                message: format!("Delivery enqueue failed: {error}"),
                            })
                            .map_err(|e| {
                                format!(
                                    "Failed to send enqueue error log for project {project_id}: {e}"
                                )
                            })?,
                    }
                } else {
                    sender
                        .send(DeliveryToFrontend::Log {
                            project_id: project_id.clone(),
                            level: DeliveryLogLevel::Error,
                            message: "Delivery worker is not registered for this project."
                                .to_string(),
                        })
                        .map_err(|e| {
                            format!(
                                "Failed to send 'not registered' log for project {project_id}: {e}"
                            )
                        })?;
                }
            }
            FrontendToDelivery::RefreshQueues => refresh_queues(&workers, &sender)
                .map_err(|e| format!("Failed to refresh queues: {e}"))?,
            FrontendToDelivery::Shutdown => {
                for (_, worker) in workers.values() {
                    worker.abort();
                }
                return Ok(());
            }
        }
    }
}

fn register_projects(
    projects: Vec<ProjectConfig>,
    workers: &mut WorkerMap,
    sender: ipc::IpcSender<DeliveryToFrontend>,
) -> Result<(), Box<dyn std::error::Error>> {
    for project in projects.into_iter().filter(|project| project.enabled) {
        if workers.contains_key(&project.id) {
            continue;
        }

        let store = Arc::new(BuildQueueStore::for_project(&project)?);
        store.recover_stale_running(&project.id)?;
        let project_id = project.id.clone();
        let event_sender = sender.clone();
        let worker =
            spawn_project_worker_with_events(project.clone(), store.clone(), move |event| {
                let _ = send_worker_event(&event_sender, event);
            });
        workers.insert(project_id, (store, worker));
    }
    Ok(())
}

fn refresh_queues(
    workers: &WorkerMap,
    sender: &ipc::IpcSender<DeliveryToFrontend>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (project_id, (store, _)) in workers {
        refresh_one_queue(project_id, store, sender)?;
    }
    Ok(())
}

fn refresh_one_queue(
    project_id: &ProjectId,
    store: &BuildQueueStore,
    sender: &ipc::IpcSender<DeliveryToFrontend>,
) -> Result<(), Box<dyn std::error::Error>> {
    sender.send(DeliveryToFrontend::QueueSnapshot {
        project_id: project_id.clone(),
        jobs: store.list_for_project(project_id)?,
    })?;
    Ok(())
}

fn send_worker_event(
    sender: &ipc::IpcSender<DeliveryToFrontend>,
    event: BuildWorkerEvent,
) -> Result<(), ipc_channel::Error> {
    match event {
        BuildWorkerEvent::QueueChanged { project_id, jobs } => {
            sender.send(DeliveryToFrontend::QueueSnapshot { project_id, jobs })
        }
        BuildWorkerEvent::JobStarted { project_id, job_id } => {
            sender.send(DeliveryToFrontend::JobStarted { project_id, job_id })
        }
        BuildWorkerEvent::JobFinished {
            project_id,
            job_id,
            status,
            error,
        } => sender.send(DeliveryToFrontend::JobFinished {
            project_id,
            job_id,
            status,
            error,
        }),
    }
}
