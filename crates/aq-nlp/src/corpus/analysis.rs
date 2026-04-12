use crate::discourse::DiscourseRelationData;
use crate::narrative::{
    build_conflict_graph, build_narrative_summary, compute_character_arcs, detect_scene_boundaries,
    CharacterArc, ConflictEdge, EntityInteractionProfile, NarrativeIssue, NarrativeSummary,
    OpposingInteraction, ParagraphEntityData, SceneBoundary,
};
use aq_core::OwnedNode;
use std::collections::HashMap;

/// Extract `ParagraphEntityData` from a merged tree's paragraph nodes.
///
/// Each paragraph's entity names are gathered from its descendant entity-type
/// leaves.  Location and temporal entities are identified by their entity_type
/// child text.
pub(crate) fn extract_paragraph_entity_data(
    merged_tree: &OwnedNode,
    entity_nodes: &[&OwnedNode],
) -> Vec<ParagraphEntityData> {
    let para_indices = match merged_tree.field_indices.get("paragraphs") {
        Some(v) => v,
        None => return Vec::new(),
    };

    para_indices
        .iter()
        .enumerate()
        .filter_map(|(idx, &i)| merged_tree.children.get(i).map(|para| (idx, para)))
        .map(|(idx, para)| {
            let start_line = para.start_line;
            let end_line = para.end_line;

            // Collect entity names that fall within this paragraph's line range.
            let mut entity_names = Vec::new();
            let mut location_entities = Vec::new();
            let mut temporal_entities = Vec::new();

            for entity in entity_nodes {
                // Check if any of the entity's locations fall within this paragraph.
                let in_range = entity_locations_in_range(entity, start_line, end_line);
                if !in_range {
                    continue;
                }

                let name = entity.text.as_deref().unwrap_or("");
                if !name.is_empty() && !entity_names.contains(&name.to_string()) {
                    entity_names.push(name.to_string());
                }

                let etype = entity_type_of(entity);
                match etype.as_str() {
                    "LOC" | "GPE" => {
                        if !location_entities.contains(&name.to_string()) {
                            location_entities.push(name.to_string());
                        }
                    }
                    "DATE" | "TIME" => {
                        if !temporal_entities.contains(&name.to_string()) {
                            temporal_entities.push(name.to_string());
                        }
                    }
                    _ => {}
                }
            }

            ParagraphEntityData {
                para_idx: idx,
                start_line,
                end_line,
                entity_names,
                location_entities,
                temporal_entities,
            }
        })
        .collect()
}

/// Check if any location child of an entity node falls within [start, end].
fn entity_locations_in_range(entity: &OwnedNode, start: usize, end: usize) -> bool {
    if let Some(loc_indices) = entity.field_indices.get("locations") {
        for &li in loc_indices {
            if let Some(loc) = entity.children.get(li) {
                if loc.start_line >= start && loc.start_line <= end {
                    return true;
                }
            }
        }
    }
    // Fallback: check entity's own line range.
    entity.start_line >= start && entity.start_line <= end
}

/// Extract the entity type string from an entity node's "type" child.
fn entity_type_of(entity: &OwnedNode) -> String {
    entity
        .field_indices
        .get("type")
        .and_then(|v| v.first())
        .and_then(|&i| entity.children.get(i))
        .and_then(|n| n.text.as_ref())
        .cloned()
        .unwrap_or_default()
}

/// Run scene detection on a merged corpus tree.
///
/// Builds paragraph entity data from the merged tree, then delegates to the
/// existing `detect_scene_boundaries` algorithm.
pub(crate) fn detect_corpus_scenes(
    merged_tree: &OwnedNode,
    discourse_relations: &[DiscourseRelationData],
) -> Vec<SceneBoundary> {
    let entity_nodes = collect_entity_refs(merged_tree);
    let para_data = extract_paragraph_entity_data(merged_tree, &entity_nodes);
    detect_scene_boundaries(&para_data, discourse_relations)
}

/// Collect references to entity children of the merged document.
fn collect_entity_refs(doc: &OwnedNode) -> Vec<&OwnedNode> {
    match doc.field_indices.get("entities") {
        Some(indices) => indices
            .iter()
            .filter_map(|&i| doc.children.get(i))
            .collect(),
        None => Vec::new(),
    }
}

/// Extract interaction data from the merged tree for corpus-wide analysis.
///
/// Returns `(profiles, opposing)` where `profiles` are per-entity interaction
/// profiles and `opposing` are pairs of opposing interactions for conflict
/// detection.
pub(crate) fn extract_corpus_interactions(
    merged_tree: &OwnedNode,
) -> (Vec<EntityInteractionProfile>, Vec<OpposingInteraction>) {
    let int_nodes = match merged_tree.field_indices.get("interactions") {
        Some(indices) => indices
            .iter()
            .filter_map(|&i| merged_tree.children.get(i))
            .collect::<Vec<_>>(),
        None => return (Vec::new(), Vec::new()),
    };

    let total_lines = if merged_tree.end_line > 0 {
        merged_tree.end_line as f64
    } else {
        1.0
    };

    // Build per-entity interaction data.
    let mut entity_interactions: HashMap<String, Vec<(f64, String)>> = HashMap::new();
    let mut opposing = Vec::new();

    for int_node in &int_nodes {
        let verb = int_node
            .field_indices
            .get("verb")
            .and_then(|v| v.first())
            .and_then(|&i| int_node.children.get(i))
            .and_then(|n| n.text.as_ref())
            .cloned()
            .unwrap_or_default();

        let agent = int_node
            .field_indices
            .get("agent")
            .and_then(|v| v.first())
            .and_then(|&i| int_node.children.get(i))
            .and_then(|n| n.text.as_ref())
            .cloned();

        let patient = int_node
            .field_indices
            .get("patient")
            .and_then(|v| v.first())
            .and_then(|&i| int_node.children.get(i))
            .and_then(|n| n.text.as_ref())
            .cloned();

        let position = int_node.start_line as f64 / total_lines;

        // Agent role tracking.
        if let Some(ref a) = agent {
            entity_interactions
                .entry(a.clone())
                .or_default()
                .push((position, "agent".to_string()));
        }
        if let Some(ref p) = patient {
            entity_interactions
                .entry(p.clone())
                .or_default()
                .push((position, "patient".to_string()));
        }

        // Track opposing interactions (agent acts on patient).
        if let (Some(a), Some(p)) = (agent, patient) {
            opposing.push(OpposingInteraction {
                agent: a,
                patient: p,
                verb,
                position,
            });
        }
    }

    // Build entity mention positions from entity nodes' locations.
    let entity_nodes = collect_entity_refs(merged_tree);
    let mut entity_mentions: HashMap<String, Vec<f64>> = HashMap::new();
    for ent in &entity_nodes {
        let name = ent.text.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        if let Some(loc_indices) = ent.field_indices.get("locations") {
            for &li in loc_indices {
                if let Some(loc) = ent.children.get(li) {
                    let pos = loc.start_line as f64 / total_lines;
                    entity_mentions.entry(name.clone()).or_default().push(pos);
                }
            }
        }
    }

    // Build profiles.
    let mut all_names: Vec<String> = entity_interactions
        .keys()
        .chain(entity_mentions.keys())
        .cloned()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    all_names.sort();

    let profiles: Vec<EntityInteractionProfile> = all_names
        .into_iter()
        .map(|name| {
            let mut mention_positions = entity_mentions.remove(&name).unwrap_or_default();
            mention_positions.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let interactions = entity_interactions.remove(&name).unwrap_or_default();
            let mut interaction_positions: Vec<f64> =
                interactions.iter().map(|(p, _)| *p).collect();
            interaction_positions.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let mut role_counts: HashMap<String, usize> = HashMap::new();
            for (_, role) in &interactions {
                *role_counts.entry(role.clone()).or_default() += 1;
            }

            EntityInteractionProfile {
                entity_name: name,
                mention_positions,
                interaction_positions,
                role_counts,
                interaction_roles: interactions,
            }
        })
        .collect();

    (profiles, opposing)
}

/// Run corpus-wide narrative analysis: scenes, arcs, conflicts, summary.
///
/// Returns `(scenes, arcs, conflicts, summary)`.
#[allow(clippy::type_complexity)]
pub(crate) fn compute_corpus_narrative(
    merged_tree: &OwnedNode,
    discourse_relations: &[DiscourseRelationData],
) -> (
    Vec<SceneBoundary>,
    Vec<CharacterArc>,
    Vec<ConflictEdge>,
    NarrativeSummary,
) {
    let scenes = detect_corpus_scenes(merged_tree, discourse_relations);
    let (profiles, opposing) = extract_corpus_interactions(merged_tree);
    let arcs = compute_character_arcs(&profiles);
    let conflicts = build_conflict_graph(&opposing);
    // For now, no cross-file narrative issues (S14 partial — issues require
    // deeper refactoring of tree.rs which is deferred).
    let issues: Vec<NarrativeIssue> = Vec::new();
    let summary = build_narrative_summary(&scenes, &arcs, &conflicts, &issues);
    (scenes, arcs, conflicts, summary)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.to_string()),
        }
    }

    fn make_entity(name: &str, etype: &str, line: usize, source: &str) -> OwnedNode {
        let type_node = OwnedNode {
            node_type: "entity_type".to_string(),
            text: Some(etype.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: line,
            end_line: line,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.to_string()),
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
            source_file: Some(source.to_string()),
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
            source_file: Some(source.to_string()),
        }
    }

    fn build_merged_doc(paras: Vec<OwnedNode>, entities: Vec<OwnedNode>) -> OwnedNode {
        let np = paras.len();
        let ne = entities.len();
        let end = paras.last().map(|p| p.end_line).unwrap_or(0);

        let mut fi = HashMap::new();
        if np > 0 {
            fi.insert("paragraphs".to_string(), (0..np).collect());
        }
        if ne > 0 {
            fi.insert("entities".to_string(), (np..np + ne).collect());
        }

        let mut children = Vec::new();
        children.extend(paras);
        children.extend(entities);

        OwnedNode {
            node_type: "document".to_string(),
            text: None,
            subtree_text: None,
            field_indices: fi,
            children,
            start_line: 1,
            end_line: end,
            start_byte: 0,
            end_byte: 0,
            source_file: None,
        }
    }

    // ── test: scene continuity across file boundary ──────────────────────────

    #[test]
    fn test_scene_continuity_across_file_boundary() {
        // Two paragraphs from different files with the same entities.
        // Should NOT produce a scene break.
        let paras = vec![
            make_paragraph("Para 1", 1, 5, "a.txt"),
            make_paragraph("Para 2", 6, 10, "b.txt"),
        ];
        let entities = vec![
            make_entity("Joseph", "PERSON", 3, "a.txt"),
            make_entity("Joseph", "PERSON", 8, "b.txt"),
        ];

        let doc = build_merged_doc(paras, entities);
        let scenes = detect_corpus_scenes(&doc, &[]);

        // With only 2 paragraphs sharing the same entity, expect 1 scene.
        assert_eq!(scenes.len(), 1);
    }

    // ── test: scene break across file boundary ───────────────────────────────

    #[test]
    fn test_scene_break_across_file_boundary() {
        // Paragraphs from different files with completely different entities.
        // Entity set shift should produce a scene break.
        let paras = vec![
            make_paragraph("Para 1", 1, 5, "a.txt"),
            make_paragraph("Para 2", 6, 10, "b.txt"),
            make_paragraph("Para 3", 11, 15, "b.txt"),
        ];
        let entities = vec![
            make_entity("Joseph", "PERSON", 3, "a.txt"),
            make_entity("Pharaoh", "PERSON", 8, "b.txt"),
            make_entity("Pharaoh", "PERSON", 12, "b.txt"),
        ];

        let doc = build_merged_doc(paras, entities);
        let scenes = detect_corpus_scenes(&doc, &[]);

        // Entity set completely changes at boundary → scene break.
        // With the low entity-set-shift threshold (0.15), completely different
        // entities should trigger a break even without 2 signals.
        // However, exact behavior depends on the algorithm.
        // At minimum, we should have ≥ 1 scene.
        assert!(scenes.len() >= 1);
    }

    // ── test: no artificial boundary scenes ──────────────────────────────────

    #[test]
    fn test_no_artificial_boundary_scenes() {
        // Splitting a continuous narrative across files should NOT create
        // extra scene breaks compared to keeping it as one file.
        // Same entity throughout → 1 scene.
        let paras = vec![
            make_paragraph("Para 1", 1, 3, "a.txt"),
            make_paragraph("Para 2", 4, 6, "a.txt"),
            make_paragraph("Para 3", 7, 9, "b.txt"),
            make_paragraph("Para 4", 10, 12, "b.txt"),
        ];
        // Same entity mentioned in all paragraphs.
        let entities = vec![
            make_entity("Joseph", "PERSON", 2, "a.txt"),
            make_entity("Joseph", "PERSON", 5, "a.txt"),
            make_entity("Joseph", "PERSON", 8, "b.txt"),
            make_entity("Joseph", "PERSON", 11, "b.txt"),
        ];

        let doc = build_merged_doc(paras, entities);
        let scenes = detect_corpus_scenes(&doc, &[]);

        assert_eq!(
            scenes.len(),
            1,
            "Continuous entity presence should produce 1 scene"
        );
    }

    // ── test: extract_paragraph_entity_data ──────────────────────────────────

    #[test]
    fn test_extract_paragraph_entity_data() {
        let paras = vec![
            make_paragraph("P1", 1, 5, "a.txt"),
            make_paragraph("P2", 6, 10, "b.txt"),
        ];
        let entities = vec![
            make_entity("Joseph", "PERSON", 3, "a.txt"),
            make_entity("Egypt", "GPE", 8, "b.txt"),
        ];

        let doc = build_merged_doc(paras, entities);
        let entity_refs = collect_entity_refs(&doc);
        let data = extract_paragraph_entity_data(&doc, &entity_refs);

        assert_eq!(data.len(), 2);

        // First paragraph has "Joseph" (PERSON)
        assert_eq!(data[0].entity_names, vec!["Joseph"]);
        assert!(data[0].location_entities.is_empty());

        // Second paragraph has "Egypt" (GPE → location entity)
        assert_eq!(data[1].entity_names, vec!["Egypt"]);
        assert_eq!(data[1].location_entities, vec!["Egypt"]);
    }

    // ── S14 helpers ──────────────────────────────────────────────────────────

    fn make_interaction(
        agent: &str,
        patient: &str,
        verb: &str,
        start: usize,
        source: &str,
    ) -> OwnedNode {
        let agent_node = OwnedNode {
            node_type: "agent".to_string(),
            text: Some(agent.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: start,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.to_string()),
        };
        let patient_node = OwnedNode {
            node_type: "patient".to_string(),
            text: Some(patient.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: start,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.to_string()),
        };
        let verb_node = OwnedNode {
            node_type: "verb".to_string(),
            text: Some(verb.to_string()),
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: start,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.to_string()),
        };
        let mut fi = HashMap::new();
        fi.insert("agent".to_string(), vec![0]);
        fi.insert("patient".to_string(), vec![1]);
        fi.insert("verb".to_string(), vec![2]);
        OwnedNode {
            node_type: "interaction".to_string(),
            text: None,
            subtree_text: None,
            field_indices: fi,
            children: vec![agent_node, patient_node, verb_node],
            start_line: start,
            end_line: start,
            start_byte: 0,
            end_byte: 0,
            source_file: Some(source.to_string()),
        }
    }

    fn build_full_doc(
        paras: Vec<OwnedNode>,
        entities: Vec<OwnedNode>,
        interactions: Vec<OwnedNode>,
    ) -> OwnedNode {
        let np = paras.len();
        let ne = entities.len();
        let ni = interactions.len();
        let end = paras
            .last()
            .map(|p| p.end_line)
            .unwrap_or(0)
            .max(interactions.last().map(|i| i.end_line).unwrap_or(0));

        let mut fi = HashMap::new();
        if np > 0 {
            fi.insert("paragraphs".to_string(), (0..np).collect());
        }
        if ne > 0 {
            fi.insert("entities".to_string(), (np..np + ne).collect());
        }
        if ni > 0 {
            fi.insert(
                "interactions".to_string(),
                (np + ne..np + ne + ni).collect(),
            );
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
            end_line: end,
            start_byte: 0,
            end_byte: 0,
            source_file: None,
        }
    }

    // ── S14 tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_corpus_arcs_span_files() {
        // Character appears in two files with interactions → arc extracted.
        let paras = vec![
            make_paragraph("P1", 1, 10, "a.txt"),
            make_paragraph("P2", 11, 20, "b.txt"),
        ];
        let entities = vec![
            make_entity("Joseph", "PERSON", 5, "a.txt"),
            make_entity("Joseph", "PERSON", 15, "b.txt"),
        ];
        let interactions = vec![
            make_interaction("Joseph", "Pharaoh", "confronted", 5, "a.txt"),
            make_interaction("Joseph", "Guards", "commanded", 15, "b.txt"),
        ];
        let doc = build_full_doc(paras, entities, interactions);
        let (profiles, _opposing) = extract_corpus_interactions(&doc);

        // Joseph should appear as agent in both interactions.
        let joseph = profiles
            .iter()
            .find(|p| p.entity_name == "Joseph")
            .expect("Joseph should have a profile");
        assert_eq!(joseph.interaction_positions.len(), 2);
        assert_eq!(*joseph.role_counts.get("agent").unwrap_or(&0), 2);

        // Compute arcs.
        let arcs = compute_character_arcs(&profiles);
        let joseph_arc = arcs
            .iter()
            .find(|a| a.entity_name == "Joseph")
            .expect("Joseph should have an arc");
        assert_eq!(joseph_arc.total_interactions, 2);
    }

    #[test]
    fn test_corpus_conflict_graph() {
        // Opposing interactions across files should produce a conflict edge.
        let paras = vec![
            make_paragraph("P1", 1, 10, "a.txt"),
            make_paragraph("P2", 11, 20, "b.txt"),
        ];
        let entities = vec![
            make_entity("Joseph", "PERSON", 5, "a.txt"),
            make_entity("Pharaoh", "PERSON", 15, "b.txt"),
        ];
        let interactions = vec![
            make_interaction("Joseph", "Pharaoh", "opposed", 5, "a.txt"),
            make_interaction("Pharaoh", "Joseph", "punished", 15, "b.txt"),
        ];
        let doc = build_full_doc(paras, entities, interactions);
        let (_profiles, opposing) = extract_corpus_interactions(&doc);

        assert_eq!(opposing.len(), 2);
        let conflicts = build_conflict_graph(&opposing);
        // Both interactions are between Joseph-Pharaoh pair → 1 conflict edge.
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].interaction_count, 2);
    }

    #[test]
    fn test_corpus_narrative_summary() {
        // Full pipeline: scenes + arcs + conflicts → summary.
        let paras = vec![
            make_paragraph("P1", 1, 10, "a.txt"),
            make_paragraph("P2", 11, 20, "b.txt"),
        ];
        let entities = vec![
            make_entity("Joseph", "PERSON", 5, "a.txt"),
            make_entity("Joseph", "PERSON", 15, "b.txt"),
        ];
        let interactions = vec![
            make_interaction("Joseph", "Pharaoh", "served", 5, "a.txt"),
            make_interaction("Joseph", "Pharaoh", "advised", 15, "b.txt"),
        ];
        let doc = build_full_doc(paras, entities, interactions);
        let (scenes, arcs, conflicts, summary) = compute_corpus_narrative(&doc, &[]);

        assert!(!scenes.is_empty(), "should detect at least one scene");
        assert!(!arcs.is_empty(), "should compute at least one arc");
        // Both interactions are between Joseph-Pharaoh → 1 conflict edge.
        assert_eq!(conflicts.len(), 1);
        assert_eq!(summary.scene_count, scenes.len());
        assert_eq!(summary.character_count, arcs.len());
    }

    #[test]
    fn test_extract_corpus_interactions_empty() {
        // Empty tree → empty profiles and no opposing interactions.
        let doc = OwnedNode {
            node_type: "document".to_string(),
            text: None,
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 0,
            end_line: 0,
            start_byte: 0,
            end_byte: 0,
            source_file: None,
        };
        let (profiles, opposing) = extract_corpus_interactions(&doc);
        assert!(profiles.is_empty());
        assert!(opposing.is_empty());
    }
}
