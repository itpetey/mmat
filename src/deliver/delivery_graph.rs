//! Graph-based delivery scheduling with topological batching.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::plan::domain_map::{DomainNodeId, DomainTree};

/// Errors that can occur when constructing or validating a delivery graph.
#[derive(Debug, Error)]
pub enum DeliveryGraphError {
    #[error("dependency cycle detected in delivery graph")]
    CycleDetected,
    #[error("domain node not found: {0:?}")]
    NodeNotFound(DomainNodeId),
    #[error("node already exists in delivery graph: {0:?}")]
    DuplicateNode(DomainNodeId),
}

/// Status of an individual delivery job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryJobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

/// Progress for one job within a delivery batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryNodeProgress {
    pub domain_node_id: DomainNodeId,
    pub status: DeliveryJobStatus,
    pub error: Option<String>,
}

/// Progress for a single topological delivery batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryBatchProgress {
    pub index: usize,
    pub active: bool,
    pub completed: bool,
    pub nodes: Vec<DeliveryNodeProgress>,
}

/// Summary of delivery progress across all batches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryProgress {
    pub total_batches: usize,
    pub active_batch_index: Option<usize>,
    pub completed_batches: usize,
    pub total_nodes: usize,
    pub succeeded_nodes: usize,
    pub failed_nodes: usize,
    pub pending_nodes: usize,
    pub running_nodes: usize,
    pub batches: Vec<DeliveryBatchProgress>,
}

/// A node in the delivery graph representing a single sub-domain build job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryNode {
    pub domain_node_id: DomainNodeId,
    pub dependencies: Vec<DomainNodeId>,
    pub status: DeliveryJobStatus,
    pub error: Option<String>,
}

/// A batch of delivery jobs that may execute in parallel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryBatch {
    pub index: usize,
    pub nodes: Vec<DomainNodeId>,
}

/// A delivery graph maps sub-domain nodes to build jobs and schedules them
/// into dependency-ordered batches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryGraph {
    pub nodes: HashMap<DomainNodeId, DeliveryNode>,
    pub batches: Vec<DeliveryBatch>,
    pub active_batch_index: Option<usize>,
}

impl DeliveryJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl DeliveryNode {
    pub fn new(domain_node_id: DomainNodeId) -> Self {
        Self {
            domain_node_id,
            dependencies: Vec::new(),
            status: DeliveryJobStatus::Pending,
            error: None,
        }
    }
}

impl DeliveryGraph {
    /// Creates an empty delivery graph.
    pub fn empty() -> Self {
        Self {
            nodes: HashMap::new(),
            batches: Vec::new(),
            active_batch_index: None,
        }
    }

    /// Builds a delivery graph from a domain tree, creating one delivery node
    /// per leaf node and sorting them into dependency-ordered batches.
    ///
    /// Returns an error if the dependency graph contains a cycle.
    pub fn from_domain_tree(tree: &DomainTree) -> Result<Self, DeliveryGraphError> {
        let mut graph = Self::empty();
        let leaves: Vec<_> = tree.leaves().iter().map(|node| node.id).collect();

        for leaf_id in &leaves {
            graph
                .add_node(*leaf_id)
                .map_err(|_| DeliveryGraphError::DuplicateNode(*leaf_id))?;
        }

        // Add dependency edges from the domain tree.
        for leaf_id in &leaves {
            if let Some(deps) = tree.dependencies(*leaf_id) {
                for dep_id in deps {
                    if leaves.contains(dep_id) {
                        graph.add_dependency(*leaf_id, *dep_id)?;
                    }
                }
            }
        }

        graph.compute_batches()?;
        Ok(graph)
    }

    /// Adds a node to the delivery graph.
    pub fn add_node(&mut self, domain_node_id: DomainNodeId) -> Result<(), DeliveryGraphError> {
        if self.nodes.contains_key(&domain_node_id) {
            return Err(DeliveryGraphError::DuplicateNode(domain_node_id));
        }
        self.nodes
            .insert(domain_node_id, DeliveryNode::new(domain_node_id));
        Ok(())
    }

    /// Adds a dependency edge from `node_id` to `dependency_id`.
    ///
    /// Returns an error if either node is missing or if the edge would create
    /// a cycle.
    pub fn add_dependency(
        &mut self,
        node_id: DomainNodeId,
        dependency_id: DomainNodeId,
    ) -> Result<(), DeliveryGraphError> {
        if !self.nodes.contains_key(&node_id) {
            return Err(DeliveryGraphError::NodeNotFound(node_id));
        }
        if !self.nodes.contains_key(&dependency_id) {
            return Err(DeliveryGraphError::NodeNotFound(dependency_id));
        }
        if node_id == dependency_id {
            return Err(DeliveryGraphError::CycleDetected);
        }
        if self.would_create_cycle(node_id, dependency_id) {
            return Err(DeliveryGraphError::CycleDetected);
        }

        let node = self.nodes.get_mut(&node_id).expect("node should exist");
        if !node.dependencies.contains(&dependency_id) {
            node.dependencies.push(dependency_id);
        }
        Ok(())
    }

    /// Returns whether adding an edge from `from` to `to` would create a cycle.
    fn would_create_cycle(&self, from: DomainNodeId, to: DomainNodeId) -> bool {
        // If `to` can reach `from`, adding from -> to creates a cycle.
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([to]);
        while let Some(current) = queue.pop_front() {
            if current == from {
                return true;
            }
            if visited.insert(current)
                && let Some(node) = self.nodes.get(&current)
            {
                for dep in &node.dependencies {
                    queue.push_back(*dep);
                }
            }
        }
        false
    }

    /// Computes dependency-ordered batches using Kahn's algorithm.
    ///
    /// All nodes whose dependencies are satisfied in the same iteration are
    /// placed in the same batch.
    fn compute_batches(&mut self) -> Result<(), DeliveryGraphError> {
        let mut in_degree: HashMap<DomainNodeId, usize> = HashMap::new();
        let mut dependents: HashMap<DomainNodeId, Vec<DomainNodeId>> = HashMap::new();

        for (id, node) in &self.nodes {
            in_degree.entry(*id).or_insert(0);
            for dep in &node.dependencies {
                in_degree.entry(*dep).or_insert(0);
                dependents.entry(*dep).or_default().push(*id);
            }
        }

        // Count in-degrees: for each node, increment its own in-degree for each dependency.
        for (id, node) in &self.nodes {
            for _dep in &node.dependencies {
                *in_degree.get_mut(id).expect("node should exist") += 1;
            }
        }

        let mut queue: VecDeque<DomainNodeId> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(id, _)| *id)
            .collect();

        let mut batches: Vec<DeliveryBatch> = Vec::new();
        let mut processed = 0;

        while !queue.is_empty() {
            let batch_nodes: Vec<DomainNodeId> = queue.drain(..).collect();
            processed += batch_nodes.len();

            for node_id in &batch_nodes {
                if let Some(node_dependents) = dependents.get(node_id) {
                    for dependent in node_dependents {
                        let degree = in_degree
                            .get_mut(dependent)
                            .expect("dependent should exist");
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(*dependent);
                        }
                    }
                }
            }

            batches.push(DeliveryBatch {
                index: batches.len(),
                nodes: batch_nodes,
            });
        }

        if processed != self.nodes.len() {
            return Err(DeliveryGraphError::CycleDetected);
        }

        self.batches = batches;
        Ok(())
    }

    /// Updates the status of a delivery node.
    pub fn set_status(&mut self, node_id: DomainNodeId, status: DeliveryJobStatus) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.status = status;
        }
    }

    /// Sets an error on a delivery node.
    pub fn set_error(&mut self, node_id: DomainNodeId, error: impl Into<String>) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.error = Some(error.into());
            node.status = DeliveryJobStatus::Failed;
        }
    }

    /// Returns the current batch if any batch is active.
    pub fn active_batch(&self) -> Option<&DeliveryBatch> {
        self.active_batch_index
            .and_then(|index| self.batches.get(index))
    }

    /// Advances to the next batch.
    pub fn advance_batch(&mut self) {
        self.active_batch_index = match self.active_batch_index {
            Some(index) if index + 1 < self.batches.len() => Some(index + 1),
            None if !self.batches.is_empty() => Some(0),
            _ => self.active_batch_index,
        };
    }

    /// Returns a progress summary for the delivery graph.
    pub fn progress(&self) -> DeliveryProgress {
        let mut succeeded = 0;
        let mut failed = 0;
        let mut pending = 0;
        let mut running = 0;

        for node in self.nodes.values() {
            match node.status {
                DeliveryJobStatus::Succeeded => succeeded += 1,
                DeliveryJobStatus::Failed => failed += 1,
                DeliveryJobStatus::Running => running += 1,
                DeliveryJobStatus::Pending => pending += 1,
            }
        }

        let completed_batches = self
            .batches
            .iter()
            .filter(|batch| {
                batch.nodes.iter().all(|node_id| {
                    self.nodes.get(node_id).is_some_and(|n| {
                        matches!(
                            n.status,
                            DeliveryJobStatus::Succeeded | DeliveryJobStatus::Failed
                        )
                    })
                })
            })
            .count();
        let batches = self
            .batches
            .iter()
            .map(|batch| {
                let nodes: Vec<_> = batch
                    .nodes
                    .iter()
                    .filter_map(|node_id| {
                        self.nodes.get(node_id).map(|node| DeliveryNodeProgress {
                            domain_node_id: *node_id,
                            status: node.status,
                            error: node.error.clone(),
                        })
                    })
                    .collect();
                let completed = nodes.iter().all(|node| {
                    matches!(
                        node.status,
                        DeliveryJobStatus::Succeeded | DeliveryJobStatus::Failed
                    )
                });
                DeliveryBatchProgress {
                    index: batch.index,
                    active: self.active_batch_index == Some(batch.index),
                    completed,
                    nodes,
                }
            })
            .collect();

        DeliveryProgress {
            total_batches: self.batches.len(),
            active_batch_index: self.active_batch_index,
            completed_batches,
            total_nodes: self.nodes.len(),
            succeeded_nodes: succeeded,
            failed_nodes: failed,
            pending_nodes: pending,
            running_nodes: running,
            batches,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_has_no_batches() {
        let graph = DeliveryGraph::empty();
        assert!(graph.batches.is_empty());
        assert!(graph.active_batch().is_none());
    }

    #[test]
    fn single_node_forms_one_batch() {
        let tree = DomainTree::new("Root", "Root desc");
        let graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert_eq!(graph.batches.len(), 1);
        assert_eq!(graph.batches[0].nodes.len(), 1);
        assert_eq!(graph.batches[0].nodes[0], tree.root);
    }

    #[test]
    fn independent_nodes_in_same_batch() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let _a = tree.add_child(tree.root, "A", "").unwrap();
        let _b = tree.add_child(tree.root, "B", "").unwrap();

        let graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        // Root is not a leaf, so only A and B are in the graph.
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.batches.len(), 1);
        assert_eq!(graph.batches[0].nodes.len(), 2);
    }

    #[test]
    fn dependent_nodes_in_separate_batches() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        assert!(tree.add_dependency(b, a));

        let graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.batches.len(), 2);
        assert_eq!(graph.batches[0].nodes, vec![a]);
        assert_eq!(graph.batches[1].nodes, vec![b]);
    }

    #[test]
    fn cycle_is_rejected() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();

        let mut graph = DeliveryGraph::empty();
        graph.add_node(a).expect("should add node");
        graph.add_node(b).expect("should add node");
        graph.add_dependency(b, a).expect("should add dependency");

        let result = graph.add_dependency(a, b);
        assert!(matches!(result, Err(DeliveryGraphError::CycleDetected)));
    }

    #[test]
    fn compute_batches_rejects_cycle() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();

        let mut graph = DeliveryGraph::empty();
        graph.add_node(a).expect("should add node");
        graph.add_node(b).expect("should add node");
        // Manually create a cycle bypassing add_dependency validation.
        graph.nodes.get_mut(&a).unwrap().dependencies.push(b);
        graph.nodes.get_mut(&b).unwrap().dependencies.push(a);

        let result = graph.compute_batches();
        assert!(matches!(result, Err(DeliveryGraphError::CycleDetected)));
    }

    #[test]
    fn diamond_dependency_produces_two_batches() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        assert!(tree.add_dependency(b, a));
        assert!(tree.add_dependency(c, a));

        let graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.batches.len(), 2);
        assert_eq!(graph.batches[0].nodes, vec![a]);
        assert!(graph.batches[1].nodes.contains(&b));
        assert!(graph.batches[1].nodes.contains(&c));
    }

    #[test]
    fn advance_batch_moves_to_next() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        assert!(tree.add_dependency(b, a));

        let mut graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert!(graph.active_batch().is_none());
        graph.advance_batch();
        assert_eq!(graph.active_batch().unwrap().nodes, vec![a]);
        graph.advance_batch();
        assert_eq!(graph.active_batch().unwrap().nodes, vec![b]);
    }

    #[test]
    fn progress_counts_completed_batches_by_terminal_node_status() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        assert!(tree.add_dependency(b, a));

        let mut graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");
        graph.advance_batch();
        graph.set_status(a, DeliveryJobStatus::Succeeded);

        let progress = graph.progress();
        assert_eq!(progress.completed_batches, 1);

        graph.advance_batch();
        graph.set_status(b, DeliveryJobStatus::Failed);

        let progress = graph.progress();
        assert_eq!(progress.completed_batches, 2);
        assert_eq!(progress.failed_nodes, 1);
        assert_eq!(
            progress.batches[1].nodes[0].status,
            DeliveryJobStatus::Failed
        );
        assert!(progress.batches[1].completed);
    }

    #[test]
    fn multi_job_complex_dependency_produces_three_batches() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        let d = tree.add_child(tree.root, "D", "").unwrap();
        assert!(tree.add_dependency(b, a));
        assert!(tree.add_dependency(c, a));
        assert!(tree.add_dependency(d, b));
        assert!(tree.add_dependency(d, c));

        let graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert_eq!(graph.batches.len(), 3);
        assert_eq!(graph.batches[0].nodes, vec![a]);
        assert_eq!(graph.batches[1].nodes.len(), 2);
        assert!(graph.batches[1].nodes.contains(&b));
        assert!(graph.batches[1].nodes.contains(&c));
        assert_eq!(graph.batches[2].nodes, vec![d]);
    }

    #[test]
    fn parallel_batch_nodes_have_no_inter_dependencies() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        let d = tree.add_child(tree.root, "D", "").unwrap();
        assert!(tree.add_dependency(c, a));
        assert!(tree.add_dependency(d, b));

        let graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert_eq!(graph.batches.len(), 2);
        assert_eq!(graph.batches[0].nodes.len(), 2);
        assert!(graph.batches[0].nodes.contains(&a));
        assert!(graph.batches[0].nodes.contains(&b));
        assert_eq!(graph.batches[1].nodes.len(), 2);
        assert!(graph.batches[1].nodes.contains(&c));
        assert!(graph.batches[1].nodes.contains(&d));

        for batch in &graph.batches {
            for node_id in &batch.nodes {
                for other_id in &batch.nodes {
                    if node_id != other_id {
                        assert!(
                            !tree.depends_on(*node_id, *other_id),
                            "nodes in the same batch should not depend on each other"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn delivery_graph_tracks_active_batch_through_execution() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let b = tree.add_child(tree.root, "B", "").unwrap();
        let c = tree.add_child(tree.root, "C", "").unwrap();
        assert!(tree.add_dependency(b, a));
        assert!(tree.add_dependency(c, a));

        let mut graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");

        assert!(graph.active_batch().is_none());

        graph.advance_batch();
        assert_eq!(graph.active_batch().unwrap().nodes, vec![a]);
        graph.set_status(a, DeliveryJobStatus::Succeeded);

        graph.advance_batch();
        assert_eq!(graph.active_batch().unwrap().nodes.len(), 2);
        assert!(graph.active_batch().unwrap().nodes.contains(&b));
        assert!(graph.active_batch().unwrap().nodes.contains(&c));
        graph.set_status(b, DeliveryJobStatus::Succeeded);
        graph.set_status(c, DeliveryJobStatus::Succeeded);

        let progress = graph.progress();
        assert_eq!(progress.total_batches, 2);
        assert_eq!(progress.completed_batches, 2);
        assert_eq!(progress.succeeded_nodes, 3);
    }

    #[test]
    fn progress_reports_running_nodes_in_active_batch() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let _b = tree.add_child(tree.root, "B", "").unwrap();
        tree.add_child(tree.root, "C", "").unwrap();

        let mut graph = DeliveryGraph::from_domain_tree(&tree).expect("should build");
        graph.advance_batch();
        graph.set_status(a, DeliveryJobStatus::Running);

        let progress = graph.progress();
        assert_eq!(progress.running_nodes, 1);
        assert_eq!(progress.pending_nodes, 2);
        assert_eq!(progress.active_batch_index, Some(0));
    }
}
