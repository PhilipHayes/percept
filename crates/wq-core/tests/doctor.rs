//! Doctor checks conditions wq's own write path cannot produce but its
//! documented escape hatches can. Each test creates the drift the way it
//! actually happens — raw SQL through `query_json`, the same surface
//! `wq query` exposes — rather than reaching past the public API.
//!
//! No `EmbedEngine` anywhere: doctor must run where the model cannot load.

use wq_core::WqDb;

fn insert_node(db: &WqDb, id: &str, title: &str) {
    db.query_json(&format!(
        "INSERT INTO nodes (id, type, title, body, status, harness_origin,
                            created_at, updated_at, metadata_json)
         VALUES ('{id}', 'ticket', '{title}', NULL, 'open', 'test',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL)"
    ))
    .unwrap();
}

fn insert_embedding(db: &WqDb, node_id: &str) {
    let zeros = vec!["0"; 384].join(",");
    db.query_json(&format!(
        "INSERT INTO node_embeddings (node_id, embedding)
         VALUES ('{node_id}', '[{zeros}]')"
    ))
    .unwrap();
}

fn finding<'a>(report: &'a wq_core::DoctorReport, check: &str) -> &'a wq_core::DoctorFinding {
    report
        .findings
        .iter()
        .find(|f| f.check == check)
        .unwrap_or_else(|| panic!("expected a {check} finding, got {:?}", report.findings))
}

#[test]
fn an_empty_db_is_healthy() {
    let db = WqDb::open_in_memory().unwrap();
    let report = db.doctor().unwrap();

    assert!(report.healthy(), "{:?}", report.findings);
    assert_eq!(report.total_nodes, 0);
}

#[test]
fn a_fully_indexed_db_is_healthy() {
    let db = WqDb::open_in_memory().unwrap();
    insert_node(&db, "a", "one");
    insert_embedding(&db, "a");

    let report = db.doctor().unwrap();
    assert!(report.healthy(), "{:?}", report.findings);
    assert_eq!(report.total_nodes, 1);
    assert_eq!(report.total_embeddings, 1);
}

#[test]
fn reports_unindexed_nodes() {
    let db = WqDb::open_in_memory().unwrap();
    insert_node(&db, "a", "one");
    insert_node(&db, "b", "two");
    insert_embedding(&db, "a");

    let report = db.doctor().unwrap();
    assert!(!report.healthy());
    assert_eq!(finding(&report, "unindexed_nodes").count, 1);
    assert_eq!(finding(&report, "unindexed_nodes").remedy, "wq reindex");
}

#[test]
fn reports_embeddings_left_behind_by_a_deleted_node() {
    let db = WqDb::open_in_memory().unwrap();
    insert_node(&db, "a", "one");
    insert_embedding(&db, "a");

    // node_embeddings is a vec0 virtual table: no foreign key, no cascade.
    db.query_json("DELETE FROM nodes WHERE id = 'a'").unwrap();

    let report = db.doctor().unwrap();
    assert_eq!(finding(&report, "orphan_embeddings").count, 1);
    assert_eq!(report.total_nodes, 0);
    assert_eq!(report.total_embeddings, 1);
}

#[test]
fn reports_edges_pointing_at_missing_nodes() {
    let db = WqDb::open_in_memory().unwrap();
    insert_node(&db, "a", "one");
    insert_embedding(&db, "a");

    // wq's own connections set foreign_keys=ON, so this is what a DB written by
    // another tool looks like once opened here.
    db.query_json("PRAGMA foreign_keys = OFF").unwrap();
    db.query_json(
        "INSERT INTO edges (id, from_id, to_id, kind, weight, created_at)
         VALUES ('e1', 'a', 'does-not-exist', 'blocks', 1.0, '2026-01-01T00:00:00Z')",
    )
    .unwrap();

    let report = db.doctor().unwrap();
    assert_eq!(finding(&report, "dangling_edges").count, 1);
    assert_eq!(report.total_edges, 1);
}

#[test]
fn reports_every_independent_problem_at_once() {
    let db = WqDb::open_in_memory().unwrap();
    insert_node(&db, "a", "one");
    insert_node(&db, "b", "two");
    insert_embedding(&db, "gone");

    db.query_json("PRAGMA foreign_keys = OFF").unwrap();
    db.query_json(
        "INSERT INTO edges (id, from_id, to_id, kind, weight, created_at)
         VALUES ('e1', 'a', 'missing', 'blocks', 1.0, '2026-01-01T00:00:00Z')",
    )
    .unwrap();

    let report = db.doctor().unwrap();
    let checks: Vec<&str> = report.findings.iter().map(|f| f.check.as_str()).collect();
    assert!(checks.contains(&"unindexed_nodes"));
    assert!(checks.contains(&"orphan_embeddings"));
    assert!(checks.contains(&"dangling_edges"));
    assert_eq!(report.findings.len(), 3);
}
