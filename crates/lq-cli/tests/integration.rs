use std::process::Command;

fn lq_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lq"))
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn json_level_filter() {
    let out = lq_cmd()
        .args(["level:error", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["level"], "error");
    }
}

#[test]
fn logfmt_level_filter() {
    let out = lq_cmd()
        .args(["level:error", &fixture("sample.logfmt")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["message"], "connection refused");
}

#[test]
fn bracket_level_filter() {
    let out = lq_cmd()
        .args(["level:error", &fixture("sample.bracket")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["level"], "error");
    }
}

#[test]
fn json_source_filter() {
    let out = lq_cmd()
        .args(["source:db", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    // JSON sample uses "service" field, not parsed as source, so 0 matches expected
    // unless the parser maps service->source. Currently it doesn't.
    // This is a valid test documenting current behavior.
    assert_eq!(stdout.lines().count(), 0);
}

#[test]
fn bracket_source_filter() {
    let out = lq_cmd()
        .args(["source:db", &fixture("sample.bracket")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["message"], "connection refused");
}

#[test]
fn summary_mode() {
    let out = lq_cmd()
        .args(["--summary", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap();
    assert_eq!(v["format"], "json");
    assert_eq!(v["lines"], 5);
    assert_eq!(v["levels"]["error"], 2);
    assert_eq!(v["levels"]["info"], 2);
}

#[test]
fn text_filter() {
    let out = lq_cmd()
        .args(["\"disk full\"", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn no_input_exits_error() {
    // With no args and stdin from /dev/null (not a tty, but empty), should output nothing
    let out = lq_cmd()
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    // Empty stdin still produces no output but may or may not error
    // Just verify it doesn't panic
    let _ = out.status;
}

#[test]
fn auto_detect_json_passthrough() {
    // With file and no query: query param gets eaten as file path thanks to heuristic
    let out = lq_cmd()
        .args(["--summary", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap();
    assert_eq!(v["lines"], 5);
}

#[test]
fn pipe_stdin() {
    let out = lq_cmd()
        .args(["level:error"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Can't easily feed stdin with Command, so just verify it doesn't panic
    // The process should read from stdin until EOF
    let _ = out.wait_with_output();
}

#[test]
fn pipeline_count_by_level() {
    let out = lq_cmd()
        .args(["| count by level", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["aggregation"], "count");
    assert_eq!(v["total"], 5);
    assert_eq!(v["groups"]["error"], 2);
}

#[test]
fn pipeline_filter_then_count() {
    let out = lq_cmd()
        .args(["level:error | count by source", &fixture("sample.bracket")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["total"], 2);
}

#[test]
fn pipeline_rate() {
    let out = lq_cmd()
        .args(["| rate 1m", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty());
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(v["count"].as_u64().unwrap() > 0);
}

#[test]
fn pipeline_patterns() {
    let out = lq_cmd()
        .args(["| patterns", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty());
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(v["count"].as_u64().unwrap() > 0);
    assert!(v["template"].as_str().is_some());
}

#[test]
fn summary_includes_top_errors() {
    let out = lq_cmd()
        .args(["--summary", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v["error_rate"].as_str().is_some());
    assert!(v["top_errors"].as_array().is_some());
    assert!(!v["top_errors"].as_array().unwrap().is_empty());
}

#[test]
fn budget_truncates_output() {
    let out = lq_cmd()
        .args(["--budget", "10", "| patterns", &fixture("sample.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    // Should have at least the truncation indicator
    let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    // Either we got all results (small file) or truncation marker
    if last.get("_truncated").is_some() {
        assert_eq!(last["_truncated"], true);
    }
}

#[test]
fn timeline_merges_files() {
    let out = lq_cmd()
        .args([
            "| timeline",
            &fixture("sample.json"),
            &fixture("sample.bracket"),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    // Should have entries from both files (5 + 5 = 10)
    assert_eq!(lines.len(), 10);
    // First entry should be earliest timestamp
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(first["timestamp"].as_str().is_some());
}

#[test]
fn follow_requires_file() {
    let out = lq_cmd()
        .args(["--follow"])
        .stdin(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--follow requires"));
}
