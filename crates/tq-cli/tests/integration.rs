use std::process::Command;

fn tq_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tq"))
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn summary_cargo_test() {
    let output = tq_bin()
        .args(["--format", "libtest", "--summary", &fixture("cargo-test.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 3);
    assert_eq!(json["failed"], 1);
}

#[test]
fn summary_libtest_json() {
    let output = tq_bin()
        .args(["--format", "libtest-json", "--summary", &fixture("cargo-test.json")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 3);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);
}

#[test]
fn summary_pytest() {
    let output = tq_bin()
        .args(["--format", "pytest", "--summary", &fixture("pytest.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 3);
    assert_eq!(json["failed"], 1);
}

#[test]
fn summary_junit() {
    let output = tq_bin()
        .args(["--format", "junit", "--summary", &fixture("junit.xml")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);
    assert_eq!(json["skipped"], 1);
}

#[test]
fn autodetect_format() {
    // Feed cargo test fixture without --format; should auto-detect libtest
    let output = tq_bin()
        .args(["--summary", &fixture("cargo-test.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 3);
}

#[test]
fn budget_truncates_output() {
    // Use a very small budget to force truncation
    let output = tq_bin()
        .args(["--budget", "5", "--summary", &fixture("cargo-test.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("truncated"), "expected truncation message, got: {}", stdout);
}

#[test]
fn stdin_input() {
    use std::process::Stdio;
    use std::io::Write;

    let fixture_content = std::fs::read_to_string(fixture("cargo-test.txt")).unwrap();
    let mut child = tq_bin()
        .args(["--format", "libtest", "--summary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn tq");
    child.stdin.take().unwrap().write_all(fixture_content.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["failed"], 1);
}

#[test]
fn full_output_without_summary() {
    let output = tq_bin()
        .args(["--format", "libtest", &fixture("cargo-test.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    // Full output includes the tests array
    assert!(json["tests"].is_array());
    assert_eq!(json["tests"].as_array().unwrap().len(), 4);
}

#[test]
fn save_and_diff() {
    let dir = tempfile::tempdir().unwrap();
    let before_path = dir.path().join("before.json");
    let after_path = dir.path().join("after.json");

    // Save before (cargo-test.txt has 3 pass, 1 fail)
    let output = tq_bin()
        .args([
            "--format", "libtest",
            "--save", before_path.to_str().unwrap(),
            "--summary",
            &fixture("cargo-test.txt"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Save after (pytest.txt has 3 pass, 1 fail — different tests)
    // For a real diff we need the same tests, so let's save the same fixture twice
    // and just verify the diff runs
    let output = tq_bin()
        .args([
            "--format", "libtest",
            "--save", after_path.to_str().unwrap(),
            "--summary",
            &fixture("cargo-test.txt"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Now diff
    let output = tq_bin()
        .args([
            "--diff",
            before_path.to_str().unwrap(),
            after_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON diff output");
    // Same file diffed with itself → no new failures/passes
    assert!(json["new_failures"].as_array().unwrap().is_empty());
    assert!(json["new_passes"].as_array().unwrap().is_empty());
    assert_eq!(json["still_failing"].as_array().unwrap().len(), 1); // test_div
}

#[test]
fn flaky_detection() {
    let dir = tempfile::tempdir().unwrap();

    // Create two slightly different runs by saving the same file
    // In real usage these would be different runs
    let run1 = dir.path().join("run1.json");
    let run2 = dir.path().join("run2.json");

    let output = tq_bin()
        .args([
            "--format", "libtest",
            "--save", run1.to_str().unwrap(),
            "--summary",
            &fixture("cargo-test.txt"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = tq_bin()
        .args([
            "--format", "libtest",
            "--save", run2.to_str().unwrap(),
            "--summary",
            &fixture("cargo-test.txt"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = tq_bin()
        .args([
            "--flaky",
            run1.to_str().unwrap(),
            run2.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON flaky output");
    assert_eq!(json["total_runs"], 2);
    // test_div fails in both runs → always_failing, not flaky
    assert_eq!(json["always_failing"].as_array().unwrap().len(), 1);
    assert!(json["flaky_tests"].as_array().unwrap().is_empty());
}

#[test]
fn summary_jest() {
    let output = tq_bin()
        .args(["--format", "jest", "--summary", &fixture("jest.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);
    assert_eq!(json["skipped"], 1);
}

#[test]
fn summary_gotest() {
    let output = tq_bin()
        .args(["--format", "gotest", "--summary", &fixture("gotest.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);
    assert_eq!(json["skipped"], 1);
}

#[test]
fn summary_gotest_json() {
    let output = tq_bin()
        .args(["--format", "gotest-json", "--summary", &fixture("gotest.json")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 2);
    assert_eq!(json["passed"], 1);
    assert_eq!(json["failed"], 1);
}

#[test]
fn summary_tap() {
    let output = tq_bin()
        .args(["--format", "tap", "--summary", &fixture("tap.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 4);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);
    assert_eq!(json["skipped"], 1);
}

#[test]
fn summary_flutter() {
    let output = tq_bin()
        .args(["--format", "flutter", "--summary", &fixture("flutter.txt")])
        .output()
        .expect("failed to run tq");
    assert!(output.status.success(), "tq exited with error: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["total"], 3);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);
}
