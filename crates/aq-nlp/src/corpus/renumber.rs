use aq_core::OwnedNode;

/// Maps a file name to its cumulative line offset in the merged corpus.
#[derive(Debug, Clone)]
pub struct LineMapping {
    /// Per-file: (filename, line_offset).  File 1 offset = 0, file 2 offset =
    /// total lines in file 1, etc.
    offsets: Vec<(String, usize)>,
}

impl LineMapping {
    /// Build a mapping from the per-file line counts.
    ///
    /// `file_line_counts` is an ordered list of (filename, total_lines_in_file).
    pub fn from_file_line_counts(file_line_counts: &[(String, usize)]) -> Self {
        let mut offsets = Vec::with_capacity(file_line_counts.len());
        let mut cumulative = 0usize;
        for (name, count) in file_line_counts {
            offsets.push((name.clone(), cumulative));
            cumulative += count;
        }
        Self { offsets }
    }

    /// Returns the offset for a given file, or 0 if the file is unknown.
    pub fn offset_for(&self, filename: &str) -> usize {
        self.offsets
            .iter()
            .find(|(n, _)| n == filename)
            .map(|(_, off)| *off)
            .unwrap_or(0)
    }

    /// Reverse-map a global line number to `(source_file, local_line)`.
    ///
    /// Searches backwards through the offset table to find the file whose
    /// offset is <= the global line, then subtracts.
    pub fn reverse(&self, global_line: usize) -> Option<(String, usize)> {
        // Walk offsets in reverse to find the last file whose offset < global_line.
        for (name, offset) in self.offsets.iter().rev() {
            if global_line > *offset {
                return Some((name.clone(), global_line - offset));
            }
        }
        // global_line == 0 or no files
        None
    }
}

/// Renumber all line references in a merged tree to use globally unique,
/// continuous line numbers.
///
/// `file_line_counts` should contain `(filename, total_line_count)` for each
/// file in merge order.  The tree is mutated in-place.
pub fn renumber_global(tree: &mut OwnedNode, file_line_counts: &[(String, usize)]) -> LineMapping {
    let mapping = LineMapping::from_file_line_counts(file_line_counts);
    renumber_node(tree, &mapping);
    mapping
}

/// Recursively renumber a single node and all its descendants.
fn renumber_node(node: &mut OwnedNode, mapping: &LineMapping) {
    let offset = node
        .source_file
        .as_deref()
        .map(|f| mapping.offset_for(f))
        .unwrap_or(0);

    if offset > 0 {
        node.start_line += offset;
        node.end_line += offset;

        // Update text-embedded line references for specific node types.
        update_embedded_lines(node, offset);
    }

    for child in &mut node.children {
        renumber_node(child, mapping);
    }
}

/// Certain node types embed line numbers in their `text` field.
/// - "line" nodes: text = "{line_number}"
/// - "location" nodes: text = "{line}:{char_offset}"
fn update_embedded_lines(node: &mut OwnedNode, offset: usize) {
    match node.node_type.as_str() {
        "line" => {
            if let Some(ref text) = node.text {
                if let Ok(line_num) = text.parse::<usize>() {
                    node.text = Some((line_num + offset).to_string());
                }
            }
        }
        "location" => {
            if let Some(ref text) = node.text {
                if let Some((line_part, rest)) = text.split_once(':') {
                    if let Ok(line_num) = line_part.parse::<usize>() {
                        node.text = Some(format!("{}:{}", line_num + offset, rest));
                    }
                }
            }
        }
        _ => {}
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aq_core::OwnedNode;
    use std::collections::HashMap;

    fn make_paragraph(text: &str, start: usize, end: usize, source: &str) -> OwnedNode {
        OwnedNode {
            node_type: "paragraph".to_string(),
            text: Some(text.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: end,
            source_file: Some(source.to_string()),
        }
    }

    fn make_sentence(text: &str, start: usize, end: usize, source: &str) -> OwnedNode {
        let token = OwnedNode {
            node_type: "token".to_string(),
            text: Some("word".to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: start,
            source_file: Some(source.to_string()),
        };
        let mut fi = HashMap::new();
        fi.insert("tokens".to_string(), vec![0]);
        OwnedNode {
            node_type: "sentence".to_string(),
            text: Some(text.to_string()),
            subtree_text: None,
            field_indices: fi,
            children: vec![token],
            start_line: start,
            end_line: end,
            source_file: Some(source.to_string()),
        }
    }

    fn make_paragraph_with_sentence(
        text: &str,
        start: usize,
        end: usize,
        source: &str,
    ) -> OwnedNode {
        let sent = make_sentence(text, start, end, source);
        let mut fi = HashMap::new();
        fi.insert("sentences".to_string(), vec![0]);
        OwnedNode {
            node_type: "paragraph".to_string(),
            text: Some(text.to_string()),
            subtree_text: None,
            field_indices: fi,
            children: vec![sent],
            start_line: start,
            end_line: end,
            source_file: Some(source.to_string()),
        }
    }

    fn make_entity_with_location(
        name: &str,
        line: usize,
        char_offset: usize,
        source: &str,
    ) -> OwnedNode {
        let loc_node = OwnedNode {
            node_type: "location".to_string(),
            text: Some(format!("{}:{}", line, char_offset)),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: line,
            end_line: line,
            source_file: Some(source.to_string()),
        };
        let mut fi = HashMap::new();
        fi.insert("locations".to_string(), vec![0]);
        OwnedNode {
            node_type: "entity".to_string(),
            text: Some(name.to_string()),
            subtree_text: None,
            field_indices: fi,
            children: vec![loc_node],
            start_line: line,
            end_line: line,
            source_file: Some(source.to_string()),
        }
    }

    fn make_interaction_with_line(verb: &str, source_line: usize, source: &str) -> OwnedNode {
        let line_node = OwnedNode {
            node_type: "line".to_string(),
            text: Some(source_line.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: source_line,
            end_line: source_line,
            source_file: Some(source.to_string()),
        };
        let mut fi = HashMap::new();
        fi.insert("lines".to_string(), vec![0]);
        OwnedNode {
            node_type: "interaction".to_string(),
            text: Some(verb.to_string()),
            subtree_text: None,
            field_indices: fi,
            children: vec![line_node],
            start_line: source_line,
            end_line: source_line,
            source_file: Some(source.to_string()),
        }
    }

    fn build_merged_doc(children: Vec<OwnedNode>) -> OwnedNode {
        let end = children.last().map(|c| c.end_line).unwrap_or(0);
        OwnedNode {
            node_type: "document".to_string(),
            text: None,
            subtree_text: None,
            field_indices: HashMap::new(),
            children,
            start_line: 1,
            end_line: end,
            source_file: None, // merged doc has no single source
        }
    }

    // ── test_renumber_two_files ──────────────────────────────────────────────

    #[test]
    fn test_renumber_two_files() {
        // File a.txt: 10 lines, file b.txt: 15 lines.
        // After renumbering, b.txt paragraph at local line 1 → global line 11.
        let mut doc = build_merged_doc(vec![
            make_paragraph("P1", 1, 10, "a.txt"),
            make_paragraph("P2", 1, 15, "b.txt"),
        ]);

        let mapping = renumber_global(
            &mut doc,
            &[("a.txt".to_string(), 10), ("b.txt".to_string(), 15)],
        );

        // a.txt paragraph: lines stay 1-10 (offset 0)
        assert_eq!(doc.children[0].start_line, 1);
        assert_eq!(doc.children[0].end_line, 10);
        // b.txt paragraph: lines shift by 10 → 11-25
        assert_eq!(doc.children[1].start_line, 11);
        assert_eq!(doc.children[1].end_line, 25);

        assert_eq!(mapping.offset_for("a.txt"), 0);
        assert_eq!(mapping.offset_for("b.txt"), 10);
    }

    // ── test_renumber_recursive ──────────────────────────────────────────────

    #[test]
    fn test_renumber_recursive() {
        // Paragraph with nested sentence+token from b.txt, local lines 1-3.
        // With a.txt having 10 lines, b.txt offset = 10.
        let mut doc = build_merged_doc(vec![
            make_paragraph("P1", 1, 10, "a.txt"),
            make_paragraph_with_sentence("P2", 1, 3, "b.txt"),
        ]);

        renumber_global(
            &mut doc,
            &[("a.txt".to_string(), 10), ("b.txt".to_string(), 5)],
        );

        let p2 = &doc.children[1];
        assert_eq!(p2.start_line, 11);
        assert_eq!(p2.end_line, 13);

        // sentence
        let sent = &p2.children[0];
        assert_eq!(sent.start_line, 11);
        assert_eq!(sent.end_line, 13);

        // token inside sentence
        let token = &sent.children[0];
        assert_eq!(token.start_line, 11);
    }

    // ── test_renumber_preserves_source_file ──────────────────────────────────

    #[test]
    fn test_renumber_preserves_source_file() {
        let mut doc = build_merged_doc(vec![
            make_paragraph("P1", 1, 5, "a.txt"),
            make_paragraph("P2", 1, 5, "b.txt"),
        ]);

        renumber_global(
            &mut doc,
            &[("a.txt".to_string(), 5), ("b.txt".to_string(), 5)],
        );

        assert_eq!(doc.children[0].source_file.as_deref(), Some("a.txt"));
        assert_eq!(doc.children[1].source_file.as_deref(), Some("b.txt"));
    }

    // ── test_line_mapping_reverse ────────────────────────────────────────────

    #[test]
    fn test_line_mapping_reverse() {
        let mapping = LineMapping::from_file_line_counts(&[
            ("a.txt".to_string(), 10),
            ("b.txt".to_string(), 15),
            ("c.txt".to_string(), 5),
        ]);

        // Global line 5 → a.txt line 5
        let (file, local) = mapping.reverse(5).unwrap();
        assert_eq!(file, "a.txt");
        assert_eq!(local, 5);

        // Global line 15 → b.txt line 5 (offset=10, 15-10=5)
        let (file, local) = mapping.reverse(15).unwrap();
        assert_eq!(file, "b.txt");
        assert_eq!(local, 5);

        // Global line 26 → c.txt line 1 (offset=25, 26-25=1)
        let (file, local) = mapping.reverse(26).unwrap();
        assert_eq!(file, "c.txt");
        assert_eq!(local, 1);
    }

    // ── test_renumber_interactions ───────────────────────────────────────────

    #[test]
    fn test_renumber_interactions() {
        // Interaction from b.txt at local line 3. With a.txt having 10 lines.
        let mut doc = build_merged_doc(vec![make_interaction_with_line("sold", 3, "b.txt")]);

        renumber_global(
            &mut doc,
            &[("a.txt".to_string(), 10), ("b.txt".to_string(), 5)],
        );

        let interaction = &doc.children[0];
        assert_eq!(interaction.start_line, 13);
        // The "line" child's text and start_line should also be renumbered.
        let line_node = &interaction.children[0];
        assert_eq!(line_node.text.as_deref(), Some("13"));
        assert_eq!(line_node.start_line, 13);
    }

    // ── test_renumber_entity_lines ───────────────────────────────────────────

    #[test]
    fn test_renumber_entity_lines() {
        // Entity from b.txt at local line 5, char offset 12.
        let mut doc = build_merged_doc(vec![make_entity_with_location("Joseph", 5, 12, "b.txt")]);

        renumber_global(
            &mut doc,
            &[("a.txt".to_string(), 10), ("b.txt".to_string(), 8)],
        );

        let entity = &doc.children[0];
        assert_eq!(entity.start_line, 15); // 5 + 10
                                           // Location text: "5:12" → "15:12"
        let loc = &entity.children[0];
        assert_eq!(loc.text.as_deref(), Some("15:12"));
        assert_eq!(loc.start_line, 15);
    }
}
