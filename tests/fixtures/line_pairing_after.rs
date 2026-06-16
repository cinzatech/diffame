//! Line-pairing algorithm: builds a bijective destination-line → source-line mapping.
//!
//! The pairing is a projection of the AST mapping onto lines, completed by
//! one classical text alignment for what the AST cannot see:
//!
//! 1. **Votes**: every mapped destination token votes for the source line
//!    its counterpart lives on; the majority wins.  This is the mapping —
//!    and therefore the edit script — read line by line.
//! 2. **Gap alignment**: between consecutive anchors, lines invisible to
//!    the mapping (blank lines, interior lines of multi-line tokens) are
//!    aligned by longest common subsequence over identical trimmed text.
//!    Alignment is order-preserving inside each gap, so it can never
//!    fabricate a move.
//!
//! Lines that neither phase pairs stay unpaired and render as whole-line
//! deletions or insertions, exactly as the edit script describes them.  No
//! pair is ever invented from text similarity or positional bookkeeping:
//! an invented pair masquerades as a changed or moved line that the edit
//! script does not contain.
//!
//! This module is independent of any rendering concern and can be reused by
//! any output format that needs to align old and new lines.

use std::collections::{HashMap, HashSet};

use crate::lis::longest_increasing_subsequence;
use crate::mapping::Mapping;
use crate::tree::Tree;

/// A single logical line within a source buffer.
#[derive(Debug)]
pub struct FileLine<'a> {
    pub text: &'a str,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// The result of pairing source and destination lines.
pub struct LinePairing {
    /// Maps destination line index → source line index.
    pub dst_to_src: HashMap<usize, usize>,
    /// Destination lines that belong to moved (reordered) blocks.
    pub moved_dst_lines: HashSet<usize>,
}

/// Splits source text into [`FileLine`]s on `\n` boundaries.
///
/// Callers with raw bytes should convert with `String::from_utf8_lossy`
/// first (matching `build_line_tree`'s behaviour) and keep the resulting
/// `Cow` alive for as long as the lines are used.
#[must_use]
pub fn split_into_lines(text: &str) -> Vec<FileLine<'_>> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        let end = offset + line.len();
        lines.push(FileLine {
            text: line,
            start_byte: offset,
            end_byte: end,
        });
        offset = end + 1;
    }
    if lines.last().is_some_and(|last| last.text.is_empty()) {
        lines.pop();
    }
    lines
}

/// Returns the line index that contains the given byte offset.
#[must_use]
pub fn line_index_at_byte(lines: &[FileLine], byte_offset: usize) -> Option<usize> {
    // Lines are sorted and contiguous, so binary search applies.
    let index = lines.partition_point(|line| line.end_byte < byte_offset);
    (index < lines.len() && byte_offset >= lines[index].start_byte).then_some(index)
}

/// Builds a complete [`LinePairing`] from the AST-level mapping.
#[must_use]
pub fn build_line_pairing<'a>(
    source_lines: &[FileLine<'a>],
    destination_lines: &[FileLine<'a>],
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> LinePairing {
    let candidates = phase1_vote(
        source_lines,
        destination_lines,
        source_tree,
        destination_tree,
        mapping,
    );
    let dst_to_src = phase2_align(candidates, source_lines, destination_lines);
    let moved_dst_lines = detect_moved_destination_lines(&dst_to_src, destination_lines.len());
    LinePairing {
        dst_to_src,
        moved_dst_lines,
    }
}

/// Phase 1: every mapped destination leaf votes for the source line its
/// mapped counterpart lives on; each destination line's majority wins.
fn phase1_vote<'a>(
    source_lines: &[FileLine<'a>],
    destination_lines: &[FileLine<'a>],
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
) -> Vec<(usize, usize, usize)> {
    let mut votes: HashMap<usize, HashMap<usize, usize>> = HashMap::new();

    for destination_node in destination_tree.all_nodes() {
        if !destination_node.children.is_empty() {
            continue;
        }
        let Some(source_node_id) = mapping.get_src(destination_node.id) else {
            continue;
        };
        let source_node = source_tree.node(source_node_id);

        let (Some(destination_line), Some(source_line)) = (
            line_index_at_byte(destination_lines, destination_node.start_byte),
            line_index_at_byte(source_lines, source_node.start_byte),
        ) else {
            continue;
        };

        *votes
            .entry(destination_line)
            .or_default()
            .entry(source_line)
            .or_insert(0) += 1;
    }

    let mut candidates: Vec<(usize, usize, usize)> = votes
        .into_iter()
        .filter_map(|(destination_line, line_votes)| {
            line_votes
                .into_iter()
                .max_by_key(|&(source_line, count)| (count, std::cmp::Reverse(source_line)))
                .map(|(source_line, count)| (destination_line, source_line, count))
        })
        .collect();

    // Strongest anchors claim their source line first; ties break on the
    // destination index so the result is deterministic.
    candidates
        .sort_by_key(|&(destination_line, _, count)| (std::cmp::Reverse(count), destination_line));
    candidates
}

/// Phase 2: accept anchors (strongest first, one source line each), then
/// align the lines inside each gap between consecutive anchors by identical
/// trimmed text, keeping their relative order.
fn phase2_align(
    candidates: Vec<(usize, usize, usize)>,
    source_lines: &[FileLine],
    destination_lines: &[FileLine],
) -> HashMap<usize, usize> {
    let mut result: HashMap<usize, usize> = HashMap::new();
    let mut used_source_lines: HashSet<usize> = HashSet::new();

    for (destination_line, source_line, _) in candidates {
        if used_source_lines.contains(&source_line) {
            continue;
        }
        result.insert(destination_line, source_line);
        used_source_lines.insert(source_line);
    }

    let destination_count = destination_lines.len();
    let source_count = source_lines.len();

    // Collect anchors sorted by destination index to define gap boundaries.
    let mut anchors: Vec<(usize, usize)> = result
        .iter()
        .map(|(&destination, &source)| (destination, source))
        .collect();
    anchors.sort_by_key(|&(destination, _)| destination);

    let mut gaps: Vec<(usize, usize, usize, usize)> = Vec::new();
    if anchors.is_empty() {
        gaps.push((0, destination_count, 0, source_count));
    } else {
        let (first_destination, first_source) = anchors[0];
        if first_destination > 0 || first_source > 0 {
            gaps.push((0, first_destination, 0, first_source));
        }
        for window in anchors.windows(2) {
            let (prev_destination, prev_source) = window[0];
            let (next_destination, next_source) = window[1];
            // Crossed anchors (a moved block) cannot form a meaningful gap.
            if prev_source < next_source {
                gaps.push((
                    prev_destination + 1,
                    next_destination,
                    prev_source + 1,
                    next_source,
                ));
            }
        }
        let (last_destination, last_source) = *anchors.last().unwrap();
        if last_destination + 1 < destination_count || last_source + 1 < source_count {
            gaps.push((
                last_destination + 1,
                destination_count,
                last_source + 1,
                source_count,
            ));
        }
    }

    // Per gap: align remaining lines with identical trimmed text, keeping
    // their relative order.  This is what pairs blank lines and bare
    // punctuation lines, which carry no tokens the mapping could see.
    for &(dst_from, dst_to, src_from, src_to) in &gaps {
        let available: Vec<usize> = (src_from..src_to)
            .filter(|source| !used_source_lines.contains(source))
            .collect();
        let unmatched: Vec<usize> = (dst_from..dst_to)
            .filter(|destination| !result.contains_key(destination))
            .collect();
        let text_matches = |destination: usize, source: usize| {
            destination_lines[destination].text.trim() == source_lines[source].text.trim()
        };
        for (destination, source) in lcs_matches(&unmatched, &available, &text_matches) {
            result.insert(destination, source);
            used_source_lines.insert(source);
        }
    }

    result
}

/// Longest common subsequence of `destination_indices` and `source_indices`
/// (slices of line indices) under `equal`, returned as `(destination, source)`
/// pairs in increasing order.
///
/// Falls back to greedy first-available matching when the DP table would be
/// unreasonably large (only possible when a diff has almost no anchors).
fn lcs_matches(
    destination_indices: &[usize],
    source_indices: &[usize],
    equal: &impl Fn(usize, usize) -> bool,
) -> Vec<(usize, usize)> {
    const MAX_CELLS: usize = 1_000_000;
    if destination_indices.is_empty() || source_indices.is_empty() {
        return Vec::new();
    }
    if destination_indices
        .len()
        .saturating_mul(source_indices.len())
        > MAX_CELLS
    {
        let mut available: Vec<usize> = source_indices.to_vec();
        let mut pairs = Vec::new();
        for &destination in destination_indices {
            if let Some(position) = available
                .iter()
                .position(|&source| equal(destination, source))
            {
                pairs.push((destination, available.remove(position)));
            }
        }
        return pairs;
    }

    let rows = destination_indices.len();
    let columns = source_indices.len();
    let cell_index = |row: usize, column: usize| row * (columns + 1) + column;
    let mut table = vec![0u32; (rows + 1) * (columns + 1)];
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            table[cell_index(row, column)] =
                if equal(destination_indices[row], source_indices[column]) {
                    table[cell_index(row + 1, column + 1)] + 1
                } else {
                    table[cell_index(row + 1, column)].max(table[cell_index(row, column + 1)])
                };
        }
    }

    let mut pairs = Vec::new();
    let (mut row, mut column) = (0, 0);
    while row < rows && column < columns {
        if equal(destination_indices[row], source_indices[column])
            && table[cell_index(row, column)] == table[cell_index(row + 1, column + 1)] + 1
        {
            pairs.push((destination_indices[row], source_indices[column]));
            row += 1;
            column += 1;
        } else if table[cell_index(row + 1, column)] >= table[cell_index(row, column + 1)] {
            row += 1;
        } else {
            column += 1;
        }
    }
    pairs
}

/// Detects which destination lines belong to moved blocks by finding the
/// minimal set of blocks that are out of order.
///
/// Uses the longest increasing subsequence (LIS) of source-line
/// representatives: blocks NOT in the LIS are the ones that moved.
fn detect_moved_destination_lines(
    line_mapping: &HashMap<usize, usize>,
    destination_line_count: usize,
) -> HashSet<usize> {
    let mut blocks: Vec<(Vec<usize>, usize)> = Vec::new();

    for destination_line in 0..destination_line_count {
        let Some(&source_line) = line_mapping.get(&destination_line) else {
            continue;
        };
        let extends_previous = blocks
            .last()
            .is_some_and(|(dst_lines, first_src)| source_line == first_src + dst_lines.len());
        if extends_previous {
            blocks.last_mut().unwrap().0.push(destination_line);
        } else {
            blocks.push((vec![destination_line], source_line));
        }
    }

    if blocks.len() <= 1 {
        return HashSet::new();
    }

    let source_representatives: Vec<usize> = blocks.iter().map(|(_, source)| *source).collect();
    let in_order_indices: HashSet<usize> = longest_increasing_subsequence(&source_representatives)
        .into_iter()
        .collect();

    let mut moved_lines: HashSet<usize> = HashSet::new();
    for (block_index, (destination_lines, _)) in blocks.iter().enumerate() {
        if !in_order_indices.contains(&block_index) {
            for &destination_line in destination_lines {
                moved_lines.insert(destination_line);
            }
        }
    }
    moved_lines
}
