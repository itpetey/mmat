use serde::{Deserialize, Serialize};

use crate::{
    deliver::queue::{BuildJob, BuildJobId, BuildJobStatus},
    plan::DesignHandoff,
    project::{ProjectConfig, ProjectId},
};

pub type DeliveryReceiver = ipc_channel::ipc::IpcReceiver<DeliveryToFrontend>;
pub type DeliverySender = ipc_channel::ipc::IpcSender<DeliveryToFrontend>;
pub type FrontendReceiver = ipc_channel::ipc::IpcReceiver<FrontendToDelivery>;
pub type FrontendSender = ipc_channel::ipc::IpcSender<FrontendToDelivery>;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeliveryHandshake {
    pub frontend_tx: FrontendSender,
    pub delivery_rx: DeliveryReceiver,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FrontendToDelivery {
    RegisterProjects(Vec<ProjectConfig>),
    Enqueue {
        project_id: ProjectId,
        domain_node_id: Option<crate::plan::domain_map::DomainNodeId>,
        handoff: DesignHandoff,
    },
    RefreshQueues,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryLogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeliveryToFrontend {
    Ready,
    QueueSnapshot {
        project_id: ProjectId,
        jobs: Vec<BuildJob>,
    },
    Log {
        project_id: ProjectId,
        level: DeliveryLogLevel,
        message: String,
    },
    JobStarted {
        project_id: ProjectId,
        job_id: BuildJobId,
    },
    JobFinished {
        project_id: ProjectId,
        job_id: BuildJobId,
        status: BuildJobStatus,
        error: Option<String>,
    },
}

impl DeliveryLogLevel {
    pub fn as_tracing_level(self) -> tracing::Level {
        match self {
            Self::Info => tracing::Level::INFO,
            Self::Warn => tracing::Level::WARN,
            Self::Error => tracing::Level::ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use ipc_channel::ipc;

    use super::*;

    #[test]
    fn frontend_to_delivery_messages_round_trip() {
        let (sender, receiver) =
            ipc::channel::<FrontendToDelivery>().expect("ipc channel should open");
        let project_id = ProjectId::new("ipc").expect("project id should parse");

        sender
            .send(FrontendToDelivery::RefreshQueues)
            .expect("message should send");

        assert!(matches!(
            receiver.recv().expect("message should receive"),
            FrontendToDelivery::RefreshQueues
        ));

        sender
            .send(FrontendToDelivery::Shutdown)
            .expect("shutdown should send");
        assert!(matches!(
            receiver.recv().expect("shutdown should receive"),
            FrontendToDelivery::Shutdown
        ));

        sender
            .send(FrontendToDelivery::Enqueue {
                project_id,
                domain_node_id: None,
                handoff: crate::plan::DesignHandoff {
                    design_run_id: uuid::Uuid::new_v4(),
                    prompt: "Ship it".to_string(),
                    architect_plan: serde_json::json!({"summary": "ok"}).to_string(),
                    knowledge_collections: vec!["p_ipc_workspace-code".to_string()],
                },
            })
            .expect("enqueue should send");
        assert!(matches!(
            receiver.recv().expect("enqueue should receive"),
            FrontendToDelivery::Enqueue { .. }
        ));
    }

    #[test]
    fn delivery_to_frontend_messages_round_trip() {
        let (sender, receiver) =
            ipc::channel::<DeliveryToFrontend>().expect("ipc channel should open");
        let project_id = ProjectId::new("ipc").expect("project id should parse");

        sender
            .send(DeliveryToFrontend::Log {
                project_id,
                level: DeliveryLogLevel::Info,
                message: "ready".to_string(),
            })
            .expect("log should send");

        assert!(matches!(
            receiver.recv().expect("log should receive"),
            DeliveryToFrontend::Log {
                level: DeliveryLogLevel::Info,
                ..
            }
        ));
    }
}
