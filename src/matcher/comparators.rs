//! Comparators for resolving ambiguous matches in the top-down phase.
//!
//! When multiple source subtrees share the same structural hash as multiple
//! destination subtrees, the matcher cannot determine pairings from structure
//! alone.  Comparators are applied in priority order; the first to return
//! a non-Equal ordering wins.
//!
//! # Adding a new comparator
//!
//! 1. Define a `compare_*` function in this file.
//! 2. Append a `.then_with(|| ...)` call in [`resolve_ambiguous`].
//! 3. Add a unit test in `tests/test_topdown.rs`.

use std::cmp::Ordering;

use crate::mapping::Mapping;
use crate::tree::{NodeId, Tree};

/// Prefer the candidate whose parents have a higher Dice similarity
/// (more matched descendants in common).
///
/// When both candidates share the same parent on each side, this returns
/// Equal — the parents' Dice scores are trivially identical.
fn compare_siblings_dice(
    candidate_a: (NodeId, NodeId),
    candidate_b: (NodeId, NodeId),
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> Ordering {
    let parent_a = (
        source_tree.node(candidate_a.0).parent,
        destination_tree.node(candidate_a.1).parent,
    );
    let parent_b = (
        source_tree.node(candidate_b.0).parent,
        destination_tree.node(candidate_b.1).parent,
    );
    if parent_a == parent_b {
        return Ordering::Equal;
    }
    let dice_a = parent_dice(
        candidate_a.0,
        candidate_a.1,
        source_tree,
        destination_tree,
        mapping,
    );
    let dice_b = parent_dice(
        candidate_b.0,
        candidate_b.1,
        source_tree,
        destination_tree,
        mapping,
    );
    // Higher dice is better → reverse comparison.
    dice_b.total_cmp(&dice_a)
}

fn parent_dice(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> f64 {
    let source_parent = source_tree.node(source_node).parent;
    let destination_parent = destination_tree.node(destination_node).parent;
    match (source_parent, destination_parent) {
        (Some(source), Some(destination)) => super::dice_with_source_descendants(
            &source_tree.descendants(source),
            destination_tree,
            destination,
            mapping,
        ),
        _ => 0.0,
    }
}

/// Prefer the candidate that preserves relative offsets to already-mapped
/// siblings.
///
/// For each sibling of `src` that is already mapped to a sibling of `dst`
/// (under the same parent), computes the difference between the source
/// offset (`src.pos − sibling.pos`) and the destination offset
/// (`dst.pos − mapped_sibling.pos`).  The sum of absolute differences is
/// the incoherence score; lower is better.
///
/// This is the key discriminator for the "identical `#[test]` attributes"
/// case: after unique function_items are matched, each `#[test]` should
/// land next to its associated function.
fn compare_sibling_coherence(
    candidate_a: (NodeId, NodeId),
    candidate_b: (NodeId, NodeId),
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> Ordering {
    let score_a = sibling_coherence(
        candidate_a.0,
        candidate_a.1,
        source_tree,
        destination_tree,
        mapping,
    );
    let score_b = sibling_coherence(
        candidate_b.0,
        candidate_b.1,
        source_tree,
        destination_tree,
        mapping,
    );
    score_a.cmp(&score_b)
}

fn sibling_coherence(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> u64 {
    let source_parent = source_tree.node(source_node).parent;
    let destination_parent = destination_tree.node(destination_node).parent;
    let source_position = source_tree.position_in_parent(source_node);
    let destination_position = destination_tree.position_in_parent(destination_node);

    let (
        Some(source_parent),
        Some(destination_parent),
        Some(source_position),
        Some(destination_position),
    ) = (
        source_parent,
        destination_parent,
        source_position,
        destination_position,
    )
    else {
        return u64::MAX;
    };

    let source_siblings = &source_tree.node(source_parent).children;

    // Find nearest mapped left and right siblings of source_node.
    let mut left_anchor: Option<(usize, usize)> = None;
    let mut right_anchor: Option<(usize, usize)> = None;

    for (sibling_index, &sibling) in source_siblings.iter().enumerate() {
        if sibling == source_node {
            continue;
        }
        let Some(mapped_destination) = mapping.get_dst(sibling) else {
            continue;
        };
        if destination_tree.node(mapped_destination).parent != Some(destination_parent) {
            continue;
        }
        let Some(mapped_index) = destination_tree.position_in_parent(mapped_destination) else {
            continue;
        };
        if sibling_index < source_position {
            // Candidate for left anchor — keep the nearest (largest sibling_index).
            if left_anchor.is_none_or(|(prev, _)| sibling_index > prev) {
                left_anchor = Some((sibling_index, mapped_index));
            }
        } else if sibling_index > source_position {
            // Candidate for right anchor — keep the nearest (smallest sibling_index).
            if right_anchor.is_none_or(|(prev, _)| sibling_index < prev) {
                right_anchor = Some((sibling_index, mapped_index));
            }
        }
    }

    let mut total: u64 = 0;
    let mut count: u64 = 0;
    for &(anchor_source, anchor_destination) in [left_anchor, right_anchor].iter().flatten() {
        let source_offset = (source_position as i64) - (anchor_source as i64);
        let destination_offset = (destination_position as i64) - (anchor_destination as i64);
        total += source_offset.abs_diff(destination_offset);
        count += 1;
    }

    if count == 0 {
        return u64::MAX / 2;
    }
    total
}

/// Prefer the candidate whose ancestor-type chain is more similar.
///
/// Collects the sequence of `kind` strings from the node up to the root and
/// computes the length of the longest common subsequence (LCS) between the
/// source and destination chains, normalised to `[0, 1]`.  Higher is better.
fn compare_ancestor_similarity(
    candidate_a: (NodeId, NodeId),
    candidate_b: (NodeId, NodeId),
    source_tree: &Tree,
    destination_tree: &Tree,
    _mapping: &Mapping,
) -> Ordering {
    let similarity_a =
        ancestor_similarity(candidate_a.0, candidate_a.1, source_tree, destination_tree);
    let similarity_b =
        ancestor_similarity(candidate_b.0, candidate_b.1, source_tree, destination_tree);
    // Higher similarity is better → reverse comparison.
    similarity_b.total_cmp(&similarity_a)
}

fn ancestor_kinds(tree: &Tree, mut node: NodeId) -> Vec<&str> {
    let mut kinds = Vec::new();
    while let Some(parent) = tree.node(node).parent {
        kinds.push(tree.node(parent).kind.as_str());
        node = parent;
    }
    kinds
}

fn ancestor_similarity(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
) -> f64 {
    let source_kinds = ancestor_kinds(source_tree, source_node);
    let destination_kinds = ancestor_kinds(destination_tree, destination_node);
    let total = source_kinds.len() + destination_kinds.len();
    if total == 0 {
        return 1.0;
    }
    let lcs_len = lcs_length_by_eq(&source_kinds, &destination_kinds);
    2.0 * lcs_len as f64 / total as f64
}

/// Standard O(n·m) LCS-length computation, comparing elements by equality.
fn lcs_length_by_eq<T: PartialEq>(left: &[T], right: &[T]) -> usize {
    let mut previous_row = vec![0usize; right.len() + 1];
    let mut current_row = vec![0usize; right.len() + 1];
    for left_item in left {
        for (column, right_item) in right.iter().enumerate() {
            current_row[column + 1] = if left_item == right_item {
                previous_row[column] + 1
            } else {
                previous_row[column + 1].max(current_row[column])
            };
        }
        std::mem::swap(&mut previous_row, &mut current_row);
        current_row.fill(0);
    }
    *previous_row.last().unwrap_or(&0)
}

/// Prefer the candidate whose position-in-parent vector is closer.
///
/// For each node, collects the child index at every ancestor level from the
/// node up to the root, producing a position vector.  The distance is the
/// sum of squared element-wise differences (Euclidean²).  Lower is better.
fn compare_position_in_parent(
    candidate_a: (NodeId, NodeId),
    candidate_b: (NodeId, NodeId),
    source_tree: &Tree,
    destination_tree: &Tree,
    _mapping: &Mapping,
) -> Ordering {
    let distance_a = position_distance(candidate_a.0, candidate_a.1, source_tree, destination_tree);
    let distance_b = position_distance(candidate_b.0, candidate_b.1, source_tree, destination_tree);
    distance_a.cmp(&distance_b)
}

fn position_vector(tree: &Tree, mut node: NodeId) -> Vec<usize> {
    let mut positions = Vec::new();
    while let Some(parent) = tree.node(node).parent {
        if let Some(position) = tree.position_in_parent(node) {
            positions.push(position);
        }
        node = parent;
    }
    positions
}

fn position_distance(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
) -> u64 {
    let source_positions = position_vector(source_tree, source_node);
    let destination_positions = position_vector(destination_tree, destination_node);
    let length = source_positions.len().max(destination_positions.len());
    let pad = usize::MAX / 2;
    let mut sum: u64 = 0;
    for index in 0..length {
        let source_value = source_positions.get(index).copied().unwrap_or(pad);
        let destination_value = destination_positions.get(index).copied().unwrap_or(pad);
        let diff = source_value.abs_diff(destination_value) as u64;
        sum = sum.saturating_add(diff.saturating_mul(diff));
    }
    sum
}

/// Prefer the candidate whose byte offsets are closest.
///
/// Final tiebreaker: when structure, ancestry, and sibling position all tie,
/// the candidate nearest in the file wins.
fn compare_textual_position(
    candidate_a: (NodeId, NodeId),
    candidate_b: (NodeId, NodeId),
    source_tree: &Tree,
    destination_tree: &Tree,
    _mapping: &Mapping,
) -> Ordering {
    let distance_a = textual_distance(candidate_a.0, candidate_a.1, source_tree, destination_tree);
    let distance_b = textual_distance(candidate_b.0, candidate_b.1, source_tree, destination_tree);
    distance_a.cmp(&distance_b)
}

fn textual_distance(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
) -> usize {
    let source = source_tree.node(source_node);
    let destination = destination_tree.node(destination_node);
    source
        .start_byte
        .abs_diff(destination.start_byte)
        .saturating_add(source.end_byte.abs_diff(destination.end_byte))
}

/// Resolve ambiguous top-down matches using the full comparator chain.
///
/// Sorts candidate pairs by the chain, then greedily links the best
/// unmatched pair via `link_pair`.
pub(crate) fn resolve_ambiguous(
    mut candidates: Vec<(NodeId, NodeId)>,
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &mut Mapping,
    mut link_pair: impl FnMut(NodeId, NodeId, &mut Mapping),
) {
    // Snapshot so Dice scores are computed against a consistent state.
    let snapshot = mapping.clone();

    candidates.sort_by(|&candidate_a, &candidate_b| {
        compare_siblings_dice(
            candidate_a,
            candidate_b,
            source_tree,
            destination_tree,
            &snapshot,
        )
        .then_with(|| {
            compare_sibling_coherence(
                candidate_a,
                candidate_b,
                source_tree,
                destination_tree,
                &snapshot,
            )
        })
        .then_with(|| {
            compare_ancestor_similarity(
                candidate_a,
                candidate_b,
                source_tree,
                destination_tree,
                &snapshot,
            )
        })
        .then_with(|| {
            compare_position_in_parent(
                candidate_a,
                candidate_b,
                source_tree,
                destination_tree,
                &snapshot,
            )
        })
        .then_with(|| {
            compare_textual_position(
                candidate_a,
                candidate_b,
                source_tree,
                destination_tree,
                &snapshot,
            )
        })
    });

    for (source_node, destination_node) in candidates {
        if !mapping.has_src(source_node) && !mapping.has_dst(destination_node) {
            link_pair(source_node, destination_node, mapping);
        }
    }
}
