//! wq-4.1 RED — wq-cli integration tests (8 scenarios, assert_cmd).
//!
//! Every invocation runs inside a fresh TempDir (its own .agents/wq.db) and
//! passes FASTEMBED_CACHE_DIR through — fastembed's default cache is
//! cwd-relative, and cwd here is a TempDir (see the wq-4 plan drift note).
//! Registration for the rollup test uses wq-core directly: ADR-038's CLI
//! surface has no `wq register` command (flagged as FU-wq-4-2).

use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

fn fastembed_cache() -> String {
    let home = std::env::var("HOME").expect("HOME set");
    format!("{home}/.cache/fastembed")
}

/// A `wq` invocation rooted in `dir` with the model cache passed through.
fn wq(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("wq").unwrap();
    cmd.current_dir(dir)
        .env("FASTEMBED_CACHE_DIR", fastembed_cache());
    cmd
}

fn parse_stdout(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be valid JSON ({e}): {:?}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Creates a ticket, returning its id.
fn seed_ticket(dir: &Path, title: &str) -> String {
    let output = wq(dir)
        .args([
            "ticket", "create", "--type", "ticket", "--title", title, "--status", "open",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "seed failed: {output:?}");
    parse_stdout(&output)["id"].as_str().unwrap().to_string()
}

#[test]
fn ticket_create_prints_json_with_generated_id() {
    let dir = TempDir::new().unwrap();
    let output = wq(dir.path())
        .args([
            "ticket",
            "create",
            "--type",
            "ticket",
            "--title",
            "First ticket",
            "--status",
            "open",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    assert!(!json["id"].as_str().unwrap_or("").is_empty());
    assert_eq!(json["title"], "First ticket");
    assert_eq!(json["status"], "open");
}

#[test]
fn ticket_update_changes_status_and_prints_updated_json() {
    let dir = TempDir::new().unwrap();
    let id = seed_ticket(dir.path(), "To update");

    let output = wq(dir.path())
        .args(["ticket", "update", &id, "--status", "in_progress"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    assert_eq!(json["id"], id.as_str());
    assert_eq!(json["status"], "in_progress");
}

#[test]
fn ticket_update_unknown_id_exits_nonzero_with_json_error() {
    let dir = TempDir::new().unwrap();
    seed_ticket(dir.path(), "irrelevant"); // DB exists, id doesn't

    let output = wq(dir.path())
        .args(["ticket", "update", "no-such-id", "--status", "done"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "unknown id must exit non-zero");
    // Error contract (pinned in GREEN): JSON object with an "error" key on stderr.
    let err: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap_or_else(|e| {
        panic!(
            "stderr must carry a JSON error object ({e}): {:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(err["error"].as_str().unwrap_or("").contains("no-such-id"));
}

#[test]
fn query_escape_hatch_returns_json_array_of_rows() {
    let dir = TempDir::new().unwrap();
    seed_ticket(dir.path(), "Queryable ticket");

    let output = wq(dir.path())
        .args(["query", "SELECT title FROM nodes"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    let rows = json.as_array().expect("query output is a JSON array");
    assert!(rows.iter().any(|r| r["title"] == "Queryable ticket"));
}

#[test]
fn search_respects_k_flag() {
    let dir = TempDir::new().unwrap();
    seed_ticket(dir.path(), "Fix login authentication");
    seed_ticket(dir.path(), "Water the garden");

    let output = wq(dir.path())
        .args(["search", "password sign-in problems", "--k", "1"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    let results = json.as_array().expect("search output is a JSON array");
    assert_eq!(results.len(), 1, "--k 1 returns exactly one result");
}

#[test]
fn edge_create_and_traverse_round_trip() {
    let dir = TempDir::new().unwrap();
    let a = seed_ticket(dir.path(), "Blocker");
    let b = seed_ticket(dir.path(), "Blocked");

    let output = wq(dir.path())
        .args(["edge", "create", &a, &b, "--kind", "blocks"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let edge = parse_stdout(&output);
    assert_eq!(edge["from_id"], a.as_str());
    assert_eq!(edge["to_id"], b.as_str());

    let output = wq(dir.path())
        .args(["traverse", &a, "--kind", "blocks", "--depth", "1"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    let hops = json.as_array().expect("traverse output is a JSON array");
    assert!(
        hops.iter().any(|h| h["node"]["id"] == b.as_str()),
        "traverse must include the blocked node, got: {json}"
    );
}

#[test]
fn rollup_global_includes_registered_child_nodes() {
    let project = TempDir::new().unwrap();
    let global_dir = TempDir::new().unwrap();
    let global_db = global_dir.path().join("global.db");

    seed_ticket(project.path(), "Project-scoped node");

    // Register the project DB as a child of the (test-scoped) global DB via
    // wq-core — no CLI registration command exists in ADR-038's surface.
    {
        let parent = wq_core::WqDb::open(&global_db).unwrap();
        let child_db = project.path().join(".agents").join("wq.db");
        parent.register_child("testproj", &child_db).unwrap();
    }

    let output = wq(project.path())
        .args(["rollup", "--global"])
        .env("WQ_GLOBAL_DB_PATH", &global_db)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    let titles: Vec<&str> = json["nodes"]
        .as_array()
        .expect("rollup output has a nodes array")
        .iter()
        .filter_map(|n| n["title"].as_str())
        .collect();
    assert!(
        titles.contains(&"Project-scoped node"),
        "global rollup must surface the registered child's nodes, got {titles:?}"
    );
}

#[test]
fn harness_flag_defaults_to_cli_when_omitted() {
    let dir = TempDir::new().unwrap();
    seed_ticket(dir.path(), "Attribution check");

    let output = wq(dir.path())
        .args(["query", "SELECT harness_origin FROM nodes"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let json = parse_stdout(&output);
    assert_eq!(
        json.as_array().unwrap()[0]["harness_origin"],
        "cli",
        "omitted --harness must default to \"cli\" (Q-wq-4-1)"
    );
}
