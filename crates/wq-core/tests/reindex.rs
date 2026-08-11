//! Reindex covers nodes that entered the DB without going through
//! `create_node` — a seed script, a restored dump, a raw `wq query` INSERT.
//!
//! These tests deliberately never construct an `EmbedEngine`: the discovery
//! half of reindex is the new logic and it is exactly the half that must work
//! without an ONNX model. The embedding half reuses `create_node`'s already
//! covered path.

use wq_core::WqDb;

/// Inserts a node by raw SQL, exactly as a seed script or an external import
/// would — through `query_json`, the same escape hatch `wq query` exposes. No
/// embedding row is written, which is the condition under test.
fn insert_unindexed(db: &WqDb, id: &str, title: &str) {
    db.query_json(&format!(
        "INSERT INTO nodes (id, type, title, body, status, harness_origin,
                            created_at, updated_at, metadata_json)
         VALUES ('{id}', 'ticket', '{title}', NULL, 'open', 'test',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL)"
    ))
    .unwrap();
}

/// A zero vector of the dimension `node_embeddings` declares. The value is
/// irrelevant here — only its presence is.
fn insert_embedding(db: &WqDb, node_id: &str) {
    let zeros = vec!["0"; 384].join(",");
    db.query_json(&format!(
        "INSERT INTO node_embeddings (node_id, embedding)
         VALUES ('{node_id}', '[{zeros}]')"
    ))
    .unwrap();
}

#[test]
fn empty_db_is_fully_indexed() {
    let db = WqDb::open_in_memory().unwrap();
    let report = db.reindex_dry_run().unwrap();

    assert_eq!(report.total_nodes, 0);
    assert_eq!(report.missing_before, 0);
    assert!(report.dry_run);
    assert!(report.fully_indexed());
}

#[test]
fn finds_nodes_inserted_outside_create_node() {
    let db = WqDb::open_in_memory().unwrap();
    insert_unindexed(&db, "a", "seeded one");
    insert_unindexed(&db, "b", "seeded two");

    let report = db.reindex_dry_run().unwrap();
    assert_eq!(report.total_nodes, 2);
    assert_eq!(report.missing_before, 2);
    assert_eq!(report.indexed, 0, "a dry run must not write");
    assert!(!report.fully_indexed());

    let pending = db.unindexed_nodes().unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].title, "seeded one");
}

#[test]
fn dry_run_writes_nothing() {
    let db = WqDb::open_in_memory().unwrap();
    insert_unindexed(&db, "a", "seeded");

    db.reindex_dry_run().unwrap();

    // Still missing after the dry run — the whole point of it being dry.
    assert_eq!(db.unindexed_count().unwrap(), 1);
}

#[test]
fn a_node_with_an_embedding_is_not_pending() {
    let db = WqDb::open_in_memory().unwrap();
    insert_unindexed(&db, "a", "seeded");

    insert_embedding(&db, "a");

    assert_eq!(db.unindexed_count().unwrap(), 0);
    assert!(db.unindexed_nodes().unwrap().is_empty());
    assert!(db.reindex_dry_run().unwrap().fully_indexed());
}

#[test]
fn counts_only_the_unindexed_when_partially_indexed() {
    let db = WqDb::open_in_memory().unwrap();
    for (id, title) in [("a", "one"), ("b", "two"), ("c", "three")] {
        insert_unindexed(&db, id, title);
    }

    insert_embedding(&db, "b");

    let report = db.reindex_dry_run().unwrap();
    assert_eq!(report.total_nodes, 3);
    assert_eq!(report.missing_before, 2);

    let pending: Vec<String> = db
        .unindexed_nodes()
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(pending, vec!["a".to_string(), "c".to_string()]);
}
