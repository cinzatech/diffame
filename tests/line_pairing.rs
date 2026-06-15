//! Tests for the line-pairing algorithm itself (dst→src mapping and moved
//! detection), independent of rendering.
//!
//! Principle under test: a destination line pairs with a source line only
//! when the AST mapping links them (mapped tokens vote) or when identical
//! text aligns inside the gap between two mapping-backed anchors.  Pairs
//! must never be invented from text similarity or leftover bookkeeping,
//! because invented pairs masquerade as moved or changed lines.

use std::collections::HashMap;

use diffame::languages;
use diffame::output::line_pairing::{build_line_pairing, split_into_lines, LinePairing};
use diffame::{diff_sources, DiffOptions};

/// Diffs two Rust snippets and returns the line pairing.
fn pairing_for(old: &str, new: &str) -> LinePairing {
    let profile = languages::profile_for_ext("rs").expect("language profile");
    let result = diff_sources(
        old.as_bytes(),
        new.as_bytes(),
        profile,
        &DiffOptions::default(),
    )
    .expect("diff failed");
    let source_lines = split_into_lines(old);
    let destination_lines = split_into_lines(new);
    build_line_pairing(
        &source_lines,
        &destination_lines,
        &result.src_tree,
        &result.dst_tree,
        &result.mapping,
    )
}

/// An inserted blank line must not pair with an unrelated deleted blank
/// line elsewhere in the file.  Blank lines are invisible to the AST, so
/// they may only pair positionally inside a gap between anchors — never
/// across the file, which fabricates a "moved" blank line.
#[test]
fn inserted_blank_does_not_steal_distant_deleted_blank() {
    let old = "\
fn alpha() {
    a();
}
fn omega() {
    b();
}

fn last() {
    c();
}
";
    let new = "\
fn alpha() {
    a();
}

fn inserted() {
    i();
}
fn omega() {
    b();
}
fn last() {
    c();
}
";
    let pairing = pairing_for(old, new);

    // The new blank (dst line 3, 0-based) was inserted along with
    // `fn inserted`; the old blank (src line 6) was deleted along with
    // the omega/last gap.  Neither has a counterpart.
    assert_eq!(
        pairing.dst_to_src.get(&3),
        None,
        "inserted blank must stay unpaired, got src {:?}",
        pairing.dst_to_src.get(&3).map(|s| s + 1),
    );
    assert!(
        !pairing.dst_to_src.values().any(|&s| s == 6),
        "deleted blank (src 7) must stay unpaired, got dst_to_src {:?}",
        sorted_pairs(&pairing.dst_to_src),
    );

    // Nothing in this diff moved: alpha, omega and last keep their order.
    assert!(
        pairing.moved_dst_lines.is_empty(),
        "no lines moved in this diff, got moved {:?}",
        pairing.moved_dst_lines,
    );
}

/// A blank line that follows a function pairs with the blank that follows
/// the same function on the other side — not with whichever unmatched
/// blank comes first in file order.
#[test]
fn trailing_blank_follows_its_function() {
    let old = "\
fn alpha(a: u32) -> u32 {
    a + 1
}

fn beta(b: u32) -> u32 {
    b + 2
}
";
    let new = "\
type Alias = u32;

fn alpha(a: u32) -> u32 {
    a + 1
}

fn beta(b: u32) -> u32 {
    b + 2
}
";
    let pairing = pairing_for(old, new);

    // alpha's trailing blank: src 3 → dst 5 (0-based).
    assert_eq!(
        pairing.dst_to_src.get(&5),
        Some(&3),
        "alpha's trailing blank must follow alpha, got dst_to_src {:?}",
        sorted_pairs(&pairing.dst_to_src),
    );
    // The blank after the inserted type alias has no counterpart.
    assert_eq!(
        pairing.dst_to_src.get(&1),
        None,
        "blank after the inserted alias must stay unpaired",
    );
    assert!(
        pairing.moved_dst_lines.is_empty(),
        "a pure insertion has no moved lines, got {:?}",
        pairing.moved_dst_lines,
    );
}

/// When one old function is replaced by several new items, the matcher maps
/// the old function onto the first replacement; the further replacements
/// have no mapped tokens at all.  Their lines must stay unpaired — pairing
/// them with leftovers of the old function would render fully-red against
/// fully-green rows — and a pure delete+insert must not flag any move.
#[test]
fn extra_replacement_function_stays_unpaired() {
    let old = "\
fn stable() {
    s();
}

fn header_cell(filename: Option<&str>, language_name: Option<&str>, width: usize) -> String {
    let tag = format!(\"[{language_name:?}]\");
    let gap = usize::from(filename.is_some());
    format!(\"{tag}{gap}{width}\")
}

fn tail() {
    t();
}
";
    let new = "\
fn stable() {
    s();
}

enum HeaderStyle {
    Filename,
    Language,
}

fn header_spans(filename: Option<&str>) -> Vec<HeaderStyle> {
    vec![HeaderStyle::Filename]
}

fn render_header_spans(spans: &[Span<HeaderStyle>]) -> String {
    spans.iter().map(make_text).collect()
}

fn tail() {
    t();
}
";
    let pairing = pairing_for(old, new);

    // `fn render_header_spans` (dst 13, 0-based) and its body (dst 14)
    // have no mapped tokens; they must not pair with anything.
    for dst in [13, 14] {
        assert_eq!(
            pairing.dst_to_src.get(&dst),
            None,
            "unmapped replacement line {} must stay unpaired, got dst_to_src {:?}",
            dst + 1,
            sorted_pairs(&pairing.dst_to_src),
        );
    }

    // stable and tail keep their order around the replacement: no moves.
    assert!(
        pairing.moved_dst_lines.is_empty(),
        "delete+insert is not a move, got moved {:?}",
        pairing.moved_dst_lines,
    );
}

/// Regression test over a real refactor of this project's own
/// `terminal.rs` (commit 0723664).  The old `header_cell` function was
/// replaced by `header_spans` + `render_header_spans`; the similarity
/// heuristic used to pair the leftovers of the old function with the new
/// `render_header_spans` lines across the file — out of order — and the
/// inversion detector then flagged them all as moved.  The blank-line
/// phase likewise used to pair a blank far from its true counterpart.
#[test]
fn real_refactor_does_not_fabricate_pairs_or_moves() {
    let old = include_str!("fixtures/terminal_v1.txt");
    let new = include_str!("fixtures/terminal_v2.txt");
    let pairing = pairing_for(old, new);

    // New `fn render_header_spans` (1-based line 527) has no mapped
    // tokens: it is an insertion, not a counterpart of old line 472.
    assert_eq!(
        pairing.dst_to_src.get(&526),
        None,
        "fully-unmapped new line 527 must stay unpaired, got src {:?}",
        pairing.dst_to_src.get(&526).map(|s| s + 1),
    );

    // None of the replacement's lines (1-based 527..=536) may be flagged
    // as moved: the edit script contains a delete and an insert, no move.
    let bogus_moves: Vec<usize> = (526..536)
        .filter(|d| pairing.moved_dst_lines.contains(d))
        .map(|d| d + 1)
        .collect();
    assert!(
        bogus_moves.is_empty(),
        "delete+insert flagged as moved on lines {bogus_moves:?}",
    );

    // The blank after `build_line_spans` (old 1-based 94) pairs with the
    // blank after the same function in the new file (1-based 98) — not
    // with whichever unmatched blank comes first in file order.
    assert_eq!(
        pairing.dst_to_src.get(&97),
        Some(&93),
        "blank after build_line_spans must follow its function, got src {:?}",
        pairing.dst_to_src.get(&97).map(|s| s + 1),
    );
}

fn sorted_pairs(map: &HashMap<usize, usize>) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(usize, usize)> = map.iter().map(|(&d, &s)| (d + 1, s + 1)).collect();
    pairs.sort_unstable();
    pairs
}

/// When many `#[test]` functions exist and new ones are inserted among them,
/// the unchanged functions must not be flagged as moved.  The ambiguous
/// `#[test]` attribute_item nodes (all structurally identical) must be
/// resolved by the comparator chain using mapped-sibling context, not
/// arbitrary tie-breaking.
#[test]
fn inserting_tests_among_existing_does_not_cascade_moved_flags() {
    let old = "\
use std::collections::HashSet;

#[test]
fn alpha() {
    let x = HashSet::new();
    assert!(x.is_empty());
}

#[test]
fn beta() {
    let y = vec![1, 2, 3];
    assert_eq!(y.len(), 3);
}

#[test]
fn gamma() {
    let z = String::from(\"hello\");
    assert_eq!(z.len(), 5);
}

#[test]
fn delta() {
    let w = Some(42);
    assert_eq!(w.unwrap(), 42);
}

#[test]
fn epsilon() {
    let v: Vec<i32> = (0..10).collect();
    assert_eq!(v.len(), 10);
}
";
    let new = "\
use std::collections::HashSet;

#[test]
fn alpha() {
    let x = HashSet::new();
    assert!(x.is_empty());
}

#[test]
fn alpha_two() {
    let a = HashSet::from([1, 2]);
    assert_eq!(a.len(), 2);
}

#[test]
fn beta() {
    let y = vec![1, 2, 3];
    assert_eq!(y.len(), 3);
}

#[test]
fn beta_two() {
    let b = vec![4, 5];
    assert_eq!(b.len(), 2);
}

#[test]
fn gamma() {
    let z = String::from(\"hello\");
    assert_eq!(z.len(), 5);
}

#[test]
fn delta() {
    let w = Some(42);
    assert_eq!(w.unwrap(), 42);
}

#[test]
fn epsilon() {
    let v: Vec<i32> = (0..10).collect();
    assert_eq!(v.len(), 10);
}
";
    let pairing = pairing_for(old, new);

    let unchanged_fns = [
        "fn beta() {",
        "fn gamma() {",
        "fn delta() {",
        "fn epsilon() {",
    ];

    let new_lines = split_into_lines(new);
    let mut checked = 0;
    for (dst_idx, line) in new_lines.iter().enumerate() {
        let text = line.text.trim();
        if unchanged_fns.contains(&text) {
            assert!(
                !pairing.moved_dst_lines.contains(&dst_idx),
                "line {} ({:?}) should not be flagged as moved",
                dst_idx + 1,
                text,
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        unchanged_fns.len(),
        "all unchanged fns must be found"
    );
}
