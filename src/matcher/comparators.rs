//! Comparators for resolving ambiguous matches in the top-down phase.
//!
//! When multiple source subtrees share the same structural hash as multiple
//! destination subtrees, the matcher cannot determine pairings from structure
//! alone.  Comparators are applied in priority order; the first to return
//! a non-Equal ordering wins.
//!
//! # Adding a new comparator
//!
//! 1. Define a struct implementing [`Comparator`] in this file.
//! 2. Append a `.then_with(|| ...)` call in [`resolve_ambiguous`].
//! 3. Add a unit test in `tests/test_topdown.rs`.

use std::cmp::Ordering;

use crate::mapping::Mapping;
use crate::tree::{NodeId, Tree};

// ── Trait ──────────────────────────────────────────────────────────────────

/// A comparator that ranks two candidate pairings during ambiguous resolution.
///
/// Given two candidates `a = (src_a → dst_a)` and `b = (src_b → dst_b)`,
/// return [`Ordering::Less`] to prefer `a`, [`Ordering::Greater`] to prefer
/// `b`, or [`Ordering::Equal`] to defer to the next comparator in the chain.
pub(crate) trait Comparator {
    fn compare(
        &self,
        a: (NodeId, NodeId),
        b: (NodeId, NodeId),
        source_tree: &Tree,
        destination_tree: &Tree,
        mapping: &Mapping,
    ) -> Ordering;
}

// ── 1. SiblingsDice ────────────────────────────────────────────────────────

/// Prefer the candidate whose parents have a higher Dice similarity
/// (more matched descendants in common).
///
/// When both candidates share the same parent on each side, this returns
/// Equal — the parents' Dice scores are trivially identical.
pub(crate) struct SiblingsDice;

impl Comparator for SiblingsDice {
    fn compare(
        &self,
        a: (NodeId, NodeId),
        b: (NodeId, NodeId),
        source_tree: &Tree,
        destination_tree: &Tree,
        mapping: &Mapping,
    ) -> Ordering {
        let parent_a = (
            source_tree.node(a.0).parent,
            destination_tree.node(a.1).parent,
        );
        let parent_b = (
            source_tree.node(b.0).parent,
            destination_tree.node(b.1).parent,
        );
        // Short-circuit: same parents on both sides → identical dice.
        if parent_a == parent_b {
            return Ordering::Equal;
        }
        let dice_a = parent_dice(a.0, a.1, source_tree, destination_tree, mapping);
        let dice_b = parent_dice(b.0, b.1, source_tree, destination_tree, mapping);
        // Higher dice is better → reverse comparison.
        dice_b.total_cmp(&dice_a)
    }
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
        (Some(sp), Some(dp)) => super::dice_with_source_descendants(
            &source_tree.descendants(sp),
            destination_tree,
            dp,
            mapping,
        ),
        _ => 0.0,
    }
}

// ── 2. MappedSiblingCoherence ──────────────────────────────────────────────

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
pub(crate) struct MappedSiblingCoherence;

impl Comparator for MappedSiblingCoherence {
    fn compare(
        &self,
        a: (NodeId, NodeId),
        b: (NodeId, NodeId),
        source_tree: &Tree,
        destination_tree: &Tree,
        mapping: &Mapping,
    ) -> Ordering {
        let score_a = sibling_coherence(a.0, a.1, source_tree, destination_tree, mapping);
        let score_b = sibling_coherence(b.0, b.1, source_tree, destination_tree, mapping);
        score_a.cmp(&score_b)
    }
}

fn sibling_coherence(
    src_node: NodeId,
    dst_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> u64 {
    let src_parent = match source_tree.node(src_node).parent {
        Some(p) => p,
        None => return u64::MAX,
    };
    let dst_parent = match destination_tree.node(dst_node).parent {
        Some(p) => p,
        None => return u64::MAX,
    };
    let src_pos = match source_tree.position_in_parent(src_node) {
        Some(p) => p,
        None => return u64::MAX,
    };
    let dst_pos = match destination_tree.position_in_parent(dst_node) {
        Some(p) => p,
        None => return u64::MAX,
    };

    let src_siblings = &source_tree.node(src_parent).children;

    // Find nearest mapped left and right siblings of src_node.
    let mut left_anchor: Option<(usize, usize)> = None; // (src_idx, dst_idx)
    let mut right_anchor: Option<(usize, usize)> = None;

    for (s_idx, &sibling) in src_siblings.iter().enumerate() {
        if sibling == src_node {
            continue;
        }
        if let Some(mapped_dst) = mapping.get_dst(sibling) {
            if destination_tree.node(mapped_dst).parent != Some(dst_parent) {
                continue;
            }
            if let Some(d_idx) = destination_tree.position_in_parent(mapped_dst) {
                if s_idx < src_pos {
                    // Candidate for left anchor — keep the nearest (largest s_idx).
                    if left_anchor.is_none_or(|(prev, _)| s_idx > prev) {
                        left_anchor = Some((s_idx, d_idx));
                    }
                } else if s_idx > src_pos {
                    // Candidate for right anchor — keep the nearest (smallest s_idx).
                    if right_anchor.is_none_or(|(prev, _)| s_idx < prev) {
                        right_anchor = Some((s_idx, d_idx));
                    }
                }
            }
        }
    }

    let mut total: u64 = 0;
    let mut count: u64 = 0;
    for &(s_idx, d_idx) in [left_anchor, right_anchor].iter().flatten() {
        let src_offset = (src_pos as i64) - (s_idx as i64);
        let dst_offset = (dst_pos as i64) - (d_idx as i64);
        total += src_offset.abs_diff(dst_offset);
        count += 1;
    }

    if count == 0 {
        return u64::MAX / 2;
    }
    total
}

// ── 3. AncestorSimilarity ──────────────────────────────────────────────────

/// Prefer the candidate whose ancestor-type chain is more similar.
///
/// Collects the sequence of `kind` strings from the node up to the root and
/// computes the length of the longest common subsequence (LCS) between the
/// source and destination chains, normalised to `[0, 1]`.  Higher is better.
pub(crate) struct AncestorSimilarity;

impl Comparator for AncestorSimilarity {
    fn compare(
        &self,
        a: (NodeId, NodeId),
        b: (NodeId, NodeId),
        source_tree: &Tree,
        destination_tree: &Tree,
        _mapping: &Mapping,
    ) -> Ordering {
        let sim_a = ancestor_similarity(a.0, a.1, source_tree, destination_tree);
        let sim_b = ancestor_similarity(b.0, b.1, source_tree, destination_tree);
        // Higher similarity is better → reverse comparison.
        sim_b.total_cmp(&sim_a)
    }
}

fn ancestor_kinds(tree: &Tree, mut node: NodeId) -> Vec<String> {
    let mut kinds = Vec::new();
    while let Some(parent) = tree.node(node).parent {
        kinds.push(tree.node(parent).kind.clone());
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
    let src_kinds = ancestor_kinds(source_tree, source_node);
    let dst_kinds = ancestor_kinds(destination_tree, destination_node);
    let total = src_kinds.len() + dst_kinds.len();
    if total == 0 {
        return 1.0;
    }
    let lcs_len = lcs_length_by_eq(&src_kinds, &dst_kinds);
    2.0 * lcs_len as f64 / total as f64
}

/// Standard O(n·m) LCS-length computation, comparing elements by equality.
fn lcs_length_by_eq<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut curr = vec![0usize; b.len() + 1];
    for ai in a {
        for (j, bj) in b.iter().enumerate() {
            curr[j + 1] = if ai == bj {
                prev[j] + 1
            } else {
                prev[j + 1].max(curr[j])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }
    *prev.last().unwrap_or(&0)
}

// ── 3. PositionInParent ────────────────────────────────────────────────────

/// Prefer the candidate whose position-in-parent vector is closer.
///
/// For each node, collects the child index at every ancestor level from the
/// node up to the root, producing a position vector.  The distance is the
/// sum of squared element-wise differences (Euclidean²).  Lower is better.
pub(crate) struct PositionInParent;

impl Comparator for PositionInParent {
    fn compare(
        &self,
        a: (NodeId, NodeId),
        b: (NodeId, NodeId),
        source_tree: &Tree,
        destination_tree: &Tree,
        _mapping: &Mapping,
    ) -> Ordering {
        let dist_a = position_distance(a.0, a.1, source_tree, destination_tree);
        let dist_b = position_distance(b.0, b.1, source_tree, destination_tree);
        dist_a.cmp(&dist_b)
    }
}

fn position_vector(tree: &Tree, mut node: NodeId) -> Vec<usize> {
    let mut vec = Vec::new();
    while let Some(parent) = tree.node(node).parent {
        if let Some(pos) = tree.position_in_parent(node) {
            vec.push(pos);
        }
        node = parent;
    }
    vec
}

fn position_distance(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
) -> u64 {
    let src = position_vector(source_tree, source_node);
    let dst = position_vector(destination_tree, destination_node);
    let len = src.len().max(dst.len());
    let pad = usize::MAX / 2;
    let mut sum: u64 = 0;
    for i in 0..len {
        let a = src.get(i).copied().unwrap_or(pad);
        let b = dst.get(i).copied().unwrap_or(pad);
        let diff = a.abs_diff(b) as u64;
        sum = sum.saturating_add(diff.saturating_mul(diff));
    }
    sum
}

// ── 4. TextualPosition ────────────────────────────────────────────────────

/// Prefer the candidate whose byte offsets are closest.
///
/// Final tiebreaker: when structure, ancestry, and sibling position all tie,
/// the candidate nearest in the file wins.
pub(crate) struct TextualPosition;

impl Comparator for TextualPosition {
    fn compare(
        &self,
        a: (NodeId, NodeId),
        b: (NodeId, NodeId),
        source_tree: &Tree,
        destination_tree: &Tree,
        _mapping: &Mapping,
    ) -> Ordering {
        let dist_a = textual_distance(a.0, a.1, source_tree, destination_tree);
        let dist_b = textual_distance(b.0, b.1, source_tree, destination_tree);
        dist_a.cmp(&dist_b)
    }
}

fn textual_distance(
    source_node: NodeId,
    destination_node: NodeId,
    source_tree: &Tree,
    destination_tree: &Tree,
) -> usize {
    let src = source_tree.node(source_node);
    let dst = destination_tree.node(destination_node);
    src.start_byte
        .abs_diff(dst.start_byte)
        .saturating_add(src.end_byte.abs_diff(dst.end_byte))
}

// ── Chain ──────────────────────────────────────────────────────────────────

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
    let siblings_dice = SiblingsDice;
    let sibling_coherence = MappedSiblingCoherence;
    let ancestor_sim = AncestorSimilarity;
    let position = PositionInParent;
    let textual = TextualPosition;

    // Snapshot so Dice scores are computed against a consistent state.
    let snapshot = mapping.clone();

    candidates.sort_by(|&a, &b| {
        siblings_dice
            .compare(a, b, source_tree, destination_tree, &snapshot)
            .then_with(|| sibling_coherence.compare(a, b, source_tree, destination_tree, &snapshot))
            .then_with(|| ancestor_sim.compare(a, b, source_tree, destination_tree, &snapshot))
            .then_with(|| position.compare(a, b, source_tree, destination_tree, &snapshot))
            .then_with(|| textual.compare(a, b, source_tree, destination_tree, &snapshot))
    });

    for (source_node, destination_node) in candidates {
        if !mapping.has_src(source_node) && !mapping.has_dst(destination_node) {
            link_pair(source_node, destination_node, mapping);
        }
    }
}
