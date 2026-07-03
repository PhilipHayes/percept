//! wq-7.1 RED — post-review hardening (contrarian review, 2026-07-03).
//!
//! Three convergent findings from the 5-specialist review get fixed here:
//! - FAULT-001 (fault-finder, critical): register_child must absolutize a
//!   relative db_path against the REGISTERING DB's own directory, not
//!   whatever cwd rollup() later happens to be called from.
//! - FAULT-003 (fault-finder, major): no busy_timeout/WAL — concurrent
//!   writers hit raw SQLITE_BUSY instead of retrying.
//! - FAULT-004 (fault-finder, major): query_json silently executes only
//!   the first statement of semicolon-separated SQL, with no error.

use tempfile::TempDir;
use wq_core::{NewNode, WqDb};

fn make_node(db: &WqDb, title: &str) -> String {
    let mut engine = mq_embed::engine::EmbedEngine::new(mq_embed::model::ModelKind::BgeSmall)
        .expect("model loads");
    db.create_node(
        &mut engine,
        NewNode {
            node_type: "ticket".into(),
            title: title.into(),
            body: None,
            status: None,
            harness_origin: None,
            metadata: None,
        },
    )
    .unwrap()
    .id
}

#[test]
fn register_child_absolutizes_relative_path_against_parent_dir_not_caller_cwd() {
    let dir = TempDir::new().unwrap();
    let parent_dir = dir.path().join("parent");
    let child_dir = dir.path().join("child");
    std::fs::create_dir_all(&parent_dir).unwrap();
    std::fs::create_dir_all(&child_dir).unwrap();

    let parent = WqDb::open(&parent_dir.join("wq.db")).unwrap();
    make_node(&WqDb::open(&child_dir.join("wq.db")).unwrap(), "child node");

    // Register with a RELATIVE path, as a human typing `wq register` from
    // parent_dir would naturally do.
    parent
        .register_child("child", std::path::Path::new("../child/wq.db"))
        .unwrap();

    let entries = parent.list_registered_children().unwrap();
    assert_eq!(entries.len(), 1);
    // The stored path must be absolute — independent of any future caller's
    // cwd — and must actually point at the real child file.
    assert!(
        std::path::Path::new(&entries[0].db_path).is_absolute(),
        "stored db_path must be absolute, got {}",
        entries[0].db_path
    );

    // The real regression test: rollup from a DIFFERENT cwd than parent_dir
    // (simulated here by calling rollup() on a WqDb opened via an absolute
    // path while std::env::current_dir() is left at whatever the test
    // runner's cwd is — proves resolution doesn't depend on process cwd).
    let result = parent.rollup().unwrap();
    assert!(
        result.warnings.is_empty(),
        "relative registration must resolve correctly regardless of process cwd: {:?}",
        result.warnings
    );
    let titles: Vec<&str> = result.nodes.iter().map(|n| n.title.as_str()).collect();
    assert!(titles.contains(&"child node"), "got {titles:?}");
}

#[test]
fn wal_and_busy_timeout_are_configured() {
    // Behavioral concurrency repro is inherently racy (fault-finder's own
    // repro needed an artificially-widened lock window to trigger
    // reliably) — asserting directly on the pragma values is the
    // deterministic version of the same check.
    let dir = TempDir::new().unwrap();
    let db = WqDb::open(&dir.path().join("wq.db")).unwrap();
    let conn = db.connection();

    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        journal_mode.to_lowercase(),
        "wal",
        "journal_mode must be WAL so readers aren't blocked by a writer's transaction"
    );

    let busy_timeout: i64 = conn
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .unwrap();
    assert!(
        busy_timeout > 0,
        "busy_timeout must be set so concurrent writers retry instead of failing immediately, got {busy_timeout}"
    );
}

#[test]
fn query_json_rejects_multi_statement_sql_instead_of_silently_dropping_the_rest() {
    let db = WqDb::open_in_memory().unwrap();
    make_node(&db, "sentinel");

    let before = db.query_json("SELECT COUNT(*) as n FROM nodes").unwrap();
    let count_before = before[0]["n"].as_i64().unwrap();

    let attempt = db.query_json(
        "SELECT 1 as x; INSERT INTO nodes (id,type,title,created_at,updated_at) \
         VALUES ('evil-id','ticket','injected','2020-01-01','2020-01-01')",
    );

    assert!(
        attempt.is_err(),
        "a semicolon-separated multi-statement string must error, not silently run only the first statement"
    );

    let after = db.query_json("SELECT COUNT(*) as n FROM nodes").unwrap();
    let count_after = after[0]["n"].as_i64().unwrap();
    assert_eq!(
        count_before, count_after,
        "the trailing INSERT must never have executed"
    );
}
