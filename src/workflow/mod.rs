use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

use naaf_core::{
    EdgeSpec, GraphPatch, NodeId, NodeInput, NodeSpec, StepNode, Workflow, WorkflowRunReport,
};
use naaf_llm::{HumanIO, LlmAgent, OpenAiClient, OpenAiConfig, OpenAiStreamObserver};
use serde::{Deserialize, Serialize};

use crate::MmatError;

mod discovery;
mod knowledge;
mod parser;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(super) enum WorkflowStageId {
    Discovery,
    KnowledgePlanning,
    KnowledgeMaterialisation,
    Solutions,
    SolutionSelection,
    SoftwareArchitect,
    ImplementationPlanning,
    Execution,
}

impl WorkflowStageId {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::KnowledgePlanning => "knowledge-planning",
            Self::KnowledgeMaterialisation => "knowledge-materialisation",
            Self::Solutions => "solutions",
            Self::SolutionSelection => "solution-selection",
            Self::SoftwareArchitect => "software-architect",
            Self::ImplementationPlanning => "implementation-planning",
            Self::Execution => "execution",
        }
    }
}

impl std::fmt::Display for WorkflowStageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub async fn greenfield<R>(
    init_prompt: String,
    runtime: R,
    stream_observer: Option<Arc<dyn OpenAiStreamObserver<R>>>,
) -> Result<WorkflowRunReport, MmatError>
where
    R: HumanIO + 'static,
    R::Error: Debug + Display + 'static,
{
    let cfg = OpenAiConfig::new("").with_base_url("http://127.0.0.1:1234/v1");
    let mut oai_client = OpenAiClient::<R>::new(cfg);
    if let Some(stream_observer) = stream_observer {
        oai_client = oai_client.with_stream_observer(stream_observer);
    }
    let agent = LlmAgent::new(oai_client);

    let discovery_id = NodeId::new();
    let knowledge_id = NodeId::new();

    let discovery_node = NodeSpec::new(
        "discovery",
        StepNode::new(discovery::step(&agent), |node_input: &NodeInput| {
            node_input.seed_as::<discovery::DiscoveryInput>()
        }),
    )
    .with_id(discovery_id)
    .with_seed(discovery::DiscoveryInput::new(init_prompt))
    .expect("discovery input should serialise");

    let knowledge_node = NodeSpec::new(
        "knowledge_planning",
        StepNode::new(knowledge::step(&agent), move |node_input: &NodeInput| {
            let discovery = node_input.output_as::<discovery::DiscoveryOutput>(discovery_id)?;
            Ok(knowledge::KnowledgeInput::new(discovery))
        }),
    )
    .with_id(knowledge_id)
    .with_parent(discovery_id);

    let workflow = Workflow::new()
        .with_patch(
            GraphPatch::new()
                .with_node(discovery_node)
                .with_node(knowledge_node)
                .with_edge(EdgeSpec::new(discovery_id, knowledge_id)),
        )
        .map_err(|error| MmatError::Workflow(error.to_string()))?;

    workflow
        .run(&runtime)
        .await
        .map_err(|error| MmatError::Workflow(error.to_string()))
}
