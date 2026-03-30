use aq_core::backend::Backend;
use aq_core::node::OwnedNode;
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

/// Recursively count nodes in the tree.
fn node_count(node: &OwnedNode) -> usize {
    1 + node.children.iter().map(node_count).sum::<usize>()
}

/// Recursively collect all field_indices keys at every level.
fn collect_field_keys(node: &OwnedNode, out: &mut Vec<Vec<String>>) {
    let mut keys: Vec<String> = node.field_indices.keys().cloned().collect();
    keys.sort();
    out.push(keys);
    for child in &node.children {
        collect_field_keys(child, out);
    }
}

#[test]
fn test_full_nlp_tree_roundtrip() {
    if !spacy_available() {
        return;
    }

    let text = "Joseph dreamed a dream and told it to his brothers. \
                They hated him and could not speak peaceably unto him. \
                Jacob sent Joseph to Shechem to check on his brothers.";

    let backend = NlpBackend;
    let tree = backend.parse(text, "english", Some("genesis.txt")).unwrap();

    // Serialize
    let json = serde_json::to_string_pretty(&tree).expect("serialize");

    // Deserialize
    let restored: OwnedNode = serde_json::from_str(&json).expect("deserialize");

    // Structural equality
    assert_eq!(tree, restored, "Round-trip produced different tree");

    // Verify non-trivial tree (not just a leaf)
    let count = node_count(&tree);
    assert!(count > 10, "Expected substantial tree, got {count} nodes");

    // Verify field_indices survived at all levels
    let mut original_keys = Vec::new();
    let mut restored_keys = Vec::new();
    collect_field_keys(&tree, &mut original_keys);
    collect_field_keys(&restored, &mut restored_keys);
    assert_eq!(
        original_keys, restored_keys,
        "field_indices keys differ after round-trip"
    );

    // Verify source_file propagated
    assert_eq!(tree.source_file.as_deref(), Some("genesis.txt"));
    assert_eq!(restored.source_file.as_deref(), Some("genesis.txt"));
}
