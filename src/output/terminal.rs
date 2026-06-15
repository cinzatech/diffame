//! Side-by-side colored terminal output.
//!
//! Colors are applied per AST span, not per line:
//!
//! * **Yellow** line numbers: the line belongs to a moved block.
//! * **Red** spans on the left: deleted tokens.
//! * **Green** spans on the right: inserted tokens.
//! * **Yellow** spans on both sides: updated (label-changed) tokens.
//! * Default: unchanged tokens and inter-token whitespace.
//! * `•`: filler for absent lines with no counterpart on the other side.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use owo_colors::OwoColorize;
use unicode_width::UnicodeWidthChar;

use crate::actions::Action;
use crate::tree::{NodeId, Tree};

use super::line_pairing::{build_line_pairing, split_into_lines, FileLine};
use super::{DiffFormatter, FormatInput};

/// The fill character used when a line has no counterpart on the other side.
const ABSENT_FILL: char = '•';

#[derive(Debug, Clone, Copy, PartialEq)]
enum SpanColor {
    Unchanged,
    Updated,
    Deleted,
    Inserted,
}

/// Source nodes targeted by an `Update` action.
fn updated_nodes(actions: &[Action]) -> HashSet<NodeId> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Update { node, .. } => Some(*node),
            _ => None,
        })
        .collect()
}

/// Colors every leaf of `tree`: `unmapped_color` when `is_unmapped`,
/// `Updated` when `is_updated`, `Unchanged` otherwise. The two closures
/// encode the only source/destination asymmetry (which side of the mapping
/// to consult).
fn classify_leaves(
    tree: &Tree,
    is_unmapped: impl Fn(NodeId) -> bool,
    is_updated: impl Fn(NodeId) -> bool,
    unmapped_color: SpanColor,
) -> HashMap<NodeId, SpanColor> {
    tree.all_nodes()
        .filter(|node| node.children.is_empty())
        .map(|node| {
            let color = if is_unmapped(node.id) {
                unmapped_color
            } else if is_updated(node.id) {
                SpanColor::Updated
            } else {
                SpanColor::Unchanged
            };
            (node.id, color)
        })
        .collect()
}

/// A run of text with a uniform style, generic over the style vocabulary:
/// diff rows use [`SpanColor`], the file header uses [`HeaderStyle`].
#[derive(Debug, Clone)]
struct Span<'a, C> {
    text: &'a str,
    color: C,
}

type ColoredSpan<'a> = Span<'a, SpanColor>;

fn build_line_spans<'a>(
    line: &FileLine<'a>,
    tree: &Tree,
    leaf_colors: &HashMap<NodeId, SpanColor>,
) -> Vec<ColoredSpan<'a>> {
    let mut node_spans: Vec<(usize, usize, SpanColor)> = tree
        .all_nodes()
        .filter(|node| node.children.is_empty())
        .filter(|node| node.end_byte > line.start_byte && node.start_byte < line.end_byte)
        .filter_map(|node| leaf_colors.get(&node.id).map(|&color| (node, color)))
        .map(|(node, color)| {
            let start = node.start_byte.saturating_sub(line.start_byte);
            let end = (node.end_byte - line.start_byte).min(line.text.len());
            (start.min(end), end, color)
        })
        .collect();

    node_spans.sort_by_key(|span| span.0);

    let mut spans: Vec<ColoredSpan> = Vec::new();
    let mut position = 0;
    for (start, end, color) in &node_spans {
        if *start > position {
            spans.push(ColoredSpan {
                text: &line.text[position..*start],
                color: SpanColor::Unchanged,
            });
        }
        if *end > *start {
            spans.push(ColoredSpan {
                text: &line.text[*start..*end],
                color: *color,
            });
        }
        position = *end;
    }
    if position < line.text.len() {
        spans.push(ColoredSpan {
            text: &line.text[position..],
            color: SpanColor::Unchanged,
        });
    }
    spans
}

struct OutputRow<'a> {
    source_line_number: Option<usize>,
    source_spans: Vec<ColoredSpan<'a>>,
    destination_line_number: Option<usize>,
    destination_spans: Vec<ColoredSpan<'a>>,
    is_changed: bool,
    is_moved: bool,
}

impl<'a> OutputRow<'a> {
    /// Unpaired source line: entire line is deleted.
    fn deleted(line_index: usize, text: &'a str) -> Self {
        Self {
            source_line_number: Some(line_index + 1),
            source_spans: vec![ColoredSpan {
                text,
                color: SpanColor::Deleted,
            }],
            destination_line_number: None,
            destination_spans: Vec::new(),
            is_changed: true,
            is_moved: false,
        }
    }

    /// Unpaired destination line: entire line is inserted.
    fn inserted(line_index: usize, text: &'a str) -> Self {
        Self {
            source_line_number: None,
            source_spans: Vec::new(),
            destination_line_number: Some(line_index + 1),
            destination_spans: vec![ColoredSpan {
                text,
                color: SpanColor::Inserted,
            }],
            is_changed: true,
            is_moved: false,
        }
    }
}

struct DiffContext<'a> {
    source_tree: &'a Tree,
    destination_tree: &'a Tree,
    source_leaf_colors: &'a HashMap<NodeId, SpanColor>,
    destination_leaf_colors: &'a HashMap<NodeId, SpanColor>,
}

fn build_output_rows<'a>(
    source_lines: &[FileLine<'a>],
    destination_lines: &[FileLine<'a>],
    line_mapping: &HashMap<usize, usize>,
    moved_destination_lines: &HashSet<usize>,
    context: &DiffContext,
) -> Vec<OutputRow<'a>> {
    let reverse_mapping: HashMap<usize, usize> = line_mapping
        .iter()
        .map(|(&destination, &source)| (source, destination))
        .collect();

    let mut rows: Vec<OutputRow> = Vec::new();
    let mut emitted_source_lines: HashSet<usize> = HashSet::new();
    let mut last_source_line: Option<usize> = None;

    for (destination_index, destination_line) in destination_lines.iter().enumerate() {
        let Some(&source_index) = line_mapping.get(&destination_index) else {
            rows.push(OutputRow::inserted(
                destination_index,
                destination_line.text,
            ));
            continue;
        };

        let gap_start = last_source_line.map_or(0, |last| last + 1);
        if source_index >= gap_start {
            for (gap, gap_line) in source_lines
                .iter()
                .enumerate()
                .skip(gap_start)
                .take(source_index - gap_start)
            {
                if reverse_mapping.contains_key(&gap) || emitted_source_lines.contains(&gap) {
                    continue;
                }
                rows.push(OutputRow::deleted(gap, gap_line.text));
                emitted_source_lines.insert(gap);
            }
        }

        let is_changed = source_lines[source_index].text != destination_line.text;
        let is_moved = moved_destination_lines.contains(&destination_index);

        // Paired identical lines: suppress all coloring so that AST
        // mapping noise (e.g. on moved code) doesn't produce false diffs.
        let (source_spans, destination_spans) = if is_changed {
            (
                build_line_spans(
                    &source_lines[source_index],
                    context.source_tree,
                    context.source_leaf_colors,
                ),
                build_line_spans(
                    destination_line,
                    context.destination_tree,
                    context.destination_leaf_colors,
                ),
            )
        } else {
            (
                vec![ColoredSpan {
                    text: source_lines[source_index].text,
                    color: SpanColor::Unchanged,
                }],
                vec![ColoredSpan {
                    text: destination_line.text,
                    color: SpanColor::Unchanged,
                }],
            )
        };

        rows.push(OutputRow {
            source_line_number: Some(source_index + 1),
            source_spans,
            destination_line_number: Some(destination_index + 1),
            destination_spans,
            is_changed,
            is_moved,
        });
        emitted_source_lines.insert(source_index);
        last_source_line = Some(source_index);
    }

    for (source_index, source_line) in source_lines.iter().enumerate() {
        if emitted_source_lines.contains(&source_index)
            || reverse_mapping.contains_key(&source_index)
        {
            continue;
        }
        rows.push(OutputRow::deleted(source_index, source_line.text));
    }

    rows
}

fn extract_hunks(rows: &[OutputRow], context: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let displayed = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.is_changed || row.is_moved);
    for (index, _) in displayed {
        let start = index.saturating_sub(context);
        let end = (index + context + 1).min(rows.len());
        if let Some(last) = ranges.last_mut() {
            if start <= last.1 {
                last.1 = end;
                continue;
            }
        }
        ranges.push((start, end));
    }
    ranges
}

fn render_spans(spans: &[ColoredSpan]) -> String {
    spans
        .iter()
        .map(|span| match span.color {
            SpanColor::Unchanged => span.text.to_string(),
            SpanColor::Updated => span.text.yellow().to_string(),
            SpanColor::Deleted => span.text.red().to_string(),
            SpanColor::Inserted => span.text.green().to_string(),
        })
        .collect()
}

/// Builds a dimmed fill string of `•` characters to occupy `width` columns.
fn absent_fill(width: usize) -> String {
    let fill: String = std::iter::repeat_n(ABSENT_FILL, width).collect();
    fill.dimmed().to_string()
}

/// Split a list of styled spans into visual lines of at most `width` columns.
/// Each returned element is (spans, `visual_length`).
///
/// Uses Unicode display width so that multi-byte characters like `─` (3 bytes,
/// 1 column) and CJK characters (3–4 bytes, 2 columns) are measured correctly.
fn wrap_spans<'a, C: Copy>(spans: &[Span<'a, C>], width: usize) -> Vec<(Vec<Span<'a, C>>, usize)> {
    if width == 0 {
        let total: usize = spans.iter().map(|span| display_width(span.text)).sum();
        return vec![(spans.to_vec(), total)];
    }
    let mut lines: Vec<(Vec<Span<C>>, usize)> = Vec::new();
    let mut current_spans: Vec<Span<C>> = Vec::new();
    let mut current_len: usize = 0; // display columns

    for span in spans {
        let mut remaining = span.text;
        while !remaining.is_empty() {
            let avail = width.saturating_sub(current_len);
            if avail == 0 {
                lines.push((std::mem::take(&mut current_spans), current_len));
                current_len = 0;
                continue;
            }
            let (take_bytes, take_cols) = bytes_fitting_columns(remaining, avail);
            if take_bytes == 0 {
                // Character wider than available space; wrap to new line.
                if !current_spans.is_empty() {
                    lines.push((std::mem::take(&mut current_spans), current_len));
                    current_len = 0;
                }
                let (take_bytes, take_cols) = bytes_fitting_columns(remaining, width);
                let take_bytes = take_bytes.max(remaining.chars().next().map_or(1, char::len_utf8));
                current_spans.push(Span {
                    text: &remaining[..take_bytes],
                    color: span.color,
                });
                current_len += take_cols.max(1);
                remaining = &remaining[take_bytes..];
            } else {
                current_spans.push(Span {
                    text: &remaining[..take_bytes],
                    color: span.color,
                });
                current_len += take_cols;
                remaining = &remaining[take_bytes..];
            }
        }
    }
    if !current_spans.is_empty() || lines.is_empty() {
        lines.push((current_spans, current_len));
    }
    lines
}

/// Returns (byte_count, column_count) for the longest prefix of `text`
/// that fits within `max_cols` display columns.
fn bytes_fitting_columns(text: &str, max_cols: usize) -> (usize, usize) {
    let mut cols = 0;
    let mut bytes = 0;
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(0);
        if cols + char_width > max_cols {
            break;
        }
        cols += char_width;
        bytes += ch.len_utf8();
    }
    (bytes, cols)
}

/// Display width of a string in terminal columns.
fn display_width(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}

/// Formats one line-number cell: right-aligned and dimmed (or yellow when the
/// row is moved) on the first visual line, blank on wrapped continuation
/// lines or when the side has no line.
fn number_cell(number: Option<usize>, visual_index: usize, is_moved: bool, width: usize) -> String {
    match number {
        Some(num) if visual_index == 0 => {
            let cell = format!("{num:>width$}");
            if is_moved {
                cell.yellow().to_string()
            } else {
                cell.dimmed().to_string()
            }
        }
        _ => " ".repeat(width),
    }
}

fn render_row(
    row: &OutputRow,
    line_number_width: usize,
    content_width: usize,
    output: &mut String,
) {
    let left_visual_lines = wrap_spans(&row.source_spans, content_width);
    let right_visual_lines = wrap_spans(&row.destination_spans, content_width);

    let num_visual = left_visual_lines.len().max(right_visual_lines.len()).max(1);
    let separator = "│".dimmed();

    for visual_index in 0..num_visual {
        let colored_left_number = number_cell(
            row.source_line_number,
            visual_index,
            row.is_moved,
            line_number_width,
        );
        let colored_right_number = number_cell(
            row.destination_line_number,
            visual_index,
            row.is_moved,
            line_number_width,
        );

        let left_padded = if row.source_line_number.is_none() {
            absent_fill(content_width)
        } else if let Some((ref spans, vis_len)) = left_visual_lines.get(visual_index) {
            let rendered = render_spans(spans);
            let padding = content_width.saturating_sub(*vis_len);
            format!("{}{}", rendered, " ".repeat(padding))
        } else {
            " ".repeat(content_width)
        };

        let right_content = if row.destination_line_number.is_none() {
            absent_fill(content_width)
        } else if let Some((ref spans, _)) = right_visual_lines.get(visual_index) {
            render_spans(spans)
        } else {
            String::new()
        };

        writeln!(
            output,
            "{colored_left_number} {separator} {left_padded} {separator} {colored_right_number} {separator} {right_content}",
        )
        .unwrap();
    }
}

/// Renders a horizontal rule across all four columns. `junction` is the
/// character drawn where the rule crosses a column separator: `┬` for the
/// top border (nothing above it), `┼` between rows, `┴` for the bottom
/// border (nothing below it).
fn render_separator(
    junction: char,
    line_number_width: usize,
    content_width: usize,
    output: &mut String,
) {
    let number_bar = "─".repeat(line_number_width);
    let content_bar = "─".repeat(content_width);
    let raw = format!(
        "{number_bar}─{junction}─{content_bar}─{junction}─{number_bar}─{junction}─{content_bar}"
    );
    writeln!(output, "{}", raw.dimmed()).unwrap();
}

pub struct TerminalFormatter;

impl DiffFormatter for TerminalFormatter {
    fn format(input: &FormatInput) -> String {
        format_side_by_side(input)
    }
}

/// Style vocabulary for the file header: bold filename, plain brackets,
/// cyan language name.
#[derive(Debug, Clone, Copy)]
enum HeaderStyle {
    Filename,
    Plain,
    Language,
}

/// Builds one header cell as styled spans: `[Language] filename`.
fn header_spans<'a>(
    filename: Option<&'a str>,
    language_name: Option<&'a str>,
) -> Vec<Span<'a, HeaderStyle>> {
    let mut spans = Vec::new();
    if let Some(language) = language_name {
        spans.push(Span {
            text: "[",
            color: HeaderStyle::Plain,
        });
        spans.push(Span {
            text: language,
            color: HeaderStyle::Language,
        });
        spans.push(Span {
            text: "] ",
            color: HeaderStyle::Plain,
        });
    }
    if let Some(name) = filename {
        spans.push(Span {
            text: name,
            color: HeaderStyle::Filename,
        });
    }
    spans
}

fn render_header_spans(spans: &[Span<HeaderStyle>]) -> String {
    spans
        .iter()
        .map(|span| match span.color {
            HeaderStyle::Filename => span.text.bold().to_string(),
            HeaderStyle::Plain => span.text.to_string(),
            HeaderStyle::Language => span.text.cyan().to_string(),
        })
        .collect()
}

/// Renders the file-info header as regular rows of the side-by-side layout
/// (blank number cells, one filename per side), so the column separators run
/// through it and the junctions of the rules above and below connect cleanly.
/// Long filenames wrap onto further visual lines exactly like code rows do.
///
/// The language tag appears only on the left side (it is always the same for
/// both).  When both sides name the same file (typical in git diffs), the
/// filename is shown only on the left; the right side shows just its ref.
fn render_file_header(
    input: &FormatInput,
    line_number_width: usize,
    content_width: usize,
    output: &mut String,
) {
    let separator = "│".dimmed();
    let number_blank = " ".repeat(line_number_width);

    let same_name = matches!(
        (input.source_filename, input.destination_filename),
        (Some(a), Some(b)) if a == b
    );

    let left_spans = header_spans(input.source_filename, input.language_name);
    let right_spans = if same_name {
        Vec::new()
    } else {
        header_spans(input.destination_filename, None)
    };

    let left_lines = wrap_spans(&left_spans, content_width);
    let right_lines = wrap_spans(&right_spans, content_width);
    let num_visual = left_lines.len().max(right_lines.len());
    for visual_index in 0..num_visual {
        let left = left_lines.get(visual_index).map_or_else(
            || " ".repeat(content_width),
            |(spans, vis_len)| {
                let padding = content_width.saturating_sub(*vis_len);
                format!("{}{}", render_header_spans(spans), " ".repeat(padding))
            },
        );
        let right = right_lines
            .get(visual_index)
            .map_or_else(String::new, |(spans, _)| render_header_spans(spans));
        writeln!(
            output,
            "{number_blank} {separator} {left} {separator} {number_blank} {separator} {right}",
        )
        .unwrap();
    }
}

/// Re-export for backward compatibility.
pub type SideBySideInput<'a> = FormatInput<'a>;

#[must_use]
pub fn format_side_by_side(input: &FormatInput) -> String {
    let source_tree = &input.result.src_tree;
    let destination_tree = &input.result.dst_tree;
    let mapping = &input.result.mapping;
    let actions = &input.result.actions;

    let source_text = String::from_utf8_lossy(input.source_bytes);
    let destination_text = String::from_utf8_lossy(input.destination_bytes);
    let source_lines = split_into_lines(&source_text);
    let destination_lines = split_into_lines(&destination_text);

    let pairing = build_line_pairing(
        &source_lines,
        &destination_lines,
        source_tree,
        destination_tree,
        mapping,
    );

    let updated = updated_nodes(actions);
    let source_leaf_colors = classify_leaves(
        source_tree,
        |id| !mapping.has_src(id),
        |id| updated.contains(&id),
        SpanColor::Deleted,
    );
    let destination_leaf_colors = classify_leaves(
        destination_tree,
        |id| !mapping.has_dst(id),
        |id| {
            mapping
                .get_src(id)
                .is_some_and(|source_id| updated.contains(&source_id))
        },
        SpanColor::Inserted,
    );

    let context = DiffContext {
        source_tree,
        destination_tree,
        source_leaf_colors: &source_leaf_colors,
        destination_leaf_colors: &destination_leaf_colors,
    };

    let rows = build_output_rows(
        &source_lines,
        &destination_lines,
        &pairing.dst_to_src,
        &pairing.moved_dst_lines,
        &context,
    );

    let hunks = extract_hunks(&rows, 3);
    let max_line_number = source_lines.len().max(destination_lines.len());
    let line_number_width = format!("{max_line_number}").len().max(4);
    let content_width = {
        // Layout: {line_num} │ {content} │ {line_num} │ {content}
        // Total  = 2*line_number_width + 2*content_width + 9  (three " │ " separators)
        let default = 50;
        input
            .terminal_width
            .map(|terminal_width| {
                let chrome = 2 * line_number_width + 9;
                if terminal_width > chrome + 2 {
                    (terminal_width - chrome) / 2
                } else {
                    default
                }
            })
            .unwrap_or(default)
    };

    let mut output = String::new();
    let framed = input.source_filename.is_some()
        || input.destination_filename.is_some()
        || input.language_name.is_some();
    if framed {
        let top = if input.first_file { '┬' } else { '┼' };
        render_separator(top, line_number_width, content_width, &mut output);
        render_file_header(input, line_number_width, content_width, &mut output);
        let below = if hunks.is_empty() && input.last_file {
            '┴'
        } else {
            '┼'
        };
        render_separator(below, line_number_width, content_width, &mut output);
    }
    for (hunk_index, &(start, end)) in hunks.iter().enumerate() {
        if hunk_index > 0 {
            render_separator('┼', line_number_width, content_width, &mut output);
        }
        for row in &rows[start..end] {
            render_row(row, line_number_width, content_width, &mut output);
        }
    }
    if framed && input.last_file && !hunks.is_empty() {
        render_separator('┴', line_number_width, content_width, &mut output);
    }
    output
}
