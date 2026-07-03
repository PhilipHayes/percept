use std::thread;
use std::time::Duration;

use wq_core::{Error, NewNode, UpdateNode, WqDb};

// ── helpers ──────────────────────────────────────────────────────────────────

fn ticket(title: &str, status: &str) -> NewNode {
    NewNode {
        node_type: "ticket".to_string(),
        title: title.to_string(),
        body: None,
        status: Some(status.to_string()),
        harness_origin: None,
        metadata: None,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn create_node_generates_id_and_timestamps() {
    let db = WqDb::open_in_memory().unwrap();
    let node = db
        .create_node(NewNode {
            node_type: "ticket".to_string(),
            title: "Test ticket".to_string(),
            body: None,
            status: Some("open".to_string()),
            harness_origin: None,
            metadata: None,
        })
        .unwrap();

    assert!(!node.id.is_empty(), "id must be non-empty");
    uuid::Uuid::parse_str(&node.id).expect("id must be a valid UUIDv4 string");
    assert_eq!(
        node.created_at, node.updated_at,
        "created_at and updated_at must be equal on first insert"
    );
}

#[test]
fn get_node_round_trips_all_fields() {
    let db = WqDb::open_in_memory().unwrap();
    let new = NewNode {
        node_type: "epic".to_string(),
        title: "My epic".to_string(),
        body: Some("some body text".to_string()),
        status: Some("in-progress".to_string()),
        harness_origin: Some("linear:ENG-42".to_string()),
        metadata: Some(serde_json::json!({"key": "value"})),
    };
    let created = db.create_node(new.clone()).unwrap();
    let fetched = db.get_node(&created.id).unwrap();

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.node_type, new.node_type);
    assert_eq!(fetched.title, new.title);
    assert_eq!(fetched.body, new.body);
    assert_eq!(fetched.status, new.status);
    assert_eq!(fetched.harness_origin, new.harness_origin);
    assert_eq!(fetched.metadata, new.metadata);
}

#[test]
fn get_node_unknown_id_returns_typed_not_found() {
    let db = WqDb::open_in_memory().unwrap();
    let result = db.get_node("nonexistent-id");
    assert!(
        matches!(result, Err(Error::NodeNotFound(_))),
        "expected Err(NodeNotFound(_)), got {:?}",
        result
    );
}

#[test]
fn update_node_changes_updated_at_not_created_at() {
    let db = WqDb::open_in_memory().unwrap();
    let created = db.create_node(ticket("Before", "open")).unwrap();

    let original_created_at = created.created_at.clone();
    let original_updated_at = created.updated_at.clone();

    // Ensure chrono::Utc::now() advances before the update
    thread::sleep(Duration::from_millis(10));

    let updated = db
        .update_node(
            &created.id,
            UpdateNode {
                title: Some("After".to_string()),
                ..UpdateNode::default()
            },
        )
        .unwrap();

    assert_eq!(
        updated.created_at, original_created_at,
        "created_at must remain unchanged after update"
    );
    assert!(
        updated.updated_at > original_updated_at,
        "updated_at ({}) must be strictly greater than original ({})",
        updated.updated_at,
        original_updated_at
    );
}

#[test]
fn list_nodes_filters_by_type_and_status() {
    let db = WqDb::open_in_memory().unwrap();

    // ("ticket","open"), ("ticket","done"), ("epic","open"), ("note", None)
    db.create_node(ticket("T1", "open")).unwrap();
    db.create_node(ticket("T2", "done")).unwrap();
    db.create_node(NewNode {
        node_type: "epic".to_string(),
        title: "E1".to_string(),
        body: None,
        status: Some("open".to_string()),
        harness_origin: None,
        metadata: None,
    })
    .unwrap();
    db.create_node(NewNode {
        node_type: "note".to_string(),
        title: "N1".to_string(),
        body: None,
        status: None,
        harness_origin: None,
        metadata: None,
    })
    .unwrap();

    let filtered = db.list_nodes(Some("ticket"), Some("open")).unwrap();
    assert_eq!(
        filtered.len(),
        1,
        "expected exactly 1 open ticket, got {}",
        filtered.len()
    );
    assert_eq!(filtered[0].node_type, "ticket");
    assert_eq!(filtered[0].status, Some("open".to_string()));

    let all = db.list_nodes(None, None).unwrap();
    assert_eq!(
        all.len(),
        4,
        "expected all 4 nodes with no filter, got {}",
        all.len()
    );
}

#[test]
fn metadata_json_round_trips_without_lossiness() {
    let db = WqDb::open_in_memory().unwrap();
    let meta = serde_json::json!({"a": {"b": [1, 2.5, "x"], "c": null}});

    let created = db
        .create_node(NewNode {
            node_type: "note".to_string(),
            title: "Meta test".to_string(),
            body: None,
            status: None,
            harness_origin: None,
            metadata: Some(meta.clone()),
        })
        .unwrap();

    let fetched = db.get_node(&created.id).unwrap();
    assert_eq!(
        fetched.metadata,
        Some(meta),
        "metadata JSON must round-trip without lossiness"
    );
}
