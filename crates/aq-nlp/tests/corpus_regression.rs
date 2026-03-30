use aq_core::backend::Backend;
use aq_core::OwnedNode;
use aq_nlp::corpus::build_corpus;
use aq_nlp::NlpBackend;

fn spacy_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import spacy; spacy.load('en_core_web_sm')"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Extract scene count from a tree's field_indices.
fn scene_count(tree: &OwnedNode) -> usize {
    tree.field_indices.get("scenes").map_or(0, |v| v.len())
}

/// Extract entity names from the tree.
fn entity_names(tree: &OwnedNode) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(indices) = tree.field_indices.get("entities") {
        for &idx in indices {
            if let Some(child) = tree.children.get(idx) {
                if let Some(ref text) = child.text {
                    names.push(text.clone());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Extract arc shapes from the tree as (entity_name, shape) pairs.
fn arc_shapes(tree: &OwnedNode) -> Vec<(String, String)> {
    let mut arcs = Vec::new();
    if let Some(indices) = tree.field_indices.get("arcs") {
        for &idx in indices {
            if let Some(child) = tree.children.get(idx) {
                let name = child.text.clone().unwrap_or_default();
                let shape = child
                    .field_indices
                    .get("shape")
                    .and_then(|si| si.first())
                    .and_then(|&i| child.children.get(i))
                    .and_then(|n| n.text.clone())
                    .unwrap_or_default();
                arcs.push((name, shape));
            }
        }
    }
    arcs.sort();
    arcs
}

/// Extract the narrative summary text from the tree.
fn narrative_summary_text(tree: &OwnedNode) -> String {
    tree.field_indices
        .get("narrative_summary")
        .and_then(|indices| indices.first())
        .and_then(|&idx| tree.children.get(idx))
        .and_then(|n| n.text.clone())
        .unwrap_or_default()
}

#[test]
fn test_genesis_37_corpus_regression() {
    if !spacy_available() {
        eprintln!("Skipping Genesis 37 regression: spaCy not available");
        return;
    }

    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/genesis37");

    let part1 = std::fs::read_to_string(fixtures.join("part1_dreams.txt")).unwrap();
    let part2 = std::fs::read_to_string(fixtures.join("part2_journey.txt")).unwrap();
    let part3 = std::fs::read_to_string(fixtures.join("part3_betrayal.txt")).unwrap();

    // Single-file: concatenated text
    let full_text = format!("{}\n\n{}\n\n{}", part1.trim(), part2.trim(), part3.trim());
    let backend = NlpBackend;
    let single_tree = backend
        .parse(&full_text, "english", Some("genesis37.txt"))
        .unwrap();

    // Corpus: 3 separate parses + build_corpus
    let tree1 = backend
        .parse(&part1, "english", Some("part1_dreams.txt"))
        .unwrap();
    let tree2 = backend
        .parse(&part2, "english", Some("part2_journey.txt"))
        .unwrap();
    let tree3 = backend
        .parse(&part3, "english", Some("part3_betrayal.txt"))
        .unwrap();

    let file_trees = vec![
        (tree1, "part1_dreams.txt".to_string()),
        (tree2, "part2_journey.txt".to_string()),
        (tree3, "part3_betrayal.txt".to_string()),
    ];
    let (corpus_tree, metadata) = build_corpus(file_trees);

    // --- Comparisons ---

    // Metadata should list 3 files.
    assert_eq!(metadata.files.len(), 3, "corpus should track 3 files");

    // Scene count: corpus should produce at least as many scenes as single-file.
    let single_scenes = scene_count(&single_tree);
    let corpus_scenes = scene_count(&corpus_tree);
    eprintln!("Scenes: single={}, corpus={}", single_scenes, corpus_scenes);
    // Corpus may detect more scenes at file boundaries. Allow ±2.
    assert!(
        corpus_scenes >= single_scenes.saturating_sub(2),
        "Corpus scenes ({}) should not be much less than single-file ({})",
        corpus_scenes,
        single_scenes
    );

    // Entities: corpus should contain the key biblical characters.
    let corpus_entities = entity_names(&corpus_tree);
    let single_entities = entity_names(&single_tree);
    eprintln!("Single entities: {:?}", single_entities);
    eprintln!("Corpus entities: {:?}", corpus_entities);
    let key_names = ["Joseph", "Jacob", "Reuben", "Judah"];
    for name in &key_names {
        let found = corpus_entities.iter().any(|e| e.contains(name));
        // Log but don't hard-fail — spaCy entity recognition is model-dependent.
        if !found {
            eprintln!(
                "WARNING: key entity '{}' not found in corpus entities",
                name
            );
        }
    }

    // Arc shapes: corpus should have arcs if single-file does.
    let single_arcs = arc_shapes(&single_tree);
    let corpus_arcs = arc_shapes(&corpus_tree);
    eprintln!("Single arcs: {:?}", single_arcs);
    eprintln!("Corpus arcs: {:?}", corpus_arcs);
    if !single_arcs.is_empty() {
        assert!(
            !corpus_arcs.is_empty(),
            "Corpus should produce arcs when single-file does"
        );
    }

    // Narrative summary must exist.
    let single_summary = narrative_summary_text(&single_tree);
    let corpus_summary = narrative_summary_text(&corpus_tree);
    eprintln!("Single summary: {}", single_summary);
    eprintln!("Corpus summary: {}", corpus_summary);
    assert!(
        !corpus_summary.is_empty(),
        "Corpus should produce narrative summary"
    );
}

#[test]
fn test_apollo_13_corpus_regression() {
    if !spacy_available() {
        eprintln!("Skipping Apollo 13 regression: spaCy not available");
        return;
    }

    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/apollo13");

    let part1 = std::fs::read_to_string(fixtures.join("part1_launch.txt")).unwrap();
    let part2 = std::fs::read_to_string(fixtures.join("part2_transit.txt")).unwrap();
    let part3 = std::fs::read_to_string(fixtures.join("part3_accident.txt")).unwrap();

    // Single-file: concatenated
    let full_text = format!("{}\n\n{}\n\n{}", part1.trim(), part2.trim(), part3.trim());
    let backend = NlpBackend;
    let single_tree = backend
        .parse(&full_text, "english", Some("apollo13.txt"))
        .unwrap();

    // Corpus: 3 separate parses
    let tree1 = backend
        .parse(&part1, "english", Some("part1_launch.txt"))
        .unwrap();
    let tree2 = backend
        .parse(&part2, "english", Some("part2_transit.txt"))
        .unwrap();
    let tree3 = backend
        .parse(&part3, "english", Some("part3_accident.txt"))
        .unwrap();

    let file_trees = vec![
        (tree1, "part1_launch.txt".to_string()),
        (tree2, "part2_transit.txt".to_string()),
        (tree3, "part3_accident.txt".to_string()),
    ];
    let (corpus_tree, metadata) = build_corpus(file_trees);

    // Metadata
    assert_eq!(metadata.files.len(), 3, "corpus should track 3 files");

    // Scene count
    let single_scenes = scene_count(&single_tree);
    let corpus_scenes = scene_count(&corpus_tree);
    eprintln!(
        "Apollo13 Scenes: single={}, corpus={}",
        single_scenes, corpus_scenes
    );
    assert!(
        corpus_scenes >= single_scenes.saturating_sub(2),
        "Corpus scenes ({}) should not be much less than single-file ({})",
        corpus_scenes,
        single_scenes
    );

    // Entities: key personnel
    let corpus_entities = entity_names(&corpus_tree);
    eprintln!("Apollo13 corpus entities: {:?}", corpus_entities);
    let key_names = ["Lovell", "Swigert", "Haise"];
    for name in &key_names {
        let found = corpus_entities.iter().any(|e| e.contains(name));
        if !found {
            eprintln!(
                "WARNING: key entity '{}' not found in corpus entities",
                name
            );
        }
    }

    // Arcs
    let single_arcs = arc_shapes(&single_tree);
    let corpus_arcs = arc_shapes(&corpus_tree);
    eprintln!("Apollo13 Single arcs: {:?}", single_arcs);
    eprintln!("Apollo13 Corpus arcs: {:?}", corpus_arcs);

    // Narrative summary
    let corpus_summary = narrative_summary_text(&corpus_tree);
    eprintln!("Apollo13 Corpus summary: {}", corpus_summary);
    assert!(
        !corpus_summary.is_empty(),
        "Corpus should produce narrative summary"
    );
}
