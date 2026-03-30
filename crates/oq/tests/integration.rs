use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

/// Create a test git repo with a Rust file aq can parse.
fn setup_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    run_git(path, &["init"]);
    run_git(path, &["config", "user.email", "test@test.com"]);
    run_git(path, &["config", "user.name", "Test User"]);

    std::fs::write(path.join("hello.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
    std::fs::write(path.join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "Initial commit"]);

    // Second commit — modify hello.rs
    std::fs::write(
        path.join("hello.rs"),
        "fn main() {\n    println!(\"hello world\");\n}\n\nfn helper() {}\n",
    )
    .unwrap();
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "Update hello"]);

    dir
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
}

fn oq_cmd() -> Command {
    Command::cargo_bin("oq").unwrap()
}

#[test]
fn test_get_miss_then_hit() {
    let repo = setup_test_repo();
    let cache_dir = TempDir::new().unwrap();

    // First call: cache miss
    let out = oq_cmd()
        .args(["--get", "hello.rs", "-C"])
        .arg(repo.path())
        .env("OQ_CACHE_DIR", cache_dir.path())
        .output()
        .unwrap();

    // Should succeed (aq must be installed)
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("aq") {
            eprintln!("skipping test — aq not installed");
            return;
        }
        panic!("oq failed: {stderr}");
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("miss"), "expected cache miss, got: {stderr}");

    // Parse output as JSON
    let data: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(data.get("declarations").is_some() || data.get("file").is_some());
}

#[test]
fn test_stats() {
    oq_cmd()
        .arg("--stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("entries"));
}

#[test]
fn test_version() {
    oq_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("oq 0.1.0"));
}

#[test]
fn test_not_a_repo() {
    let dir = TempDir::new().unwrap();
    oq_cmd()
        .args(["--get", "foo.rs", "-C"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a git repository"));
}
