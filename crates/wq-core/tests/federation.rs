//! wq-3.1 RED — registry CRUD + rollup (7 tests).
//!
//! Contract per phase-wq-3-plan.yml: pointer-only registration, live
//! rollup across (nested) registries, stale-pointer warnings instead of
//! hard errors, cycle termination, and write-target resolution (D-wq-3-3).
//!
//! Fixture DBs are temp FILES, not in-memory — ATTACH needs real paths.

mod common;

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use wq_core::{resolve_write_target, NewNode, WqDb};

fn db_at(dir: &Path, name: &str) -> (WqDb, PathBuf) {
    let path = dir.join(name);
    (WqDb::open(&path).unwrap(), path)
}

fn seed_node(db: &WqDb, title: &str) -> String {
    let mut engine = common::engine().lock().unwrap();
    db.create_node(
        &mut engine,
        NewNode {
            node_type: "ticket".into(),
            title: title.into(),
            body: None,
            status: Some("open".into()),
            harness_origin: None,
            metadata: None,
        },
    )
    .unwrap()
    .id
}

#[test]
fn register_child_adds_pointer_row_not_copy() {
    let dir = TempDir::new().unwrap();
    let (parent, _) = db_at(dir.path(), "parent.db");
    let (child, child_path) = db_at(dir.path(), "child.db");
    seed_node(&child, "child ticket");

    parent.register_child("childproj", &child_path).unwrap();

    let entries = parent.list_registered_children().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].project_name, "childproj");
    assert_eq!(entries[0].db_path, child_path.to_string_lossy());

    // Pointer, not copy: parent's own nodes table stays empty.
    let parent_nodes = parent.list_nodes(None, None).unwrap();
    assert!(parent_nodes.is_empty(), "registration must not copy rows");
    // Child untouched.
    assert_eq!(child.list_nodes(None, None).unwrap().len(), 1);
}

#[test]
fn rollup_returns_nodes_from_all_registered_children() {
    let dir = TempDir::new().unwrap();
    let (parent, _) = db_at(dir.path(), "parent.db");
    let (child_a, path_a) = db_at(dir.path(), "a.db");
    let (child_b, path_b) = db_at(dir.path(), "b.db");

    seed_node(&parent, "parent node");
    seed_node(&child_a, "node in A");
    seed_node(&child_b, "node in B");

    parent.register_child("a", &path_a).unwrap();
    parent.register_child("b", &path_b).unwrap();

    let result = parent.rollup().unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let mut titles: Vec<&str> = result.nodes.iter().map(|n| n.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, vec!["node in A", "node in B", "parent node"]);
}

#[test]
fn rollup_supports_nested_registry_two_levels_deep() {
    let dir = TempDir::new().unwrap();
    let (grandparent, _) = db_at(dir.path(), "grand.db");
    let (parent, parent_path) = db_at(dir.path(), "mid.db");
    let (leaf, leaf_path) = db_at(dir.path(), "leaf.db");

    seed_node(&leaf, "leaf node");
    parent.register_child("leaf", &leaf_path).unwrap();
    grandparent.register_child("mid", &parent_path).unwrap();

    let result = grandparent.rollup().unwrap();
    let titles: Vec<&str> = result.nodes.iter().map(|n| n.title.as_str()).collect();
    assert!(
        titles.contains(&"leaf node"),
        "leaf nodes must surface through a nested registry, got {titles:?}"
    );
}

#[test]
fn rollup_skips_stale_child_with_warning_not_error() {
    let dir = TempDir::new().unwrap();
    let (parent, _) = db_at(dir.path(), "parent.db");
    let (child_ok, path_ok) = db_at(dir.path(), "ok.db");
    let stale_path = dir.path().join("gone.db");
    {
        let (child_gone, _) = db_at(dir.path(), "gone.db");
        seed_node(&child_gone, "doomed node");
    }
    seed_node(&child_ok, "surviving node");

    parent.register_child("ok", &path_ok).unwrap();
    parent.register_child("gone", &stale_path).unwrap();
    std::fs::remove_file(&stale_path).unwrap();

    let result = parent.rollup().unwrap();

    assert_eq!(result.warnings.len(), 1, "exactly one stale-child warning");
    assert!(
        result.warnings[0].db_path.contains("gone.db"),
        "warning must name the missing path: {:?}",
        result.warnings[0]
    );
    let titles: Vec<&str> = result.nodes.iter().map(|n| n.title.as_str()).collect();
    assert!(
        titles.contains(&"surviving node"),
        "valid children must still be rolled up: {titles:?}"
    );
}

#[test]
fn rollup_terminates_on_cyclic_registry() {
    let dir = TempDir::new().unwrap();
    let (a, path_a) = db_at(dir.path(), "a.db");
    let (b, path_b) = db_at(dir.path(), "b.db");

    seed_node(&a, "node in A");
    seed_node(&b, "node in B");
    a.register_child("b", &path_b).unwrap();
    b.register_child("a", &path_a).unwrap(); // cycle: A → B → A

    // Watchdog: fail rather than hang if the visited-set guard regresses.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = a.rollup();
        let _ = tx.send(result);
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("rollup() must terminate on a cyclic registry")
        .unwrap();

    let mut titles: Vec<&str> = result.nodes.iter().map(|n| n.title.as_str()).collect();
    titles.sort_unstable();
    titles.dedup();
    assert_eq!(
        titles,
        vec!["node in A", "node in B"],
        "each node appears despite the cycle, without infinite duplication"
    );
}

#[test]
fn resolve_write_target_defaults_to_project_scoped() {
    let dir = TempDir::new().unwrap();
    let resolved = resolve_write_target(dir.path(), false);
    assert_eq!(
        resolved,
        wq_core::resolve_project_db_path(dir.path()),
        "non-global writes target the project-scoped DB"
    );
}

#[test]
fn resolve_write_target_honors_explicit_global() {
    let dir = TempDir::new().unwrap();
    let resolved = resolve_write_target(dir.path(), true);
    assert_eq!(
        resolved,
        wq_core::resolve_global_db_path(),
        "explicit_global=true must return the global DB path regardless of cwd"
    );
    assert_ne!(
        resolved,
        wq_core::resolve_project_db_path(dir.path()),
        "global target must not be the project-scoped path"
    );
}
