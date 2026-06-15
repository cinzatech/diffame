//! Top-down (anchor) matching phase.
//!
//! Walks both trees from highest subtrees downward, mapping isomorphic
//! subtrees that share a structural hash. Ambiguous candidates are deferred
//! and resolved at the end using parent-context dice similarity.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::mapping::Mapping;
use crate::tree::{NodeId, Tree};

/// Default minimum height for top-down anchor matching.
pub const DEFAULT_MIN_HEIGHT: usize = 2;

/// Runs the top-down phase, extending `mapping` in place.
///
/// Subtrees whose height is at most `min_height` are deferred to the
/// bottom-up phase.
pub fn match_top_down(
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &mut Mapping,
    min_height: usize,
) {
    let mut source_queue = HeightPQ::new();
    let mut destination_queue = HeightPQ::new();
    source_queue.push(source_tree, source_tree.root());
    destination_queue.push(destination_tree, destination_tree.root());

    let mut ambiguous: Vec<(NodeId, NodeId)> = Vec::new();

    while let Some(source_height) = source_queue.peek_height() {
        let Some(destination_height) = destination_queue.peek_height() else {
            break;
        };

        let max_height = source_height.max(destination_height);
        if max_height <= min_height {
            break;
        }

        if source_height > destination_height {
            for node_id in source_queue.pop_max() {
                source_queue.open(source_tree, node_id);
            }
        } else if destination_height > source_height {
            for node_id in destination_queue.pop_max() {
                destination_queue.open(destination_tree, node_id);
            }
        } else {
            let source_nodes = source_queue.pop_max();
            let destination_nodes = destination_queue.pop_max();
            let (matched_sources, matched_destinations) = match_at_height(
                source_tree,
                destination_tree,
                &source_nodes,
                &destination_nodes,
                mapping,
                &mut ambiguous,
            );

            for node_id in &source_nodes {
                if !matched_sources.contains(node_id) {
                    source_queue.open(source_tree, *node_id);
                }
            }
            for node_id in &destination_nodes {
                if !matched_destinations.contains(node_id) {
                    destination_queue.open(destination_tree, *node_id);
                }
            }
        }
    }

    super::comparators::resolve_ambiguous(
        ambiguous,
        source_tree,
        destination_tree,
        mapping,
        |src, dst, m| map_isomorphic_subtree(source_tree, src, destination_tree, dst, m),
    );
}

/// Attempts to match isomorphic nodes at the same height level.
/// Unique matches are linked immediately; ambiguous ones are deferred.
/// Returns the sets of source and destination nodes that were matched
/// (either uniquely or ambiguously) so the caller knows which nodes NOT to open.
fn match_at_height(
    source_tree: &Tree,
    destination_tree: &Tree,
    source_nodes: &[NodeId],
    destination_nodes: &[NodeId],
    mapping: &mut Mapping,
    ambiguous: &mut Vec<(NodeId, NodeId)>,
) -> (HashSet<NodeId>, HashSet<NodeId>) {
    // Group destination nodes by (hash, kind) so pair generation is O(s + d)
    // instead of O(s × d).
    let mut dest_by_signature: HashMap<(u64, &str), Vec<NodeId>> = HashMap::new();
    for &dst in destination_nodes {
        let node = destination_tree.node(dst);
        dest_by_signature
            .entry((node.hash, node.kind.as_str()))
            .or_default()
            .push(dst);
    }

    // Collect isomorphic pairs via bucket lookup.
    let mut iso_pairs: Vec<(NodeId, NodeId)> = Vec::new();
    for &src in source_nodes {
        let node = source_tree.node(src);
        let key = (node.hash, node.kind.as_str());
        if let Some(matches) = dest_by_signature.get(&key) {
            for &dst in matches {
                iso_pairs.push((src, dst));
            }
        }
    }

    // Count occurrences to detect ambiguity.
    let mut source_count: HashMap<NodeId, usize> = HashMap::new();
    let mut destination_count: HashMap<NodeId, usize> = HashMap::new();
    for &(source_node, destination_node) in &iso_pairs {
        *source_count.entry(source_node).or_insert(0) += 1;
        *destination_count.entry(destination_node).or_insert(0) += 1;
    }

    let mut matched_sources: HashSet<NodeId> = HashSet::new();
    let mut matched_destinations: HashSet<NodeId> = HashSet::new();

    for (source_node, destination_node) in &iso_pairs {
        matched_sources.insert(*source_node);
        matched_destinations.insert(*destination_node);

        let is_unique = source_count[source_node] == 1 && destination_count[destination_node] == 1;
        if !is_unique {
            ambiguous.push((*source_node, *destination_node));
            continue;
        }
        if mapping.has_src(*source_node) || mapping.has_dst(*destination_node) {
            continue;
        }
        map_isomorphic_subtree(
            source_tree,
            *source_node,
            destination_tree,
            *destination_node,
            mapping,
        );
    }

    (matched_sources, matched_destinations)
}

/// Links two isomorphic subtrees node-by-node in lockstep.
fn map_isomorphic_subtree(
    source_tree: &Tree,
    source_node: NodeId,
    destination_tree: &Tree,
    destination_node: NodeId,
    mapping: &mut Mapping,
) {
    let mut stack = vec![(source_node, destination_node)];
    while let Some((current_source, current_destination)) = stack.pop() {
        mapping.link(current_source, current_destination);
        let source_children = &source_tree.node(current_source).children;
        let destination_children = &destination_tree.node(current_destination).children;
        if source_children.len() != destination_children.len() {
            continue;
        }
        // Push in reverse so leftmost pair is processed first.
        for (source_child, destination_child) in source_children
            .iter()
            .zip(destination_children.iter())
            .rev()
        {
            stack.push((*source_child, *destination_child));
        }
    }
}

/// Dice similarity between two subtrees, given a partial mapping.
///
/// Defined as `2 * |common| / (|desc(n1)| + |desc(n2)|)` where `common` is the
/// number of descendants of `n1` whose mapped image lies within the descendants
/// of `n2`.
#[must_use]
pub fn dice_coefficient(
    source_tree: &Tree,
    source_node: NodeId,
    destination_tree: &Tree,
    destination_node: NodeId,
    mapping: &Mapping,
) -> f64 {
    super::dice_with_source_descendants(
        &source_tree.descendants(source_node),
        destination_tree,
        destination_node,
        mapping,
    )
}

/// Priority queue keyed by node height, with max-heap behaviour.
struct HeightPQ {
    buckets: BTreeMap<usize, Vec<NodeId>>,
}

impl HeightPQ {
    fn new() -> Self {
        Self {
            buckets: BTreeMap::new(),
        }
    }

    fn push(&mut self, tree: &Tree, node_id: NodeId) {
        let height = tree.node(node_id).height;
        self.buckets.entry(height).or_default().push(node_id);
    }

    fn peek_height(&self) -> Option<usize> {
        self.buckets.keys().next_back().copied()
    }

    fn pop_max(&mut self) -> Vec<NodeId> {
        let Some(max_height) = self.peek_height() else {
            return Vec::new();
        };
        self.buckets.remove(&max_height).unwrap_or_default()
    }

    fn open(&mut self, tree: &Tree, node_id: NodeId) {
        let children = tree.node(node_id).children.clone();
        for child_id in children {
            self.push(tree, child_id);
        }
    }
}
