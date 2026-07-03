//! wq-2.1 RED — embedding write path + semantic search (4 tests).
//!
//! Contract per phase-wq-2-plan.yml substory wq-2.1 and the vec0 spike
//! findings (reviews/manual/wq-spikes/vec0-poc/FINDINGS.md): embeddings are
//! written at create_node time into the vec0 `node_embeddings` table;
//! search() embeds the query, runs a KNN MATCH (`k = ?` — not LIMIT), and
//! joins back to full Node records ordered best-first.

mod common;

use wq_core::{NewNode, WqDb};

fn node(title: &str, body: &str) -> NewNode {
    NewNode {
        node_type: "ticket".into(),
        title: title.into(),
        body: Some(body.into()),
        status: Some("open".into()),
        harness_origin: None,
        metadata: None,
    }
}

/// Five semantically distinct fixture nodes; returns the id of the
/// authentication-themed one (the expected best match in ranking tests).
fn seed_fixture(db: &WqDb) -> String {
    let mut engine = common::engine().lock().unwrap();
    let auth = db
        .create_node(
            &mut engine,
            node(
                "Authentication rework",
                "Replace session cookies with signed tokens; login, logout, password reset flows.",
            ),
        )
        .unwrap();
    for (t, b) in [
        (
            "Chord wheel rotation bug",
            "Dial snaps back when rotated past the octave boundary in the music UI.",
        ),
        (
            "Database migration tooling",
            "Schema versioning for the analytics warehouse; backfill and rollback scripts.",
        ),
        (
            "Garden watering schedule",
            "Automate drip irrigation timing based on soil moisture sensor readings.",
        ),
        (
            "Invoice PDF rendering",
            "Fix page-break layout when line items overflow a single page.",
        ),
    ] {
        db.create_node(&mut engine, node(t, b)).unwrap();
    }
    auth.id
}

#[test]
fn create_node_generates_and_stores_embedding() {
    let db = WqDb::open_in_memory().unwrap();
    let mut engine = common::engine().lock().unwrap();
    let created = db
        .create_node(&mut engine, node("Some ticket", "with a body"))
        .unwrap();
    drop(engine);

    // Verified via direct query, not via search() — plumbing, not ranking.
    let count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM node_embeddings WHERE node_id = ?1",
            [&created.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "create_node must write exactly one embedding row");
}

#[test]
fn search_ranks_closest_semantic_match_first() {
    let db = WqDb::open_in_memory().unwrap();
    let auth_id = seed_fixture(&db);

    let mut engine = common::engine().lock().unwrap();
    let results = db
        .search(&mut engine, "login and password rework", 5)
        .unwrap();

    assert!(!results.is_empty());
    assert_eq!(
        results[0].0.id,
        auth_id,
        "authentication node should rank first, got: {:?}",
        results
            .iter()
            .map(|(n, d)| (&n.title, d))
            .collect::<Vec<_>>()
    );
    // Ordered best-first: distances non-decreasing.
    let distances: Vec<f32> = results.iter().map(|(_, d)| *d).collect();
    let mut sorted = distances.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        distances, sorted,
        "results must be ordered by distance ascending"
    );
}

#[test]
fn search_respects_k_truncation() {
    let db = WqDb::open_in_memory().unwrap();
    seed_fixture(&db);

    let mut engine = common::engine().lock().unwrap();
    let results = db.search(&mut engine, "software work", 2).unwrap();
    assert_eq!(
        results.len(),
        2,
        "k=2 on a 5-node fixture returns exactly 2"
    );
}

#[test]
fn search_returns_full_node_records() {
    let db = WqDb::open_in_memory().unwrap();
    seed_fixture(&db);

    let mut engine = common::engine().lock().unwrap();
    let results = db.search(&mut engine, "user login security", 3).unwrap();

    for (n, _) in &results {
        assert!(!n.id.is_empty());
        assert!(!n.title.is_empty());
        assert!(n.body.is_some(), "fixture bodies must survive the join");
        assert_eq!(n.node_type, "ticket");
        assert_eq!(n.status.as_deref(), Some("open"));
        assert!(!n.created_at.is_empty());
    }
}
