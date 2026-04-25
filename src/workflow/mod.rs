use naaf_core::{EdgeSpec, GraphPatch, NodeId, NodeInput, NodeSpec, StepNode, Workflow};
use naaf_llm::{ChannelHumanIO, LlmAgent, OpenAiClient, OpenAiConfig};
use serde::{Deserialize, Serialize};

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

pub async fn greenfield(init_prompt: String) {
    let (_runtime, _pending_questions) = ChannelHumanIO::new(1024 * 512); // 512k question buffer
    let cfg = OpenAiConfig::new("blah");
    let oai_client = OpenAiClient::<ChannelHumanIO>::new(cfg);
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

    let _workflow = Workflow::new()
        .with_patch(
            GraphPatch::new()
                .with_node(discovery_node)
                .with_node(knowledge_node)
                .with_edge(EdgeSpec::new(discovery_id, knowledge_id)),
        )
        .expect("workflow graph patch should be valid");
}
