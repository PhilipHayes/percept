use aq_core::OwnedNode;
use std::collections::HashMap;

/// Metadata about the merged corpus: which files were merged and where their
/// paragraphs live in the unified tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CorpusMetadata {
    /// Ordered list of source file paths.
    pub files: Vec<String>,
    /// For each file, the `(start_para_idx, end_para_idx)` range (inclusive)
    /// into the merged tree's paragraphs.  If a file contributed zero
    /// paragraphs the range is `(start, start - 1)` (empty).
    pub file_boundaries: Vec<(usize, usize)>,
}

/// Merge multiple per-file `OwnedNode` document trees into a single unified
/// tree.  The caller supplies `(tree, filename)` pairs in the desired order.
///
/// The merged tree's children are laid out as:
///   paragraphs | entities | interactions | discourse | scenes | arcs | conflicts | narrative_issues | narrative_summary
/// — the same order as a single-file document tree (see `tree.rs`).
///
/// Entity deduplication: entities with the same lowercased name are merged
/// into one node, combining their locations and keeping the first occurrence's
/// metadata.
pub fn merge_trees(trees: Vec<(OwnedNode, String)>) -> (OwnedNode, CorpusMetadata) {
    let mut files = Vec::new();
    let mut file_boundaries = Vec::new();
    let mut all_paragraphs: Vec<OwnedNode> = Vec::new();
    let mut entity_map: EntityAccumulator = EntityAccumulator::new();
    let mut all_interactions: Vec<OwnedNode> = Vec::new();
    let mut all_discourse: Vec<OwnedNode> = Vec::new();
    let mut all_scenes: Vec<OwnedNode> = Vec::new();
    let mut all_arcs: Vec<OwnedNode> = Vec::new();
    let mut all_conflicts: Vec<OwnedNode> = Vec::new();
    let mut all_narrative_issues: Vec<OwnedNode> = Vec::new();

    let mut max_end_line: usize = 0;

    for (doc, filename) in trees {
        files.push(filename.clone());

        let para_start = all_paragraphs.len();

        // Extract children by field_indices groups.
        let paragraphs = extract_field(&doc, "paragraphs");
        let entities = extract_field(&doc, "entities");
        let interactions = extract_field(&doc, "interactions");
        let discourse = extract_field(&doc, "discourse");
        let scenes = extract_field(&doc, "scenes");
        let arcs = extract_field(&doc, "arcs");
        let conflicts = extract_field(&doc, "conflicts");
        let narrative_issues = extract_field(&doc, "narrative_issues");

        // Tag paragraphs with source_file if not already set.
        for mut para in paragraphs {
            if para.source_file.is_none() {
                para.source_file = Some(filename.clone());
            }
            all_paragraphs.push(para);
        }

        let para_end = if all_paragraphs.len() > para_start {
            all_paragraphs.len() - 1
        } else {
            para_start.wrapping_sub(1) // empty sentinel
        };
        file_boundaries.push((para_start, para_end));

        // Accumulate entities for dedup.
        for entity in entities {
            entity_map.add(entity);
        }

        all_interactions.extend(interactions);
        all_discourse.extend(discourse);
        all_scenes.extend(scenes);
        all_arcs.extend(arcs);
        all_conflicts.extend(conflicts);
        all_narrative_issues.extend(narrative_issues);

        if doc.end_line > max_end_line {
            max_end_line = doc.end_line;
        }
    }

    let merged_entities = entity_map.finish();

    // Build children vec and field_indices in canonical order.
    let num_paras = all_paragraphs.len();
    let num_entities = merged_entities.len();
    let num_interactions = all_interactions.len();
    let num_discourse = all_discourse.len();
    let num_scenes = all_scenes.len();
    let num_arcs = all_arcs.len();
    let num_conflicts = all_conflicts.len();
    let num_narrative_issues = all_narrative_issues.len();

    let mut children = Vec::new();
    children.extend(all_paragraphs);
    children.extend(merged_entities);
    children.extend(all_interactions);
    children.extend(all_discourse);
    children.extend(all_scenes);
    children.extend(all_arcs);
    children.extend(all_conflicts);
    children.extend(all_narrative_issues);

    let mut field_indices = HashMap::new();
    let mut offset = 0usize;

    if num_paras > 0 {
        field_indices.insert(
            "paragraphs".to_string(),
            (offset..offset + num_paras).collect(),
        );
    }
    offset += num_paras;

    if num_entities > 0 {
        field_indices.insert(
            "entities".to_string(),
            (offset..offset + num_entities).collect(),
        );
    }
    offset += num_entities;

    if num_interactions > 0 {
        field_indices.insert(
            "interactions".to_string(),
            (offset..offset + num_interactions).collect(),
        );
    }
    offset += num_interactions;

    if num_discourse > 0 {
        field_indices.insert(
            "discourse".to_string(),
            (offset..offset + num_discourse).collect(),
        );
    }
    offset += num_discourse;

    if num_scenes > 0 {
        field_indices.insert(
            "scenes".to_string(),
            (offset..offset + num_scenes).collect(),
        );
    }
    offset += num_scenes;

    if num_arcs > 0 {
        field_indices.insert("arcs".to_string(), (offset..offset + num_arcs).collect());
    }
    offset += num_arcs;

    if num_conflicts > 0 {
        field_indices.insert(
            "conflicts".to_string(),
            (offset..offset + num_conflicts).collect(),
        );
    }
    offset += num_conflicts;

    if num_narrative_issues > 0 {
        field_indices.insert(
            "narrative_issues".to_string(),
            (offset..offset + num_narrative_issues).collect(),
        );
    }

    let doc = OwnedNode {
        node_type: "document".to_string(),
        text: None,
        subtree_text: None,
        field_indices,
        children,
        start_line: if max_end_line > 0 { 1 } else { 0 },
        end_line: max_end_line,
        start_byte: 0,
        end_byte: 0,
        source_file: None,
    };

    let metadata = CorpusMetadata {
        files,
        file_boundaries,
    };

    (doc, metadata)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract child nodes belonging to a named field from a document node.
fn extract_field(doc: &OwnedNode, field: &str) -> Vec<OwnedNode> {
    match doc.field_indices.get(field) {
        Some(indices) => indices
            .iter()
            .filter_map(|&i| doc.children.get(i).cloned())
            .collect(),
        None => Vec::new(),
    }
}

/// Accumulates entity nodes and deduplicates by lowercase name.
struct EntityAccumulator {
    /// key = lowercase entity name, value = merged entity node.
    map: HashMap<String, OwnedNode>,
    /// Insertion order for deterministic output.
    order: Vec<String>,
}

impl EntityAccumulator {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn add(&mut self, entity: OwnedNode) {
        let name_lower = entity.text.as_deref().unwrap_or("").to_lowercase();

        if let Some(existing) = self.map.get_mut(&name_lower) {
            // Merge: combine locations from the incoming entity into the existing one.
            merge_entity_into(existing, &entity);
        } else {
            self.order.push(name_lower.clone());
            self.map.insert(name_lower, entity);
        }
    }

    fn finish(self) -> Vec<OwnedNode> {
        self.order
            .into_iter()
            .filter_map(|k| self.map.get(&k).cloned())
            .collect()
    }
}

/// Merge `incoming` entity's location children into `target`.
fn merge_entity_into(target: &mut OwnedNode, incoming: &OwnedNode) {
    // Collect location nodes from incoming.
    let incoming_locations: Vec<OwnedNode> = match incoming.field_indices.get("locations") {
        Some(indices) => indices
            .iter()
            .filter_map(|&i| incoming.children.get(i).cloned())
            .collect(),
        None => Vec::new(),
    };

    if incoming_locations.is_empty() {
        return;
    }

    // Append location nodes to target's children.
    let mut new_location_indices: Vec<usize> = target
        .field_indices
        .get("locations")
        .cloned()
        .unwrap_or_default();

    for loc in incoming_locations {
        new_location_indices.push(target.children.len());
        target.children.push(loc);
    }

    target
        .field_indices
        .insert("locations".to_string(), new_location_indices);

    // Update end_line to cover the merged range.
    if incoming.end_line > target.end_line {
        target.end_line = incoming.end_line;
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aq_core::OwnedNode;
    use std::collections::HashMap;

    /// Build a minimal document node with the given paragraphs (and optional
    /// other field groups).
    fn doc_with_paragraphs(paras: Vec<OwnedNode>, source_file: Option<&str>) -> OwnedNode {
        let num = paras.len();
        let end_line = paras.last().map(|p| p.end_line).unwrap_or(0);
        let mut fi = HashMap::new();
        if num > 0 {
            fi.insert("paragraphs".to_string(), (0..num).collect());
        }
        OwnedNode {
            node_type: "document".to_string(),
            text: None,
            subtree_text: None,
            field_indices: fi,
            children: paras,
            start_line: 1,
            end_line,
            start_byte: 0,
            end_byte: 0,
            source_file: source_file.map(|s| s.to_string()),
        }
    }

    fn make_paragraph(text: &str, start: usize, end: usize, source: Option<&str>) -> OwnedNode {
        OwnedNode {
            node_type: "paragraph".to_string(),
            text: Some(text.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: end,
            start_byte: 0,
            end_byte: 0,
            source_file: source.map(|s| s.to_string()),
        }
    }

    fn make_entity(name: &str, line: usize, source: Option<&str>) -> OwnedNode {
        let type_node = OwnedNode {
            node_type: "entity_type".to_string(),
            text: Some("PERSON".to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: line,
            end_line: line,
            start_byte: 0,
            end_byte: 0,
            source_file: source.map(|s| s.to_string()),
        };
        let loc_node = OwnedNode {
            node_type: "location".to_string(),
            text: Some(format!("{}:0", line)),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: line,
            end_line: line,
            start_byte: 0,
            end_byte: 0,
            source_file: source.map(|s| s.to_string()),
        };
        let mut fi = HashMap::new();
        fi.insert("type".to_string(), vec![0]);
        fi.insert("locations".to_string(), vec![1]);
        OwnedNode {
            node_type: "entity".to_string(),
            text: Some(name.to_string()),
            subtree_text: None,
            field_indices: fi,
            children: vec![type_node, loc_node],
            start_line: line,
            end_line: line,
            start_byte: 0,
            end_byte: 0,
            source_file: source.map(|s| s.to_string()),
        }
    }

    fn make_interaction(text: &str, line: usize, source: Option<&str>) -> OwnedNode {
        OwnedNode {
            node_type: "interaction".to_string(),
            text: Some(text.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: line,
            end_line: line,
            start_byte: 0,
            end_byte: 0,
            source_file: source.map(|s| s.to_string()),
        }
    }

    fn doc_with_all(
        paras: Vec<OwnedNode>,
        entities: Vec<OwnedNode>,
        interactions: Vec<OwnedNode>,
        source_file: Option<&str>,
    ) -> OwnedNode {
        let np = paras.len();
        let ne = entities.len();
        let ni = interactions.len();
        let end_line = paras.last().map(|p| p.end_line).unwrap_or(0);

        let mut fi = HashMap::new();
        let mut offset = 0;
        if np > 0 {
            fi.insert("paragraphs".to_string(), (offset..offset + np).collect());
        }
        offset += np;
        if ne > 0 {
            fi.insert("entities".to_string(), (offset..offset + ne).collect());
        }
        offset += ne;
        if ni > 0 {
            fi.insert("interactions".to_string(), (offset..offset + ni).collect());
        }

        let mut children = Vec::new();
        children.extend(paras);
        children.extend(entities);
        children.extend(interactions);

        OwnedNode {
            node_type: "document".to_string(),
            text: None,
            subtree_text: None,
            field_indices: fi,
            children,
            start_line: 1,
            end_line,
            start_byte: 0,
            end_byte: 0,
            source_file: source_file.map(|s| s.to_string()),
        }
    }

    // ── Test: merge two empty documents ──────────────────────────────────────

    #[test]
    fn test_merge_two_empty_documents() {
        let doc1 = doc_with_paragraphs(vec![], Some("a.txt"));
        let doc2 = doc_with_paragraphs(vec![], Some("b.txt"));

        let (merged, meta) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        assert_eq!(merged.node_type, "document");
        assert!(merged.children.is_empty());
        assert_eq!(meta.files, vec!["a.txt", "b.txt"]);
    }

    // ── Test: merge two single-paragraph docs ────────────────────────────────

    #[test]
    fn test_merge_two_single_paragraph_docs() {
        let doc1 = doc_with_paragraphs(
            vec![make_paragraph("Para 1", 1, 3, Some("a.txt"))],
            Some("a.txt"),
        );
        let doc2 = doc_with_paragraphs(
            vec![make_paragraph("Para 2", 1, 2, Some("b.txt"))],
            Some("b.txt"),
        );

        let (merged, meta) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        let para_indices = merged.field_indices.get("paragraphs").unwrap();
        assert_eq!(para_indices.len(), 2);
        assert_eq!(
            merged.children[para_indices[0]].text.as_deref(),
            Some("Para 1")
        );
        assert_eq!(
            merged.children[para_indices[1]].text.as_deref(),
            Some("Para 2")
        );
        assert_eq!(meta.file_boundaries, vec![(0, 0), (1, 1)]);
    }

    // ── Test: paragraphs retain source_file ──────────────────────────────────

    #[test]
    fn test_merge_preserves_source_file() {
        let doc1 = doc_with_paragraphs(
            vec![make_paragraph("A", 1, 1, Some("file1.md"))],
            Some("file1.md"),
        );
        let doc2 = doc_with_paragraphs(
            vec![make_paragraph("B", 1, 1, None)], // source_file not set
            Some("file2.md"),
        );

        let (merged, _) = merge_trees(vec![
            (doc1, "file1.md".to_string()),
            (doc2, "file2.md".to_string()),
        ]);

        let paras = merged.field_indices.get("paragraphs").unwrap();
        assert_eq!(
            merged.children[paras[0]].source_file.as_deref(),
            Some("file1.md")
        );
        // Paragraph without source_file gets tagged with the filename from the pair.
        assert_eq!(
            merged.children[paras[1]].source_file.as_deref(),
            Some("file2.md")
        );
    }

    // ── Test: entity deduplication ───────────────────────────────────────────

    #[test]
    fn test_merge_entity_deduplication() {
        let doc1 = doc_with_all(
            vec![make_paragraph("P1", 1, 2, Some("a.txt"))],
            vec![make_entity("Joseph", 1, Some("a.txt"))],
            vec![],
            Some("a.txt"),
        );
        let doc2 = doc_with_all(
            vec![make_paragraph("P2", 1, 2, Some("b.txt"))],
            vec![make_entity("Joseph", 5, Some("b.txt"))],
            vec![],
            Some("b.txt"),
        );

        let (merged, _) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        let ent_indices = merged.field_indices.get("entities").unwrap();
        // Only one merged entity for "Joseph".
        assert_eq!(ent_indices.len(), 1);
        let joseph = &merged.children[ent_indices[0]];
        assert_eq!(joseph.text.as_deref(), Some("Joseph"));
        // Should have locations from both files.
        let loc_indices = joseph.field_indices.get("locations").unwrap();
        assert_eq!(loc_indices.len(), 2);
    }

    // ── Test: unique entities preserved ──────────────────────────────────────

    #[test]
    fn test_merge_unique_entities() {
        let doc1 = doc_with_all(
            vec![make_paragraph("P1", 1, 1, Some("a.txt"))],
            vec![make_entity("Joseph", 1, Some("a.txt"))],
            vec![],
            Some("a.txt"),
        );
        let doc2 = doc_with_all(
            vec![make_paragraph("P2", 1, 1, Some("b.txt"))],
            vec![make_entity("Reuben", 1, Some("b.txt"))],
            vec![],
            Some("b.txt"),
        );

        let (merged, _) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        let ent_indices = merged.field_indices.get("entities").unwrap();
        assert_eq!(ent_indices.len(), 2);
        let names: Vec<_> = ent_indices
            .iter()
            .map(|&i| merged.children[i].text.as_deref().unwrap())
            .collect();
        assert!(names.contains(&"Joseph"));
        assert!(names.contains(&"Reuben"));
    }

    // ── Test: interactions concatenated ──────────────────────────────────────

    #[test]
    fn test_merge_interactions_concatenated() {
        let doc1 = doc_with_all(
            vec![],
            vec![],
            vec![make_interaction("sold", 1, Some("a.txt"))],
            Some("a.txt"),
        );
        let doc2 = doc_with_all(
            vec![],
            vec![],
            vec![
                make_interaction("spoke", 1, Some("b.txt")),
                make_interaction("wept", 2, Some("b.txt")),
            ],
            Some("b.txt"),
        );

        let (merged, _) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        let int_indices = merged.field_indices.get("interactions").unwrap();
        assert_eq!(int_indices.len(), 3);
        let texts: Vec<_> = int_indices
            .iter()
            .map(|&i| merged.children[i].text.as_deref().unwrap())
            .collect();
        assert_eq!(texts, vec!["sold", "spoke", "wept"]);
    }

    // ── Test: field_indices correct ──────────────────────────────────────────

    #[test]
    fn test_merge_field_indices_correct() {
        let doc1 = doc_with_all(
            vec![make_paragraph("P1", 1, 2, Some("a.txt"))],
            vec![make_entity("Joseph", 1, Some("a.txt"))],
            vec![make_interaction("sold", 1, Some("a.txt"))],
            Some("a.txt"),
        );
        let doc2 = doc_with_all(
            vec![make_paragraph("P2", 1, 2, Some("b.txt"))],
            vec![make_entity("Reuben", 1, Some("b.txt"))],
            vec![make_interaction("wept", 1, Some("b.txt"))],
            Some("b.txt"),
        );

        let (merged, _) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        // 2 paragraphs + 2 entities + 2 interactions = 6 children
        assert_eq!(merged.children.len(), 6);

        let para_idx = merged.field_indices.get("paragraphs").unwrap();
        let ent_idx = merged.field_indices.get("entities").unwrap();
        let int_idx = merged.field_indices.get("interactions").unwrap();

        // paragraphs at 0,1
        assert_eq!(para_idx, &vec![0, 1]);
        // entities at 2,3
        assert_eq!(ent_idx, &vec![2, 3]);
        // interactions at 4,5
        assert_eq!(int_idx, &vec![4, 5]);

        // Verify types at those indices
        assert_eq!(merged.children[0].node_type, "paragraph");
        assert_eq!(merged.children[1].node_type, "paragraph");
        assert_eq!(merged.children[2].node_type, "entity");
        assert_eq!(merged.children[3].node_type, "entity");
        assert_eq!(merged.children[4].node_type, "interaction");
        assert_eq!(merged.children[5].node_type, "interaction");
    }

    // ── Test: preserves all node types ───────────────────────────────────────

    #[test]
    fn test_merge_preserves_all_node_types() {
        // Build a document with discourse and scenes too.
        let mut doc1 = doc_with_all(
            vec![make_paragraph("P1", 1, 2, Some("a.txt"))],
            vec![],
            vec![],
            Some("a.txt"),
        );
        let discourse_node = OwnedNode {
            node_type: "discourse_relation".to_string(),
            text: Some("elaboration".to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 0,
            source_file: Some("a.txt".to_string()),
        };
        let scene_node = OwnedNode {
            node_type: "scene".to_string(),
            text: Some("Scene 1".to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 0,
            source_file: Some("a.txt".to_string()),
        };
        let di = doc1.children.len();
        doc1.children.push(discourse_node);
        doc1.field_indices.insert("discourse".to_string(), vec![di]);
        let si = doc1.children.len();
        doc1.children.push(scene_node);
        doc1.field_indices.insert("scenes".to_string(), vec![si]);

        let doc2 = doc_with_paragraphs(
            vec![make_paragraph("P2", 1, 1, Some("b.txt"))],
            Some("b.txt"),
        );

        let (merged, _) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
        ]);

        assert!(merged.field_indices.contains_key("paragraphs"));
        assert!(merged.field_indices.contains_key("discourse"));
        assert!(merged.field_indices.contains_key("scenes"));
    }

    // ── Test: corpus metadata boundaries ─────────────────────────────────────

    #[test]
    fn test_corpus_metadata_boundaries() {
        let doc1 = doc_with_paragraphs(
            vec![
                make_paragraph("P1a", 1, 3, Some("a.txt")),
                make_paragraph("P1b", 4, 6, Some("a.txt")),
            ],
            Some("a.txt"),
        );
        let doc2 = doc_with_paragraphs(
            vec![make_paragraph("P2a", 1, 2, Some("b.txt"))],
            Some("b.txt"),
        );
        let doc3 = doc_with_paragraphs(
            vec![
                make_paragraph("P3a", 1, 1, Some("c.txt")),
                make_paragraph("P3b", 2, 3, Some("c.txt")),
                make_paragraph("P3c", 4, 5, Some("c.txt")),
            ],
            Some("c.txt"),
        );

        let (merged, meta) = merge_trees(vec![
            (doc1, "a.txt".to_string()),
            (doc2, "b.txt".to_string()),
            (doc3, "c.txt".to_string()),
        ]);

        assert_eq!(meta.files, vec!["a.txt", "b.txt", "c.txt"]);
        // a.txt: paras 0,1  b.txt: para 2  c.txt: paras 3,4,5
        assert_eq!(meta.file_boundaries, vec![(0, 1), (2, 2), (3, 5)]);

        let para_count = merged.field_indices.get("paragraphs").unwrap().len();
        assert_eq!(para_count, 6);
    }
}
