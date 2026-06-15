//! JSON output format for diff results.

use serde::Serialize;

use crate::actions::Action;
use crate::mapping::Mapping;
use crate::tree::Tree;

use super::{format_node, DiffFormatter, FormatInput};

#[derive(Serialize)]
struct DiffOutput {
    matches: Vec<MatchEntry>,
    actions: Vec<ActionEntry>,
}

#[derive(Serialize)]
struct MatchEntry {
    src: String,
    dest: String,
}

#[derive(Serialize)]
struct ActionEntry {
    action: &'static str,
    tree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

pub struct JsonFormatter;

impl DiffFormatter for JsonFormatter {
    fn format(input: &FormatInput) -> String {
        to_json(
            &input.result.src_tree,
            &input.result.dst_tree,
            &input.result.mapping,
            &input.result.actions,
        )
    }
}

/// Serialises the full diff result to a JSON string.
/// output: `{"matches": [...], "actions": [...]}`.
#[must_use]
pub fn to_json(
    source_tree: &Tree,
    destination_tree: &Tree,
    mapping: &Mapping,
    actions: &[Action],
) -> String {
    let matches: Vec<MatchEntry> = mapping
        .pairs()
        .iter()
        .map(|(source, destination)| MatchEntry {
            src: format_node(source_tree.node(*source)),
            dest: format_node(destination_tree.node(*destination)),
        })
        .collect();

    let action_entries: Vec<ActionEntry> = actions
        .iter()
        .map(|action| format_action(source_tree, destination_tree, action))
        .collect();

    let output = DiffOutput {
        matches,
        actions: action_entries,
    };

    serde_json::to_string_pretty(&output).expect("serialization cannot fail for string data")
}

fn format_action(source_tree: &Tree, destination_tree: &Tree, action: &Action) -> ActionEntry {
    let (tree, parent, at, label) = match action {
        // Inserts name destination nodes; everything else names source nodes.
        Action::InsertTree {
            node,
            parent,
            position,
        }
        | Action::InsertNode {
            node,
            parent,
            position,
        } => (
            format_node(destination_tree.node(*node)),
            Some(format_node(destination_tree.node(*parent))),
            Some(*position),
            None,
        ),
        Action::DeleteTree { node } | Action::DeleteNode { node } => {
            (format_node(source_tree.node(*node)), None, None, None)
        }
        Action::Update { node, new_label } => (
            format_node(source_tree.node(*node)),
            None,
            None,
            Some(new_label.clone()),
        ),
        Action::MoveTree {
            node,
            parent,
            position,
        } => (
            format_node(source_tree.node(*node)),
            Some(format_node(destination_tree.node(*parent))),
            Some(*position),
            None,
        ),
    };
    ActionEntry {
        action: action.action_str(),
        tree,
        parent,
        at,
        label,
    }
}
