//! Architectural backflow types and cascade logic.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::plan::domain_map::{DomainNodeId, DomainNodeStatus, DomainTree};

/// Severity of a backflow event, determining how far back the pipeline must
/// retreat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BackflowSeverity {
    /// Minor issue; retry the same delivery phase.
    Minor,
    /// Moderate issue; retreat to the architect phase.
    Moderate,
    /// Major issue; retreat to solution selection phase.
    Major,
    /// Critical issue; retreat to discovery phase and cascade to dependents.
    Critical,
}

/// Pipeline phase target selected for a backflow event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackflowRouteTarget {
    Delivery,
    Architect,
    SolutionSelection,
    DomainMapping,
}

impl BackflowSeverity {
    /// Returns the human-readable label for this severity.
    pub fn label(self) -> &'static str {
        match self {
            Self::Minor => "Minor",
            Self::Moderate => "Moderate",
            Self::Major => "Major",
            Self::Critical => "Critical",
        }
    }

    /// Returns the phase that should handle this severity.
    pub fn route_target(self) -> BackflowRouteTarget {
        match self {
            Self::Minor => BackflowRouteTarget::Delivery,
            Self::Moderate => BackflowRouteTarget::Architect,
            Self::Major => BackflowRouteTarget::SolutionSelection,
            Self::Critical => BackflowRouteTarget::DomainMapping,
        }
    }
}

/// An event signalling that a sub-domain's delivery has failed and the
/// pipeline must backflow to an earlier phase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackflowEvent {
    pub source_node_id: DomainNodeId,
    pub severity: BackflowSeverity,
    pub reason: String,
    pub cascade_depth: usize,
}

impl BackflowEvent {
    pub fn new(
        source_node_id: DomainNodeId,
        severity: BackflowSeverity,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            source_node_id,
            severity,
            reason: reason.into(),
            cascade_depth: 0,
        }
    }

    pub fn with_cascade_depth(mut self, depth: usize) -> Self {
        self.cascade_depth = depth;
        self
    }

    pub fn route_target(&self) -> BackflowRouteTarget {
        self.severity.route_target()
    }
}

/// Tracks which nodes are affected by backflow and at what cascade depth.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackflowCascade {
    pub affected_nodes: HashMap<DomainNodeId, BackflowEvent>,
}

/// Result of applying a cascade to a domain tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackflowApplication {
    pub replanning_nodes: Vec<DomainNodeId>,
    pub invalidated_delivery_nodes: Vec<DomainNodeId>,
    pub human_review_required: bool,
}

impl BackflowCascade {
    pub fn new() -> Self {
        Self {
            affected_nodes: HashMap::new(),
        }
    }

    /// Computes the cascade of a critical backflow event through a domain tree.
    ///
    /// Starting from `source_node_id`, marks all dependent nodes as affected.
    /// Stops when `max_depth` is reached.
    pub fn compute(tree: &DomainTree, source_event: &BackflowEvent, max_depth: usize) -> Self {
        let mut cascade = Self::new();
        if source_event.severity != BackflowSeverity::Critical {
            cascade
                .affected_nodes
                .insert(source_event.source_node_id, source_event.clone());
            return cascade;
        }

        let mut queue = vec![(source_event.source_node_id, 0usize)];
        let mut visited = HashSet::new();

        while let Some((node_id, depth)) = queue.pop() {
            if !visited.insert(node_id) {
                continue;
            }

            let event = BackflowEvent {
                source_node_id: source_event.source_node_id,
                severity: source_event.severity,
                reason: source_event.reason.clone(),
                cascade_depth: depth,
            };
            cascade.affected_nodes.insert(node_id, event);

            if depth >= max_depth {
                continue;
            }

            // Find all nodes that depend on this node.
            for node in tree.nodes.values() {
                if node.dependencies.contains(&node_id) {
                    queue.push((node.id, depth + 1));
                }
            }
        }

        cascade
    }

    /// Computes the cascade using the domain tree's configured cascade depth.
    pub fn compute_for_tree(tree: &DomainTree, source_event: &BackflowEvent) -> Self {
        Self::compute(tree, source_event, tree.config.max_cascade_depth)
    }

    /// Returns true if the given node is affected by this cascade.
    pub fn is_affected(&self, node_id: DomainNodeId) -> bool {
        self.affected_nodes.contains_key(&node_id)
    }

    /// Returns true if the cascade exceeded the configured maximum depth,
    /// indicating that human review is required.
    pub fn halted(&self, max_depth: usize) -> bool {
        self.affected_nodes
            .values()
            .any(|event| event.cascade_depth >= max_depth)
    }

    pub fn requires_human_review(&self, max_depth: usize) -> bool {
        self.halted(max_depth)
    }

    /// Marks affected domain nodes for replanning and reports delivery nodes
    /// whose completed/running delivery state was invalidated by the cascade.
    pub fn apply_to_domain_tree(&self, tree: &mut DomainTree) -> BackflowApplication {
        let mut application = BackflowApplication {
            human_review_required: self.requires_human_review(tree.config.max_cascade_depth),
            ..BackflowApplication::default()
        };

        let mut affected: Vec<_> = self.affected_nodes.keys().copied().collect();
        affected.sort();
        for node_id in affected {
            let Some(node) = tree.get_mut(node_id) else {
                continue;
            };
            if matches!(
                node.status,
                DomainNodeStatus::Delivering | DomainNodeStatus::Complete
            ) {
                application.invalidated_delivery_nodes.push(node_id);
            }
            node.status = DomainNodeStatus::Replanning;
            application.replanning_nodes.push(node_id);
        }

        application
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_critical_backflow_affects_only_source() {
        let tree = DomainTree::new("Root", "Root desc");
        let event = BackflowEvent::new(tree.root, BackflowSeverity::Moderate, " architect issue");

        let cascade = BackflowCascade::compute(&tree, &event, 3);

        assert!(cascade.is_affected(tree.root));
        assert_eq!(cascade.affected_nodes.len(), 1);
    }

    #[test]
    fn severity_maps_to_backflow_route_target() {
        assert_eq!(
            BackflowSeverity::Minor.route_target(),
            BackflowRouteTarget::Delivery
        );
        assert_eq!(
            BackflowSeverity::Moderate.route_target(),
            BackflowRouteTarget::Architect
        );
        assert_eq!(
            BackflowSeverity::Major.route_target(),
            BackflowRouteTarget::SolutionSelection
        );
        assert_eq!(
            BackflowSeverity::Critical.route_target(),
            BackflowRouteTarget::DomainMapping
        );
    }

    #[test]
    fn critical_backflow_cascades_to_dependents() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        tree.add_dependency(b, a);
        tree.add_dependency(c, b);

        let event = BackflowEvent::new(a, BackflowSeverity::Critical, "api broken");
        let cascade = BackflowCascade::compute(&tree, &event, 3);

        assert!(cascade.is_affected(a));
        assert!(cascade.is_affected(b));
        assert!(cascade.is_affected(c));
        assert_eq!(cascade.affected_nodes[&a].cascade_depth, 0);
        assert_eq!(cascade.affected_nodes[&b].cascade_depth, 1);
        assert_eq!(cascade.affected_nodes[&c].cascade_depth, 2);
    }

    #[test]
    fn critical_backflow_marks_dependents_for_replanning() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        tree.add_dependency(b, a);
        tree.get_mut(a).unwrap().status = DomainNodeStatus::Complete;
        tree.get_mut(b).unwrap().status = DomainNodeStatus::Delivering;

        let event = BackflowEvent::new(a, BackflowSeverity::Critical, "api broken");
        let cascade = BackflowCascade::compute_for_tree(&tree, &event);
        let application = cascade.apply_to_domain_tree(&mut tree);

        assert_eq!(tree.get(a).unwrap().status, DomainNodeStatus::Replanning);
        assert_eq!(tree.get(b).unwrap().status, DomainNodeStatus::Replanning);
        assert_eq!(application.replanning_nodes, vec![a, b]);
        assert_eq!(application.invalidated_delivery_nodes, vec![a, b]);
    }

    #[test]
    fn cascade_halts_at_max_depth() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        tree.add_dependency(b, a);
        tree.add_dependency(c, b);

        let event = BackflowEvent::new(a, BackflowSeverity::Critical, "api broken");
        let cascade = BackflowCascade::compute(&tree, &event, 1);

        assert!(cascade.is_affected(a));
        assert!(cascade.is_affected(b));
        assert!(!cascade.is_affected(c));
        assert!(cascade.halted(1));
        assert!(cascade.requires_human_review(1));
    }

    #[test]
    fn cascade_uses_domain_tree_configured_depth() {
        let mut tree = DomainTree::with_config(
            "Root",
            "Root desc",
            crate::plan::domain_map::DomainTreeConfig {
                max_depth: 3,
                max_cascade_depth: 1,
            },
        );
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        tree.add_dependency(b, a);
        tree.add_dependency(c, b);

        let event = BackflowEvent::new(a, BackflowSeverity::Critical, "api broken");
        let cascade = BackflowCascade::compute_for_tree(&tree, &event);

        assert!(cascade.is_affected(a));
        assert!(cascade.is_affected(b));
        assert!(!cascade.is_affected(c));
        assert!(cascade.requires_human_review(tree.config.max_cascade_depth));

        let application = cascade.apply_to_domain_tree(&mut tree);
        assert!(application.human_review_required);
    }
}
