//! Internal tree representation for AST diffing.
//!
//! Trees are stored in an arena (`Vec<Node>`) with `NodeId` indices into it.
//! Each node carries cached `height`, `size`, and a structural `hash` that allow
//! the matching algorithms to compare subtrees in O(1).
//!
//! Construction goes through [`TreeBuilder`], which lets you build a tree bottom-up
//! while passing parent IDs. Once built, the tree is immutable.

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

/// Index of a node within a [`Tree`]'s arena.
pub type NodeId = usize;

/// A single node in an AST.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    /// The node "type" (e.g. `block_mapping_pair`, `if_expression`).
    pub kind: String,
    /// Text content for leaves; empty string for internal nodes by convention.
    pub label: String,
    /// Byte offset of the node's start in the source file.
    pub start_byte: usize,
    /// Byte offset just past the node's end.
    pub end_byte: usize,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// 1 for a leaf; max(child.height) + 1 otherwise.
    pub height: usize,
    /// Number of nodes in the subtree rooted here (including this node).
    pub size: usize,
    /// Structural hash combining kind, label, and ordered child hashes.
    pub hash: u64,
}

/// An immutable arena-backed tree.
#[derive(Debug, Clone)]
pub struct Tree {
    nodes: Vec<Node>,
    root: NodeId,
}

impl Tree {
    /// Returns the root node's id.
    #[must_use]
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Returns a reference to the node with the given id.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// Returns the total number of nodes in the tree.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns an iterator over every node, in id order.
    pub fn all_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }

    /// Pre-order traversal starting at `start` (root before children).
    #[must_use]
    pub fn pre_order(&self, start: NodeId) -> Vec<NodeId> {
        let mut result = Vec::with_capacity(self.nodes[start].size);
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            result.push(id);
            // Push children in reverse so the leftmost child is visited first.
            for &child_id in self.nodes[id].children.iter().rev() {
                stack.push(child_id);
            }
        }
        result
    }

    /// Post-order traversal starting at `start` (children before root).
    #[must_use]
    pub fn post_order(&self, start: NodeId) -> Vec<NodeId> {
        // Two-pass: reverse pre-order (right-to-left), then reverse the result.
        let mut result = Vec::with_capacity(self.nodes[start].size);
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            result.push(id);
            for &child_id in &self.nodes[id].children {
                stack.push(child_id);
            }
        }
        result.reverse();
        result
    }

    /// Breadth-first traversal starting at `start`.
    #[must_use]
    pub fn bfs_order(&self, start: NodeId) -> Vec<NodeId> {
        let mut result = Vec::with_capacity(self.nodes[start].size);
        let mut queue = VecDeque::new();
        queue.push_back(start);
        while let Some(id) = queue.pop_front() {
            result.push(id);
            for child_id in &self.nodes[id].children {
                queue.push_back(*child_id);
            }
        }
        result
    }

    /// All proper descendants of `start` (excluding `start` itself).
    #[must_use]
    pub fn descendants(&self, start: NodeId) -> Vec<NodeId> {
        let mut result = Vec::with_capacity(self.nodes[start].size.saturating_sub(1));
        let mut stack: Vec<NodeId> = self.nodes[start].children.iter().copied().rev().collect();
        while let Some(id) = stack.pop() {
            result.push(id);
            for &child_id in self.nodes[id].children.iter().rev() {
                stack.push(child_id);
            }
        }
        result
    }

    /// Boolean membership array for all proper descendants of `start`.
    ///
    /// Returns a `Vec<bool>` of length `node_count()` where index `i` is `true`
    /// iff node `i` is a proper descendant of `start`. Faster than collecting
    /// into a `HashSet<NodeId>` when only membership testing is needed, because
    /// `NodeId`s are dense arena indices.
    #[must_use]
    pub fn descendant_set(&self, start: NodeId) -> Vec<bool> {
        let mut member = vec![false; self.nodes.len()];
        let mut stack: Vec<NodeId> = self.nodes[start].children.clone();
        while let Some(id) = stack.pop() {
            member[id] = true;
            stack.extend(self.nodes[id].children.iter().copied());
        }
        member
    }

    /// Returns the position of `child` within its parent's children list,
    /// or `None` if `child` is the root or has no parent.
    #[must_use]
    pub fn position_in_parent(&self, child: NodeId) -> Option<usize> {
        let parent = self.nodes[child].parent?;
        self.nodes[parent]
            .children
            .iter()
            .position(|&candidate| candidate == child)
    }

    fn recompute_metadata(&mut self) {
        // Post-order ensures children are processed before parents.
        let order = self.post_order(self.root);
        for id in order {
            let children: Vec<NodeId> = self.nodes[id].children.clone();

            if children.is_empty() {
                self.nodes[id].height = 1;
                self.nodes[id].size = 1;
            } else {
                let mut max_height = 0;
                let mut total_size = 1usize;
                for &child_id in &children {
                    if self.nodes[child_id].height > max_height {
                        max_height = self.nodes[child_id].height;
                    }
                    total_size += self.nodes[child_id].size;
                }
                self.nodes[id].height = max_height + 1;
                self.nodes[id].size = total_size;
            }

            // Structural hash: combines kind, label, and ordered child hashes.
            let mut hasher = DefaultHasher::new();
            self.nodes[id].kind.hash(&mut hasher);
            self.nodes[id].label.hash(&mut hasher);
            for &child_id in &children {
                self.nodes[child_id].hash.hash(&mut hasher);
            }
            self.nodes[id].hash = hasher.finish();
        }
    }
}

/// Mutable builder used to construct a [`Tree`] node by node.
#[derive(Debug, Default)]
pub struct TreeBuilder {
    nodes: Vec<Node>,
}

impl TreeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node and returns its assigned id.
    ///
    /// `parent` must be a previously-added node (or `None` for the root).
    pub fn add(
        &mut self,
        kind: &str,
        label: &str,
        parent: Option<NodeId>,
        start_byte: usize,
        end_byte: usize,
    ) -> NodeId {
        let id = self.nodes.len();
        let node = Node {
            id,
            kind: kind.to_string(),
            label: label.to_string(),
            start_byte,
            end_byte,
            parent,
            children: Vec::new(),
            height: 0,
            size: 0,
            hash: 0,
        };
        if let Some(parent_id) = parent {
            self.nodes[parent_id].children.push(id);
        }
        self.nodes.push(node);
        id
    }

    /// Finalises the tree. `root` must be a valid id, normally the first node added.
    #[must_use]
    pub fn build(self, root: NodeId) -> Tree {
        assert!(root < self.nodes.len(), "root id out of bounds");
        let mut tree = Tree {
            nodes: self.nodes,
            root,
        };
        tree.recompute_metadata();
        tree
    }
}
