#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunArtifact {
    RunSummary,
    DiscoveryBrief,
    ReconciledProposal,
    ApprovalOutcome,
    ImplementationPlan,
    ArchitectReview,
    FinalReview,
    WorkflowOutcome,
}

impl RunArtifact {
    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::RunSummary => "run-summary.json",
            Self::DiscoveryBrief => "discovery-brief.json",
            Self::ReconciledProposal => "reconciled-proposal.json",
            Self::ApprovalOutcome => "approval-outcome.json",
            Self::ImplementationPlan => "implementation-plan.json",
            Self::ArchitectReview => "architect-review.json",
            Self::FinalReview => "final-review.json",
            Self::WorkflowOutcome => "workflow-outcome.json",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RunArtifact;

    #[test]
    fn run_artifacts_map_to_stable_file_names() {
        assert_eq!(RunArtifact::RunSummary.file_name(), "run-summary.json");
        assert_eq!(
            RunArtifact::DiscoveryBrief.file_name(),
            "discovery-brief.json"
        );
        assert_eq!(
            RunArtifact::ReconciledProposal.file_name(),
            "reconciled-proposal.json"
        );
        assert_eq!(
            RunArtifact::ApprovalOutcome.file_name(),
            "approval-outcome.json"
        );
        assert_eq!(
            RunArtifact::ImplementationPlan.file_name(),
            "implementation-plan.json"
        );
        assert_eq!(
            RunArtifact::ArchitectReview.file_name(),
            "architect-review.json"
        );
        assert_eq!(RunArtifact::FinalReview.file_name(), "final-review.json");
        assert_eq!(
            RunArtifact::WorkflowOutcome.file_name(),
            "workflow-outcome.json"
        );
    }
}
