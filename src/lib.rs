//! Structural code differencing powered by tree-sitter.
//!
//! Implements the `SimpleGumTree` matcher (Falleri & Martinez, ICSE 2024) on top
//! of [tree-sitter](https://tree-sitter.github.io) parsers, plus a Chawathe
//! edit-script generator and multiple output formatters.

pub mod actions;
pub mod language;
pub mod languages;
pub mod line_tree;
pub mod lis;
pub mod mapping;
pub mod matcher;
pub mod output;
pub mod string_distance;
pub mod tree;
pub mod ts_convert;

/// Backward-compatible re-export of the old `format` module surface.
pub mod format {
    pub use crate::output::format_node;
    pub use crate::output::json::to_json;
}

/// Backward-compatible re-export of the old `side_by_side` module surface.
pub mod side_by_side {
    pub use crate::output::terminal::{format_side_by_side, SideBySideInput};
    pub use crate::output::FormatInput;
}

use crate::actions::{generate_actions, Action};
use crate::language::LanguageProfile;
use crate::line_tree::build_line_tree;
use crate::mapping::Mapping;
use crate::matcher::line_diff::match_lines;
use crate::matcher::{match_trees, MatchOptions};
use crate::tree::Tree;

/// The full result of diffing two trees: their internal representations, the
/// final node mapping, and the edit script.
pub struct DiffResult {
    pub src_tree: Tree,
    pub dst_tree: Tree,
    pub mapping: Mapping,
    pub actions: Vec<Action>,
}

/// Options forwarded to the matcher and parser.
#[derive(Debug, Clone, Copy)]
pub struct DiffOptions {
    pub match_options: MatchOptions,
    /// Maximum file size in bytes. Files exceeding this are rejected before
    /// parsing. Set to `0` to disable the limit.
    pub max_file_size: u64,
    /// Parser timeout in microseconds. Set to `0` to disable.
    pub parse_timeout_us: u64,
}

/// Default: 100 MB file limit, 60-second parse timeout.
impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            match_options: MatchOptions::default(),
            max_file_size: 100 * 1024 * 1024,
            parse_timeout_us: 60_000_000,
        }
    }
}

/// Rejects a source buffer that exceeds `max_file_size` (0 disables the limit).
fn check_file_size(label: &str, len: usize, max_file_size: u64) -> Result<(), String> {
    if max_file_size > 0 && len as u64 > max_file_size {
        return Err(format!(
            "{label} source exceeds max file size ({len} bytes > {max_file_size} bytes)"
        ));
    }
    Ok(())
}

/// Diffs two already-built internal trees.
#[must_use]
pub fn diff_trees(source: Tree, destination: Tree, options: &DiffOptions) -> DiffResult {
    let mapping = match_trees(&source, &destination, options.match_options);
    let actions = generate_actions(&source, &destination, &mapping);
    DiffResult {
        src_tree: source,
        dst_tree: destination,
        mapping,
        actions,
    }
}

/// Parses two source buffers with the given language profile, then diffs them.
pub fn diff_sources(
    old_source: &[u8],
    new_source: &[u8],
    profile: &dyn LanguageProfile,
    options: &DiffOptions,
) -> Result<DiffResult, String> {
    check_file_size("old", old_source.len(), options.max_file_size)?;
    check_file_size("new", new_source.len(), options.max_file_size)?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&profile.language())
        .map_err(|error| format!("set_language: {error}"))?;

    let old_syntax_tree = parse_with_timeout(&mut parser, old_source, options.parse_timeout_us)
        .ok_or_else(|| "failed to parse old source (timeout or error)".to_string())?;
    let new_syntax_tree = parse_with_timeout(&mut parser, new_source, options.parse_timeout_us)
        .ok_or_else(|| "failed to parse new source (timeout or error)".to_string())?;

    let source = ts_convert::convert(&old_syntax_tree, old_source, profile);
    let destination = ts_convert::convert(&new_syntax_tree, new_source, profile);
    Ok(diff_trees(source, destination, options))
}

/// Parses `source` with an optional timeout. A `timeout_us` of `0` means no limit.
fn parse_with_timeout(
    parser: &mut tree_sitter::Parser,
    source: &[u8],
    timeout_us: u64,
) -> Option<tree_sitter::Tree> {
    use std::ops::ControlFlow;
    use std::time::Instant;

    if timeout_us == 0 {
        return parser.parse(source, None);
    }

    let deadline = Instant::now() + std::time::Duration::from_micros(timeout_us);
    let len = source.len();
    let mut callback = |_: &tree_sitter::ParseState| {
        if Instant::now() >= deadline {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let mut opts = tree_sitter::ParseOptions::new().progress_callback(&mut callback);
    parser.parse_with_options(
        &mut |byte_offset, _| {
            if byte_offset < len {
                &source[byte_offset..]
            } else {
                &[]
            }
        },
        None,
        Some(opts.reborrow()),
    )
}

/// Line-based diff for files with no recognized grammar.
///
/// Builds flat `file → line*` trees from both inputs and uses LCS-based
/// matching. The result uses the same action vocabulary as the AST-level diff,
/// so callers (including the JSON formatter) need no special handling.
pub fn diff_lines(
    old_source: &[u8],
    new_source: &[u8],
    options: &DiffOptions,
) -> Result<DiffResult, String> {
    check_file_size("old", old_source.len(), options.max_file_size)?;
    check_file_size("new", new_source.len(), options.max_file_size)?;

    let source_tree = build_line_tree(old_source);
    let destination_tree = build_line_tree(new_source);
    let mapping = match_lines(&source_tree, &destination_tree);
    let actions = generate_actions(&source_tree, &destination_tree, &mapping);
    Ok(DiffResult {
        src_tree: source_tree,
        dst_tree: destination_tree,
        mapping,
        actions,
    })
}
