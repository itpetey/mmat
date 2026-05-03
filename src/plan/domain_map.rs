use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a node in a domain tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DomainNodeId(Uuid);

impl DomainNodeId {
    /// Creates a new random domain node identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DomainNodeId {
    fn default() -> Self {
        Self::new()
    }
}

/// Visibility of a knowledge group within a domain tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeVisibility {
    /// Visible to all nodes in the tree.
    Public,
    /// Visible only to this node and its descendants.
    Private,
}

/// A single node in the domain decomposition tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainNode {
    pub id: DomainNodeId,
    pub name: String,
    pub description: String,
    pub depth: usize,
    pub parent: Option<DomainNodeId>,
    pub children: Vec<DomainNodeId>,
    pub dependencies: Vec<DomainNodeId>,
    pub knowledge_collections: Vec<String>,
    pub knowledge_visibility: KnowledgeVisibility,
    pub status: DomainNodeStatus,
}

/// Lifecycle status of a domain node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainNodeStatus {
    /// Discovery is in progress for this node.
    Discovering,
    /// Discovery complete; ready for knowledge planning.
    Ready,
    /// Knowledge groups planned and materialised.
    KnowledgeMaterialised,
    /// Solution branches generated and collected.
    SolutionsCollected,
    /// Solution chosen by user.
    SolutionChosen,
    /// Architect plan produced.
    ArchitectComplete,
    /// Build jobs created and in delivery.
    Delivering,
    /// Delivery complete.
    Complete,
    /// Replanning due to backflow.
    Replanning,
}

/// Configuration controlling domain tree construction and behaviour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainTreeConfig {
    /// Maximum depth of the domain tree (default: 3).
    pub max_depth: usize,
    /// Maximum cascade depth for backflow replanning (default: 3).
    pub max_cascade_depth: usize,
}

impl Default for DomainTreeConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_cascade_depth: 3,
        }
    }
}

/// A hierarchical decomposition of a project domain into sub-domains.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainTree {
    pub root: DomainNodeId,
    pub nodes: HashMap<DomainNodeId, DomainNode>,
    pub config: DomainTreeConfig,
}

impl DomainTree {
    /// Creates a new domain tree with a single root node.
    pub fn new(root_name: impl Into<String>, root_description: impl Into<String>) -> Self {
        let root_id = DomainNodeId::new();
        let root = DomainNode {
            id: root_id,
            name: root_name.into(),
            description: root_description.into(),
            depth: 0,
            parent: None,
            children: Vec::new(),
            dependencies: Vec::new(),
            knowledge_collections: Vec::new(),
            knowledge_visibility: KnowledgeVisibility::Public,
            status: DomainNodeStatus::Discovering,
        };
        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);
        Self {
            root: root_id,
            nodes,
            config: DomainTreeConfig::default(),
        }
    }

    /// Creates a new domain tree with the given configuration.
    pub fn with_config(
        root_name: impl Into<String>,
        root_description: impl Into<String>,
        config: DomainTreeConfig,
    ) -> Self {
        let mut tree = Self::new(root_name, root_description);
        tree.config = config;
        tree
    }

    /// Returns the root node.
    pub fn root(&self) -> &DomainNode {
        self.nodes.get(&self.root).expect("root should exist")
    }

    /// Returns a node by id, if it exists.
    pub fn get(&self, id: DomainNodeId) -> Option<&DomainNode> {
        self.nodes.get(&id)
    }

    /// Returns a mutable reference to a node by id.
    pub fn get_mut(&mut self, id: DomainNodeId) -> Option<&mut DomainNode> {
        self.nodes.get_mut(&id)
    }

    /// Adds a child node under the given parent.
    ///
    /// Returns `None` if the parent does not exist or if adding the child
    /// would exceed the configured maximum depth.
    pub fn add_child(
        &mut self,
        parent_id: DomainNodeId,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Option<DomainNodeId> {
        let parent = self.nodes.get(&parent_id)?;
        if parent.depth + 1 > self.config.max_depth {
            return None;
        }

        let child_id = DomainNodeId::new();
        let child = DomainNode {
            id: child_id,
            name: name.into(),
            description: description.into(),
            depth: parent.depth + 1,
            parent: Some(parent_id),
            children: Vec::new(),
            dependencies: Vec::new(),
            knowledge_collections: Vec::new(),
            knowledge_visibility: KnowledgeVisibility::Private,
            status: DomainNodeStatus::Discovering,
        };

        self.nodes.insert(child_id, child);
        self.nodes
            .get_mut(&parent_id)
            .expect("parent should exist")
            .children
            .push(child_id);

        Some(child_id)
    }

    /// Applies a discovery output to a node, updating its status and
    /// adding sub-domain children when appropriate.
    ///
    /// Returns the ids of any newly created child nodes.
    #[allow(dead_code)]
    pub(super) fn apply_discovery_output(
        &mut self,
        node_id: DomainNodeId,
        output: &super::discovery::DiscoveryOutput,
    ) -> Vec<DomainNodeId> {
        let mut new_children = Vec::new();
        if self.nodes.contains_key(&node_id) {
            for sub in &output.sub_domains {
                if let Some(child_id) =
                    self.add_child(node_id, sub.name.clone(), sub.description.clone())
                {
                    new_children.push(child_id);
                }
            }

            if let Some(node) = self.nodes.get_mut(&node_id)
                && output.ready_for_solution
                && new_children.is_empty()
            {
                node.status = DomainNodeStatus::Ready;
            }
        }
        new_children
    }

    /// Adds a dependency edge from `node_id` to `dependency_id`.
    ///
    /// Returns `false` if either node is missing, the edge points to itself, or
    /// the dependency has already been registered or would create a cycle.
    pub fn add_dependency(&mut self, node_id: DomainNodeId, dependency_id: DomainNodeId) -> bool {
        if node_id == dependency_id
            || !self.nodes.contains_key(&node_id)
            || !self.nodes.contains_key(&dependency_id)
            || self.depends_on(dependency_id, node_id)
        {
            return false;
        }

        let node = self.nodes.get_mut(&node_id).expect("node should exist");
        if node.dependencies.contains(&dependency_id) {
            return false;
        }
        node.dependencies.push(dependency_id);
        true
    }

    /// Returns the direct dependencies declared by a node.
    pub fn dependencies(&self, id: DomainNodeId) -> Option<&[DomainNodeId]> {
        self.nodes.get(&id).map(|node| node.dependencies.as_slice())
    }

    /// Returns whether `node_id` depends on `target_id`, directly or transitively.
    pub fn depends_on(&self, node_id: DomainNodeId, target_id: DomainNodeId) -> bool {
        let mut stack = vec![node_id];
        let mut visited = Vec::new();
        while let Some(id) = stack.pop() {
            if !visited.contains(&id) {
                visited.push(id);
                if let Some(node) = self.nodes.get(&id) {
                    for dependency in &node.dependencies {
                        if *dependency == target_id {
                            return true;
                        }
                        stack.push(*dependency);
                    }
                }
            }
        }
        false
    }

    /// Returns true if the given node is a leaf (has no children).
    pub fn is_leaf(&self, id: DomainNodeId) -> bool {
        self.nodes
            .get(&id)
            .is_some_and(|node| node.children.is_empty())
    }

    /// Returns all leaf nodes in the tree.
    pub fn leaves(&self) -> Vec<&DomainNode> {
        self.nodes
            .values()
            .filter(|node| node.children.is_empty())
            .collect()
    }

    /// Returns all nodes at the given depth.
    pub fn at_depth(&self, depth: usize) -> Vec<&DomainNode> {
        self.nodes
            .values()
            .filter(|node| node.depth == depth)
            .collect()
    }

    /// Removes a node and all its descendants.
    ///
    /// Returns `false` if the node does not exist or if it is the root.
    pub fn remove_subtree(&mut self, id: DomainNodeId) -> bool {
        if id == self.root {
            return false;
        }
        let Some(node) = self.nodes.remove(&id) else {
            return false;
        };
        if let Some(parent_id) = node.parent
            && let Some(parent) = self.nodes.get_mut(&parent_id)
        {
            parent.children.retain(|child_id| *child_id != id);
        }
        for child_id in node.children {
            self.remove_subtree(child_id);
        }
        true
    }

    /// Returns an iterator over nodes in breadth-first order.
    pub fn bfs(&self) -> Vec<&DomainNode> {
        let mut result = Vec::new();
        let mut queue = vec![self.root];
        while let Some(id) = queue.pop() {
            if let Some(node) = self.nodes.get(&id) {
                result.push(node);
                for child_id in &node.children {
                    queue.insert(0, *child_id);
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_tree_starts_with_root() {
        let tree = DomainTree::new("Platform", "A data platform");

        assert_eq!(tree.root().name, "Platform");
        assert_eq!(tree.root().depth, 0);
        assert!(tree.root().parent.is_none());
        assert!(tree.is_leaf(tree.root));
    }

    #[test]
    fn domain_tree_adds_children_up_to_max_depth() {
        let config = DomainTreeConfig {
            max_depth: 2,
            max_cascade_depth: 3,
        };
        let mut tree = DomainTree::with_config("Root", "Root desc", config);

        let child = tree.add_child(tree.root, "Ingestion", "Data ingestion");
        assert!(child.is_some());

        let grandchild = tree.add_child(child.unwrap(), "Parser", "Message parser");
        assert!(grandchild.is_some());

        let great_grandchild = tree.add_child(grandchild.unwrap(), "Lexer", "Token lexer");
        assert!(great_grandchild.is_none());
    }

    #[test]
    fn domain_tree_detects_leaves() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let child = tree.add_child(tree.root, "Child", "Child desc").unwrap();

        assert!(!tree.is_leaf(tree.root));
        assert!(tree.is_leaf(child));
        assert_eq!(tree.leaves().len(), 1);
    }

    #[test]
    fn domain_tree_removes_subtree() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let child = tree.add_child(tree.root, "Child", "Child desc").unwrap();
        let grandchild = tree
            .add_child(child, "Grandchild", "Grandchild desc")
            .unwrap();

        assert!(tree.remove_subtree(child));
        assert!(tree.get(child).is_none());
        assert!(tree.get(grandchild).is_none());
        assert!(tree.root().children.is_empty());
    }

    #[test]
    fn domain_tree_cannot_remove_root() {
        let mut tree = DomainTree::new("Root", "Root desc");

        assert!(!tree.remove_subtree(tree.root));
    }

    #[test]
    fn domain_tree_bfs_order() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let a = tree.add_child(tree.root, "A", "").unwrap();
        let _b = tree.add_child(tree.root, "B", "").unwrap();
        tree.add_child(a, "A1", "").unwrap();

        let names: Vec<_> = tree.bfs().into_iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Root", "A", "B", "A1"]);
    }

    #[test]
    fn domain_tree_filters_by_depth() {
        let mut tree = DomainTree::new("Root", "Root desc");
        tree.add_child(tree.root, "D1", "").unwrap();
        tree.add_child(tree.root, "D2", "").unwrap();

        assert_eq!(tree.at_depth(0).len(), 1);
        assert_eq!(tree.at_depth(1).len(), 2);
        assert!(tree.at_depth(2).is_empty());
    }

    #[test]
    fn domain_node_defaults_to_public_visibility() {
        let tree = DomainTree::new("Root", "Root desc");
        assert_eq!(
            tree.root().knowledge_visibility,
            KnowledgeVisibility::Public
        );
    }

    #[test]
    fn child_defaults_to_private_visibility() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let child = tree.add_child(tree.root, "Child", "").unwrap();
        assert_eq!(
            tree.get(child).unwrap().knowledge_visibility,
            KnowledgeVisibility::Private
        );
    }

    #[test]
    fn apply_discovery_output_updates_status_and_adds_children() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let output = crate::plan::discovery::DiscoveryOutput {
            assistant_message: String::new(),
            ready_for_solution: true,
            problem_statement: "Build a platform".to_string(),
            goals: Vec::new(),
            constraints: Vec::new(),
            assumptions: Vec::new(),
            risks: Vec::new(),
            notes: Vec::new(),
            recommended_path: String::new(),
            open_questions: Vec::new(),
            sub_domains: vec![
                crate::plan::discovery::SubDomainSuggestion {
                    name: "Ingestion".to_string(),
                    description: "Data ingestion layer".to_string(),
                },
                crate::plan::discovery::SubDomainSuggestion {
                    name: "Storage".to_string(),
                    description: "Data storage layer".to_string(),
                },
            ],
        };

        let children = tree.apply_discovery_output(tree.root, &output);

        assert_eq!(tree.root().status, DomainNodeStatus::Discovering);
        assert_eq!(children.len(), 2);
        assert_eq!(tree.leaves().len(), 2);
        assert_eq!(tree.get(children[0]).unwrap().name, "Ingestion");
        assert_eq!(tree.get(children[1]).unwrap().name, "Storage");
    }

    #[test]
    fn apply_discovery_output_marks_leaf_ready_only_without_children() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let output = crate::plan::discovery::DiscoveryOutput {
            assistant_message: String::new(),
            ready_for_solution: true,
            problem_statement: String::new(),
            goals: Vec::new(),
            constraints: Vec::new(),
            assumptions: Vec::new(),
            risks: Vec::new(),
            notes: Vec::new(),
            recommended_path: String::new(),
            open_questions: Vec::new(),
            sub_domains: Vec::new(),
        };

        let children = tree.apply_discovery_output(tree.root, &output);

        assert!(children.is_empty());
        assert_eq!(tree.root().status, DomainNodeStatus::Ready);
    }

    #[test]
    fn domain_nodes_declare_dependencies() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let storage = tree.add_child(tree.root, "Storage", "Stores data").unwrap();
        let api = tree.add_child(tree.root, "API", "Serves data").unwrap();

        assert!(tree.add_dependency(api, storage));
        assert_eq!(tree.dependencies(api).unwrap(), &[storage]);
        assert!(tree.depends_on(api, storage));
        assert!(!tree.add_dependency(api, storage));
        assert!(!tree.add_dependency(api, api));
    }

    #[test]
    fn domain_dependencies_reject_cycles() {
        let mut tree = DomainTree::new("Root", "Root desc");
        let storage = tree.add_child(tree.root, "Storage", "Stores data").unwrap();
        let api = tree.add_child(tree.root, "API", "Serves data").unwrap();
        let ui = tree.add_child(tree.root, "UI", "Displays data").unwrap();

        assert!(tree.add_dependency(api, storage));
        assert!(tree.add_dependency(ui, api));
        assert!(!tree.add_dependency(storage, ui));
    }

    #[test]
    fn apply_discovery_output_respects_max_depth() {
        let config = DomainTreeConfig {
            max_depth: 0,
            max_cascade_depth: 3,
        };
        let mut tree = DomainTree::with_config("Root", "Root desc", config);
        let output = crate::plan::discovery::DiscoveryOutput {
            assistant_message: String::new(),
            ready_for_solution: false,
            problem_statement: String::new(),
            goals: Vec::new(),
            constraints: Vec::new(),
            assumptions: Vec::new(),
            risks: Vec::new(),
            notes: Vec::new(),
            recommended_path: String::new(),
            open_questions: Vec::new(),
            sub_domains: vec![crate::plan::discovery::SubDomainSuggestion {
                name: "Child".to_string(),
                description: "".to_string(),
            }],
        };

        let children = tree.apply_discovery_output(tree.root, &output);

        assert!(children.is_empty());
    }
}
