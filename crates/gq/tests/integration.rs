use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

/// Create a temporary git repository with some commits for testing.
fn setup_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    // init + configure
    run_git(path, &["init"]);
    run_git(path, &["config", "user.email", "test@test.com"]);
    run_git(path, &["config", "user.name", "Test User"]);

    // First commit
    std::fs::write(path.join("hello.txt"), "hello world\n").unwrap();
    std::fs::write(path.join("README.md"), "# Test\n\nA test repo.\n").unwrap();
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "Initial commit"]);

    // Second commit
    std::fs::write(path.join("hello.txt"), "hello world\ngoodbye world\n").unwrap();
    std::fs::write(path.join("src.rs"), "fn main() {}\n").unwrap();
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "Add line and src"]);

    // Third commit
    std::fs::write(
        path.join("hello.txt"),
        "hello world\ngoodbye world\nthird line\n",
    )
    .unwrap();
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "Third commit"]);

    dir
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
}

fn gq_cmd() -> Command {
    Command::cargo_bin("gq").unwrap()
}

#[test]
fn test_log() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--log", "-n", "10", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["message"], "Third commit");
    assert!(entries[0]["files"].as_array().unwrap().len() > 0);
}

#[test]
fn test_log_with_count() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--log", "-n", "1", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn test_at() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--at", "HEAD", "-C"])
        .arg(repo.path())
        .arg("hello.txt")
        .output()
        .unwrap();
    assert!(output.status.success());
    let content = String::from_utf8(output.stdout).unwrap();
    assert!(content.contains("third line"));
}

#[test]
fn test_at_previous_rev() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--at", "HEAD~1", "-C"])
        .arg(repo.path())
        .arg("hello.txt")
        .output()
        .unwrap();
    assert!(output.status.success());
    let content = String::from_utf8(output.stdout).unwrap();
    assert!(!content.contains("third line"));
    assert!(content.contains("goodbye world"));
}

#[test]
fn test_diff() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--diff", "HEAD~1..HEAD", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!entries.is_empty());
    assert!(entries[0]["path"].as_str().unwrap() == "hello.txt");
    assert_eq!(entries[0]["insertions"], 1);
}

#[test]
fn test_diff_files_only() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--diff", "HEAD~2..HEAD", "--files-only", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!entries.is_empty());
    // files-only should have status and path, not insertions
    assert!(entries[0].get("status").is_some());
    assert!(entries[0].get("insertions").is_none());
}

#[test]
fn test_blame() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--blame", "hello.txt", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let lines: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["line"], 1);
    assert_eq!(lines[0]["content"], "hello world");
    assert_eq!(lines[0]["author"], "Test User");
}

#[test]
fn test_churn() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--churn", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!entries.is_empty());
    // hello.txt was touched in all 3 commits
    let hello = entries.iter().find(|e| e["path"] == "hello.txt").unwrap();
    assert_eq!(hello["commits"], 3);
}

#[test]
fn test_changed_since() {
    let repo = setup_test_repo();
    let output = gq_cmd()
        .args(["--changed-since", "HEAD~2", "-C"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!entries.is_empty());
    // hello.txt changed in commits since HEAD~2 (third commit touches hello.txt)
    let hello = entries.iter().find(|e| e["path"] == "hello.txt").unwrap();
    assert!(hello["change_count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_not_a_repo() {
    let dir = TempDir::new().unwrap();
    gq_cmd()
        .args(["--log", "-C"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a git repository"));
}

#[test]
fn test_version() {
    gq_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("gq 0.2.0"));
}
