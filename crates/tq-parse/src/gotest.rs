use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Format, TestResult, TestRun, TestStatus};

// "--- PASS: TestAdd (0.00s)"
// "--- FAIL: TestDiv (0.01s)"
// "--- SKIP: TestSlow (0.00s)"
static GO_RESULT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---\s+(PASS|FAIL|SKIP):\s+(\S+)\s+\((\d+\.\d+)s\)").unwrap());

// Failure output lines after "--- FAIL:" until next "---" or "==="
static GO_INDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s{4,}(.+)$").unwrap());

/// Parse `go test -v` text output.
pub fn parse_gotest(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut failure_lines: Vec<String> = Vec::new();
    let mut current_fail: Option<String> = None;

    for line in input.lines() {
        // Result line: --- PASS/FAIL/SKIP
        if let Some(caps) = GO_RESULT.captures(line) {
            // Flush previous failure
            if let Some(ref fail_name) = current_fail {
                if let Some(r) = results.iter_mut().find(|r| r.name == *fail_name) {
                    let msg = failure_lines.join("\n").trim().to_string();
                    if !msg.is_empty() {
                        r.message = Some(
                            msg.lines()
                                .find(|l| {
                                    l.contains("Error")
                                        || l.contains("expected")
                                        || l.contains("got")
                                })
                                .unwrap_or(msg.lines().next().unwrap_or(""))
                                .trim()
                                .to_string(),
                        );
                        r.stdout = Some(msg);
                    }
                }
                current_fail = None;
                failure_lines.clear();
            }

            let status = match &caps[1] {
                "PASS" => TestStatus::Passed,
                "FAIL" => TestStatus::Failed,
                "SKIP" => TestStatus::Skipped,
                _ => TestStatus::Passed,
            };
            let name = caps[2].to_string();
            let dur_secs: f64 = caps[3].parse().unwrap_or(0.0);
            let duration_ms = Some((dur_secs * 1000.0) as u64);

            if status == TestStatus::Failed {
                current_fail = Some(name.clone());
            }

            results.push(TestResult {
                name,
                status,
                duration_ms,
                file: None,
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: None,
            });
            continue;
        }

        // Collect indented failure output
        if current_fail.is_some() && GO_INDENT.is_match(line) {
            failure_lines.push(line.trim().to_string());
        }
    }

    // Flush last failure
    if let Some(ref fail_name) = current_fail {
        if let Some(r) = results.iter_mut().find(|r| r.name == *fail_name) {
            let msg = failure_lines.join("\n").trim().to_string();
            if !msg.is_empty() {
                r.message = Some(msg.lines().next().unwrap_or("").trim().to_string());
                r.stdout = Some(msg);
            }
        }
    }

    TestRun::from_results(results, Some("go-test".into()), Format::GoTest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_passing_go() {
        let input = r#"
=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestSub
--- PASS: TestSub (0.01s)
PASS
ok      math    0.012s
"#;
        let run = parse_gotest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 2);
    }

    #[test]
    fn parse_go_failure() {
        let input = r#"
=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestDiv
    math_test.go:15: expected 0, got infinity
--- FAIL: TestDiv (0.01s)
FAIL
"#;
        let run = parse_gotest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures[0].name, "TestDiv");
    }

    #[test]
    fn parse_go_skip() {
        let input = r#"
=== RUN   TestSlow
--- SKIP: TestSlow (0.00s)
"#;
        let run = parse_gotest(input);
        assert_eq!(run.total, 1);
        assert_eq!(run.skipped, 1);
    }
}
