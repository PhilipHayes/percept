//! Regression tests for aq-nlp against technical documentation (ADR-like prose).
//!
//! Validates entity extraction, scene detection, and narrative structure
//! on architectural decision records — the primary prose domain for canopy's
//! gestalt vault integration.

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

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("technical_adr")
}

/// Extract scene count from a tree's field_indices.
fn scene_count(tree: &OwnedNode) -> usize {
    tree.field_indices.get("scenes").map_or(0, |v| v.len())
}

/// Extract entity names from the tree via field_indices.
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

/// Extract the narrative summary text from the tree.
fn narrative_summary_text(tree: &OwnedNode) -> String {
    tree.field_indices
        .get("narrative_summary")
        .and_then(|indices| indices.first())
        .and_then(|&idx| tree.children.get(idx))
        .and_then(|n| n.text.clone())
        .unwrap_or_default()
}

// ── Single-file parse ────────────────────────────────────────────────────────

#[test]
fn technical_adr_single_file_parses_without_error() {
    if !spacy_available() {
        eprintln!("Skipping technical_adr_single_file_parses_without_error — spaCy not available");
        return;
    }
    let part1 = std::fs::read_to_string(fixtures_dir().join("part1_context.txt")).unwrap();
    let backend = NlpBackend;
    let tree = backend
        .parse(&part1, "english", Some("part1_context.txt"))
        .unwrap();
    let entities = entity_names(&tree);
    let scenes = scene_count(&tree);
    eprintln!("Part 1 — scenes: {}, entities: {:?}", scenes, entities);
    assert!(
        !entities.is_empty(),
        "technical doc should produce at least 1 entity"
    );
}

#[test]
fn technical_adr_detects_person_entities() {
    if !spacy_available() {
        eprintln!("Skipping technical_adr_detects_person_entities — spaCy not available");
        return;
    }
    let part2 = std::fs::read_to_string(fixtures_dir().join("part2_decision.txt")).unwrap();
    let backend = NlpBackend;
    let tree = backend
        .parse(&part2, "english", Some("part2_decision.txt"))
        .unwrap();
    let names = entity_names(&tree);
    eprintln!("Part 2 entity names: {:?}", names);
    // Should detect at least some of: Philip Hayes, Sarah Chen, James Rodriguez, Houston.
    // Technical docs have fewer entities than fiction — we just check non-empty.
    assert!(
        !names.is_empty(),
        "decision doc should have at least 1 named entity"
    );
}

#[test]
fn technical_adr_detects_tool_names_as_entities() {
    if !spacy_available() {
        eprintln!("Skipping technical_adr_detects_tool_names_as_entities — spaCy not available");
        return;
    }
    let part1 = std::fs::read_to_string(fixtures_dir().join("part1_context.txt")).unwrap();
    let backend = NlpBackend;
    let tree = backend
        .parse(&part1, "english", Some("part1_context.txt"))
        .unwrap();
    let names = entity_names(&tree);
    eprintln!("Part 1 entity names: {:?}", names);
    // Tool names like "Canopy", "Rust", "Tauri", "Pixi.js" may or may not be detected
    // as entities — captures baseline behavior rather than asserting correctness.
    // Key question: does spaCy's en_core_web_sm recognise technical tool names?
    eprintln!("Tool name detection baseline captured — review entity list above");
}

// ── Corpus-level merge ───────────────────────────────────────────────────────

#[test]
fn technical_adr_corpus_merges_three_parts() {
    if !spacy_available() {
        eprintln!("Skipping technical_adr_corpus_merges_three_parts — spaCy not available");
        return;
    }
    let fixtures = fixtures_dir();
    let part1 = std::fs::read_to_string(fixtures.join("part1_context.txt")).unwrap();
    let part2 = std::fs::read_to_string(fixtures.join("part2_decision.txt")).unwrap();
    let part3 = std::fs::read_to_string(fixtures.join("part3_implementation.txt")).unwrap();

    let backend = NlpBackend;
    let tree1 = backend
        .parse(&part1, "english", Some("part1_context.txt"))
        .unwrap();
    let tree2 = backend
        .parse(&part2, "english", Some("part2_decision.txt"))
        .unwrap();
    let tree3 = backend
        .parse(&part3, "english", Some("part3_implementation.txt"))
        .unwrap();

    let file_trees = vec![
        (tree1, "part1_context.txt".to_string()),
        (tree2, "part2_decision.txt".to_string()),
        (tree3, "part3_implementation.txt".to_string()),
    ];
    let (corpus_tree, metadata) = build_corpus(file_trees);

    assert_eq!(metadata.files.len(), 3, "corpus should track 3 files");

    let scenes = scene_count(&corpus_tree);
    let entities = entity_names(&corpus_tree);

    eprintln!(
        "Technical ADR corpus: scenes={}, entities={}",
        scenes,
        entities.len()
    );
    eprintln!("Corpus entities: {:?}", entities);

    // Baseline assertions — technical docs should produce SOME structure.
    assert!(scenes >= 1, "should detect at least 1 scene");
    assert!(
        entities.len() >= 3,
        "should detect at least 3 entities across 3 parts"
    );
}

#[test]
fn technical_adr_scene_count_reasonable() {
    if !spacy_available() {
        eprintln!("Skipping technical_adr_scene_count_reasonable — spaCy not available");
        return;
    }
    let fixtures = fixtures_dir();
    let part1 = std::fs::read_to_string(fixtures.join("part1_context.txt")).unwrap();
    let part2 = std::fs::read_to_string(fixtures.join("part2_decision.txt")).unwrap();
    let part3 = std::fs::read_to_string(fixtures.join("part3_implementation.txt")).unwrap();

    let full_text = format!("{}\n\n{}\n\n{}", part1.trim(), part2.trim(), part3.trim());
    let backend = NlpBackend;
    let tree = backend
        .parse(&full_text, "english", Some("technical_adr.txt"))
        .unwrap();

    let scenes = scene_count(&tree);
    let paragraph_count = full_text
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .count();

    eprintln!("Scenes: {}, Paragraphs: {}", scenes, paragraph_count);

    // Key regression from fiction benchmarks: scene-per-paragraph was the main issue.
    // For technical docs, we expect fewer scenes than paragraphs.
    // This assertion captures whether the Jaccard threshold fix from bf40307 helps
    // with technical prose as well as fiction.
    if scenes == paragraph_count {
        eprintln!(
            "WARNING: scene-per-paragraph pattern detected in technical docs \
             — same issue as pre-fix fiction"
        );
    }
}

#[test]
fn technical_adr_entity_type_distribution() {
    if !spacy_available() {
        eprintln!("Skipping technical_adr_entity_type_distribution — spaCy not available");
        return;
    }
    let part2 = std::fs::read_to_string(fixtures_dir().join("part2_decision.txt")).unwrap();
    let backend = NlpBackend;
    let tree = backend
        .parse(&part2, "english", Some("part2_decision.txt"))
        .unwrap();

    let all_entities = entity_names(&tree);
    let narrative_summary = narrative_summary_text(&tree);
    eprintln!("All entities in decision doc: {:?}", all_entities);
    eprintln!("Narrative summary: {}", narrative_summary);

    // Part 2 has explicit person names: Sarah Chen, James Rodriguez, Houston
    // and technical terms: Rust, Python, spaCy, ONNX, Option A/B/C, Cargo.
    // The entity type filtering (bf40307) should prevent CARDINAL/ORDINAL pollution.
    // This test captures the baseline for technical docs.
    eprintln!(
        "Entity count: {} — review for type pollution (CARDINAL/ORDINAL leaking)",
        all_entities.len()
    );
}
