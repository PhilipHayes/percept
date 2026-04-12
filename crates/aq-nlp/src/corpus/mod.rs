pub mod analysis;
pub mod coref;
pub mod merge;
pub mod renumber;

pub use merge::{merge_trees, CorpusMetadata};
pub use renumber::{renumber_global, LineMapping};

use aq_core::OwnedNode;
use std::collections::HashMap;

/// High-level Phase 2 pipeline: merge per-file trees, renumber, analyse, and
/// graft narrative nodes (arcs, conflicts, summary) into the result.
///
/// Returns `(merged_tree, metadata)`.
pub fn build_corpus(file_trees: Vec<(OwnedNode, String)>) -> (OwnedNode, CorpusMetadata) {
    let file_line_counts: Vec<(String, usize)> = file_trees
        .iter()
        .map(|(tree, path)| (path.clone(), tree.end_line))
        .collect();

    let (mut tree, meta) = merge_trees(file_trees);
    renumber_global(&mut tree, &file_line_counts);

    // Cross-file scene detection + arcs/conflicts/summary.
    let (_scenes, arcs, conflicts, summary) = analysis::compute_corpus_narrative(&tree, &[]);

    // Graft narrative analysis into the tree.
    graft_narrative_nodes(&mut tree, &arcs, &conflicts, &summary);

    (tree, meta)
}

/// Graft arc, conflict, and narrative_summary nodes into a merged tree.
fn graft_narrative_nodes(
    tree: &mut OwnedNode,
    arcs: &[crate::narrative::CharacterArc],
    conflicts: &[crate::narrative::ConflictEdge],
    summary: &crate::narrative::NarrativeSummary,
) {
    // Arc nodes.
    let arc_start = tree.children.len();
    for arc in arcs {
        let mut fi = HashMap::new();
        let shape_node = OwnedNode {
            node_type: "arc_shape".into(),
            text: Some(arc.arc_shape.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 0,
            end_line: 0,
            start_byte: 0,
            end_byte: 0,
            source_file: None,
        };
        fi.insert("shape".to_string(), vec![0]);
        tree.children.push(OwnedNode {
            node_type: "arc".into(),
            text: Some(arc.entity_name.clone()),
            subtree_text: None,
            field_indices: fi,
            children: vec![shape_node],
            start_line: 0,
            end_line: 0,
            start_byte: 0,
            end_byte: 0,
            source_file: None,
        });
    }
    if !arcs.is_empty() {
        tree.field_indices.insert(
            "arcs".to_string(),
            (arc_start..arc_start + arcs.len()).collect(),
        );
    }

    // Conflict nodes.
    let conflict_start = tree.children.len();
    for edge in conflicts {
        tree.children.push(OwnedNode {
            node_type: "conflict".into(),
            text: Some(format!("{} vs {}", edge.entity_a, edge.entity_b)),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 0,
            end_line: 0,
            start_byte: 0,
            end_byte: 0,
            source_file: None,
        });
    }
    if !conflicts.is_empty() {
        tree.field_indices.insert(
            "conflicts".to_string(),
            (conflict_start..conflict_start + conflicts.len()).collect(),
        );
    }

    // Narrative summary node.
    let central_conflict_text = match &summary.central_conflict {
        Some((a, b)) => format!("{} \u{2194} {}", a, b),
        None => "none".to_string(),
    };
    let central_child = OwnedNode {
        node_type: "central_conflict".into(),
        text: Some(central_conflict_text),
        subtree_text: None,
        field_indices: HashMap::new(),
        children: vec![],
        start_line: 0,
        end_line: 0,
        start_byte: 0,
        end_byte: 0,
        source_file: None,
    };
    let mut ns_fi = HashMap::new();
    ns_fi.insert("central_conflict".to_string(), vec![0]);
    let summary_idx = tree.children.len();
    tree.children.push(OwnedNode {
        node_type: "narrative_summary".into(),
        text: Some(format!(
            "scenes={} characters={} conflicts={}",
            summary.scene_count, summary.character_count, summary.conflict_count,
        )),
        subtree_text: None,
        field_indices: ns_fi,
        children: vec![central_child],
        start_line: 0,
        end_line: 0,
        start_byte: 0,
        end_byte: 0,
        source_file: None,
    });
    tree.field_indices
        .insert("narrative_summary".to_string(), vec![summary_idx]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_doc(text: &str, source: &str, end_line: usize) -> OwnedNode {
        let para = OwnedNode {
            node_type: "paragraph".into(),
            text: Some(text.into()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 1,
            end_line,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.into()),
        };
        let mut fi = HashMap::new();
        fi.insert("paragraphs".to_string(), vec![0]);
        OwnedNode {
            node_type: "document".into(),
            text: None,
            subtree_text: None,
            field_indices: fi,
            children: vec![para],
            start_line: 1,
            end_line,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.into()),
        }
    }

    #[test]
    fn test_build_corpus_grafts_summary() {
        let trees = vec![
            (
                simple_doc("First paragraph.", "a.txt", 5),
                "a.txt".to_string(),
            ),
            (
                simple_doc("Second paragraph.", "b.txt", 5),
                "b.txt".to_string(),
            ),
        ];
        let (tree, meta) = build_corpus(trees);

        assert_eq!(meta.files.len(), 2);
        // The merged tree should have a narrative_summary field.
        assert!(
            tree.field_indices.contains_key("narrative_summary"),
            "Merged tree should have narrative_summary"
        );
        let summary_idx = tree.field_indices["narrative_summary"][0];
        let summary_node = &tree.children[summary_idx];
        assert_eq!(summary_node.node_type, "narrative_summary");
        assert!(summary_node.text.is_some());
    }
}
