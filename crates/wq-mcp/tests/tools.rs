//! wq-5.1 RED — wq-mcp tool tests: 7 tools + harness_origin (8 tests).
//!
//! Tests call handle_tool_call directly (bare-value contract) — the JSON-RPC
//! envelope is the serve loop's concern, mirroring canopy's testing split.
//! Each test gets its own TempDir project root; embedding calls hit the
//! local fastembed cache (cwd never changes, so no cache-dir trap here).

use serde_json::{json, Value};
use tempfile::TempDir;
use wq_mcp::{handle_tool_call, ServerState};

fn state_in(dir: &TempDir) -> ServerState {
    ServerState::with_harness(dir.path().to_path_buf(), "test-harness")
}

fn create_ticket(state: &mut ServerState, title: &str) -> Value {
    handle_tool_call(
        state,
        "wq_ticket_create",
        &json!({"type": "ticket", "title": title, "status": "open"}),
    )
    .expect("ticket create succeeds")
}

#[test]
fn wq_ticket_create_tool_returns_node_with_id() {
    let dir = TempDir::new().unwrap();
    let mut state = state_in(&dir);

    let node = create_ticket(&mut state, "MCP-born ticket");
    assert!(!node["id"].as_str().unwrap_or("").is_empty());
    assert_eq!(node["title"], "MCP-born ticket");
    assert_eq!(node["status"], "open");
}

#[test]
fn wq_ticket_update_tool_changes_status() {
    let dir = TempDir::new().unwrap();
    let mut state = state_in(&dir);
    let id = create_ticket(&mut state, "To update")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let updated = handle_tool_call(
        &mut state,
        "wq_ticket_update",
        &json!({"id": id, "status": "in_progress"}),
    )
    .expect("update succeeds");
    assert_eq!(updated["status"], "in_progress");
    assert_eq!(updated["id"], id.as_str());
}

#[test]
fn wq_ticket_update_tool_unknown_id_returns_structured_error() {
    let dir = TempDir::new().unwrap();
    let mut state = state_in(&dir);
    create_ticket(&mut state, "exists"); // DB present, id absent

    let result = handle_tool_call(
        &mut state,
        "wq_ticket_update",
        &json!({"id": "no-such-id", "status": "done"}),
    );
    let err = result.expect_err("unknown id must be an Err, not a panic");
    assert!(err.contains("no-such-id"), "error names the id: {err}");
}

#[test]
fn wq_query_tool_returns_json_rows() {
    let dir = TempDir::new().unwrap();
    let mut state = state_in(&dir);
    create_ticket(&mut state, "Queryable");

    let rows = handle_tool_call(
        &mut state,
        "wq_query",
        &json!({"sql": "SELECT title FROM nodes"}),
    )
    .expect("query succeeds");
    let rows = rows.as_array().expect("array of row objects");
    assert!(rows.iter().any(|r| r["title"] == "Queryable"));
}

#[test]
fn wq_search_tool_respects_k_param() {
    let dir = TempDir::new().unwrap();
    let mut state = state_in(&dir);
    create_ticket(&mut state, "Fix login authentication flow");
    create_ticket(&mut state, "Water the garden on Tuesdays");

    let results = handle_tool_call(
        &mut state,
        "wq_search",
        &json!({"text": "password sign-in problems", "k": 1}),
    )
    .expect("search succeeds");
    assert_eq!(
        results.as_array().expect("array").len(),
        1,
        "k=1 returns exactly one result"
    );
}

#[test]
fn wq_edge_create_and_wq_traverse_round_trip() {
    let dir = TempDir::new().unwrap();
    let mut state = state_in(&dir);
    let a = create_ticket(&mut state, "Blocker")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = create_ticket(&mut state, "Blocked")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let edge = handle_tool_call(
        &mut state,
        "wq_edge_create",
        &json!({"from_id": a, "to_id": b, "kind": "blocks"}),
    )
    .expect("edge create succeeds");
    assert_eq!(edge["from_id"], a.as_str());

    let hops = handle_tool_call(
        &mut state,
        "wq_traverse",
        &json!({"id": a, "kind": "blocks", "depth": 1}),
    )
    .expect("traverse succeeds");
    assert!(
        hops.as_array()
            .expect("array")
            .iter()
            .any(|h| h["node"]["id"] == b.as_str()),
        "traverse surfaces the blocked node: {hops}"
    );
}

#[test]
fn wq_rollup_tool_returns_cross_db_results() {
    let project = TempDir::new().unwrap();
    let global_dir = TempDir::new().unwrap();
    let global_db = global_dir.path().join("global.db");

    // Seed the project DB via the MCP tool, then register it under the
    // test-scoped global DB via wq-core (no register tool — FU-wq-4-2).
    let mut project_state = state_in(&project);
    create_ticket(&mut project_state, "Project-scoped node");
    {
        let parent = wq_core::WqDb::open(&global_db).unwrap();
        let child_db = project.path().join(".agents").join("wq.db");
        parent.register_child("testproj", &child_db).unwrap();
    }

    // A rollup with global=true resolves through WQ_GLOBAL_DB_PATH; pass
    // the path explicitly instead via the tool's global_db_path arg? No —
    // contract: env var. Tests set it process-wide; serial by nature of
    // being the only test that touches it.
    std::env::set_var("WQ_GLOBAL_DB_PATH", &global_db);
    let result = handle_tool_call(&mut project_state, "wq_rollup", &json!({"global": true}));
    std::env::remove_var("WQ_GLOBAL_DB_PATH");

    let rolled = result.expect("rollup succeeds");
    let titles: Vec<&str> = rolled["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter_map(|n| n["title"].as_str())
        .collect();
    assert!(
        titles.contains(&"Project-scoped node"),
        "global rollup surfaces the registered child's nodes: {titles:?}"
    );
}

#[test]
fn harness_origin_is_attributed_per_wq_5_0_mechanism() {
    let dir = TempDir::new().unwrap();
    // Per-process attribution: state carries the identity read at launch
    // (WQ_HARNESS_ORIGIN in production; injected directly here).
    let mut state = ServerState::with_harness(dir.path().to_path_buf(), "copilot-chat");

    let node = create_ticket(&mut state, "Attributed ticket");
    assert_eq!(
        node["harness_origin"], "copilot-chat",
        "created nodes carry the server's per-process harness identity"
    );
}
