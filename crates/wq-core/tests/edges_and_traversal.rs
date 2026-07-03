use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use wq_core::{NewEdge, NewNode, WqDb};

mod common;

// ── helpers ────────────────────────────────────────────────────────────────────────────

fn fresh_db() -> WqDb {
    WqDb::open_in_memory().unwrap()
}

fn make_node(db: &WqDb, title: &str) -> String {
    let mut engine = common::engine().lock().unwrap();
    db.create_node(
        &mut engine,
        NewNode {
            node_type: "ticket".to_string(),
            title: title.to_string(),
            body: None,
            status: None,
            harness_origin: None,
            metadata: None,
        },
    )
    .unwrap()
    .id
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn create_edge_round_trips() {
    let db = fresh_db();
    let from = make_node(&db, "A");
    let to = make_node(&db, "B");

    let created = db
        .create_edge(NewEdge {
            from_id: from.clone(),
            to_id: to.clone(),
            kind: "blocks".to_string(),
            weight: Some(0.8),
        })
        .unwrap();

    assert!(!created.id.is_empty(), "edge id must be non-empty");
    uuid::Uuid::parse_str(&created.id).expect("edge id must be a valid UUID string");
    assert!(
        !created.created_at.is_empty(),
        "created_at must be non-empty"
    );
    assert_eq!(created.weight, 0.8);

    let edges = db.list_edges(Some(&from), None, Some("blocks")).unwrap();

    assert_eq!(edges.len(), 1, "expected exactly 1 matching edge");
    let edge = &edges[0];
    assert_eq!(edge.id, created.id);
    assert_eq!(edge.from_id, from);
    assert_eq!(edge.to_id, to);
    assert_eq!(edge.kind, "blocks");
    assert_eq!(edge.weight, 0.8);
    assert_eq!(edge.created_at, created.created_at);
}

#[test]
fn create_edge_rejects_unknown_node_id() {
    let db = fresh_db();
    let known = make_node(&db, "A");

    let bad_from = db.create_edge(NewEdge {
        from_id: "nonexistent-from".to_string(),
        to_id: known.clone(),
        kind: "blocks".to_string(),
        weight: None,
    });
    assert!(
        bad_from.is_err(),
        "create_edge with an unknown from_id must return Err"
    );

    let bad_to = db.create_edge(NewEdge {
        from_id: known,
        to_id: "nonexistent-to".to_string(),
        kind: "blocks".to_string(),
        weight: None,
    });
    assert!(
        bad_to.is_err(),
        "create_edge with an unknown to_id must return Err"
    );
}

#[test]
fn traverse_depth_one_returns_direct_neighbors_only() {
    let db = fresh_db();
    let a = make_node(&db, "A");
    let b = make_node(&db, "B");
    let c = make_node(&db, "C");

    db.create_edge(NewEdge {
        from_id: a.clone(),
        to_id: b.clone(),
        kind: "blocks".to_string(),
        weight: None,
    })
    .unwrap();
    db.create_edge(NewEdge {
        from_id: b.clone(),
        to_id: c.clone(),
        kind: "blocks".to_string(),
        weight: None,
    })
    .unwrap();

    let result = db.traverse(&a, "blocks", 1).unwrap();

    assert_eq!(result.len(), 1, "expected exactly 1 node at depth 1");
    assert_eq!(result[0].0.id, b);
    assert_eq!(result[0].1, 1);
    assert!(
        !result.iter().any(|(n, _)| n.id == c),
        "depth-1 traversal must not include C"
    );
}

#[test]
fn traverse_depth_n_returns_transitive_neighbors_filtered_by_kind() {
    let db = fresh_db();
    let a = make_node(&db, "A");
    let b = make_node(&db, "B");
    let c = make_node(&db, "C");
    let d = make_node(&db, "D");

    db.create_edge(NewEdge {
        from_id: a.clone(),
        to_id: b.clone(),
        kind: "depends_on".to_string(),
        weight: None,
    })
    .unwrap();
    db.create_edge(NewEdge {
        from_id: b.clone(),
        to_id: c.clone(),
        kind: "depends_on".to_string(),
        weight: None,
    })
    .unwrap();
    db.create_edge(NewEdge {
        from_id: a.clone(),
        to_id: d.clone(),
        kind: "blocks".to_string(),
        weight: None,
    })
    .unwrap();

    let mut result = db.traverse(&a, "depends_on", 3).unwrap();
    result.sort_by(|x, y| x.0.id.cmp(&y.0.id));

    let ids: Vec<&str> = result.iter().map(|(n, _)| n.id.as_str()).collect();
    assert!(ids.contains(&b.as_str()), "expected B in result");
    assert!(ids.contains(&c.as_str()), "expected C in result");
    assert!(
        !ids.contains(&d.as_str()),
        "D must not be in result (wrong edge kind)"
    );

    let b_entry = result.iter().find(|(n, _)| n.id == b).unwrap();
    assert_eq!(b_entry.1, 1, "B must be at depth 1");
    let c_entry = result.iter().find(|(n, _)| n.id == c).unwrap();
    assert_eq!(c_entry.1, 2, "C must be at depth 2");
}

#[test]
fn traverse_terminates_on_cyclic_graph() {
    let db = fresh_db();
    let a = make_node(&db, "A");
    let b = make_node(&db, "B");

    db.create_edge(NewEdge {
        from_id: a.clone(),
        to_id: b.clone(),
        kind: "blocks".to_string(),
        weight: None,
    })
    .unwrap();
    db.create_edge(NewEdge {
        from_id: b.clone(),
        to_id: a.clone(),
        kind: "blocks".to_string(),
        weight: None,
    })
    .unwrap();

    let (tx, rx) = mpsc::channel();
    let start = a.clone();
    thread::spawn(move || {
        let result = db.traverse(&start, "blocks", 5);
        let _ = tx.send(result);
    });

    let result = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("traverse() did not return within 5s — likely infinite recursion on a cyclic graph")
        .unwrap();

    assert!(
        result.iter().any(|(n, _)| n.id == b),
        "cyclic traversal must still contain B"
    );
}
