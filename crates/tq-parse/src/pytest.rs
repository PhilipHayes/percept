use regex::Regex;
use std::sync::LazyLock;

use crate::model::{TestResult, TestRun, TestStatus, Format};

// "test_file.py::TestClass::test_method PASSED"
// "test_file.py::test_function FAILED"
static PYTEST_RESULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\S+::(?:\S+::)?\S+)\s+(PASSED|FAILED|ERROR|SKIPPED)").unwrap()
});

// "FAILED test_file.py::test_name - AssertionError: message"
static PYTEST_FAILURE_SHORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^FAILED\s+(\S+)\s+-\s+(.+)$").unwrap()
});


// "_______ TestClass.test_name _______" failure section header
static PYTEST_FAILURE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^_{3,}\s+(\S+)\s+_{3,}$").unwrap()
});

/// Parse pytest text output.
pub fn parse_pytest(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut failure_messages: Vec<(String, String)> = Vec::new();

    // Collect failure blocks
    let mut current_failure: Option<(String, Vec<String>)> = None;

    for line in input.lines() {
        // Check for failure block header
        if let Some(caps) = PYTEST_FAILURE_HEADER.captures(line) {
            if let Some((name, lines)) = current_failure.take() {
                failure_messages.push((name, lines.join("\n")));
            }
            current_failure = Some((caps[1].to_string(), Vec::new()));
            continue;
        }

        // Collect failure block content
        if let Some((_, ref mut lines)) = current_failure {
            if line.starts_with("___") || line.starts_with("===") {
                let (name, collected) = current_failure.take().unwrap();
                failure_messages.push((name, collected.join("\n")));
                // Fall through
            } else {
                lines.push(line.to_string());
                continue;
            }
        }

        // Parse verbose result lines: "test_file.py::test_name PASSED"
        if let Some(caps) = PYTEST_RESULT.captures(line) {
            let full_name = caps[1].to_string();
            let status = match &caps[2] {
                "PASSED" => TestStatus::Passed,
                "FAILED" => TestStatus::Failed,
                "ERROR" => TestStatus::Errored,
                "SKIPPED" => TestStatus::Skipped,
                _ => TestStatus::Passed,
            };
            // Extract file from test path (before first ::)
            let file = full_name.split("::").next().map(|s| s.to_string());
            results.push(TestResult {
                name: full_name,
                status,
                duration_ms: None,
                file,
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: None,
            });
            continue;
        }

        // Parse short failure lines: "FAILED test_file.py::test_name - message"
        if let Some(caps) = PYTEST_FAILURE_SHORT.captures(line) {
            let name = caps[1].to_string();
            let msg = caps[2].to_string();
            // Only add if not already captured via verbose output
            if !results.iter().any(|r| r.name == name) {
                let file = name.split("::").next().map(|s| s.to_string());
                results.push(TestResult {
                    name,
                    status: TestStatus::Failed,
                    duration_ms: None,
                    file,
                    line: None,
                    message: Some(msg),
                    stdout: None,
                    stderr: None,
                    suite: None,
                });
            } else {
                // Attach message to existing result
                if let Some(r) = results.iter_mut().find(|r| r.name == name) {
                    r.message = Some(msg);
                }
            }
        }
    }

    // Flush last failure block
    if let Some((name, lines)) = current_failure.take() {
        failure_messages.push((name, lines.join("\n")));
    }

    // Attach failure messages to results by matching test names
    for (fail_name, output) in &failure_messages {
        // pytest failure headers use "TestClass.test_method" but result lines use "file::TestClass::test_method"
        if let Some(result) = results.iter_mut().find(|r| {
            r.name.ends_with(fail_name) || r.name.contains(fail_name)
        }) {
            if result.message.is_none() {
                let trimmed = output.trim();
                let msg = trimmed
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string();
                result.message = Some(msg);
                result.stdout = Some(trimmed.to_string());
            }
        }
    }

    TestRun::from_results(results, Some("pytest".into()), Format::Pytest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_passing_suite() {
        let input = r#"
test_math.py::test_add PASSED
test_math.py::test_sub PASSED
test_math.py::test_mul PASSED

========================= 3 passed in 0.42s =========================
"#;
        let run = parse_pytest(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 3);
        assert_eq!(run.failed, 0);
        assert_eq!(run.runner, Some("pytest".into()));
    }

    #[test]
    fn parse_with_failures() {
        let input = r#"
test_math.py::test_add PASSED
test_math.py::test_div FAILED

=================================== FAILURES ===================================
___________________________________ test_div ___________________________________

    def test_div():
>       assert 1 / 0 == 0
E       ZeroDivisionError: division by zero

test_math.py:10: ZeroDivisionError
=========================== short test summary info ============================
FAILED test_math.py::test_div - ZeroDivisionError: division by zero
========================= 1 failed, 1 passed in 0.05s =========================
"#;
        let run = parse_pytest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures[0].name, "test_math.py::test_div");
        assert!(failures[0].message.as_ref().unwrap().contains("ZeroDivisionError"));
    }

    #[test]
    fn parse_with_skipped() {
        let input = r#"
test_math.py::test_add PASSED
test_math.py::test_slow SKIPPED
test_math.py::test_sub PASSED

========================= 2 passed, 1 skipped in 0.01s =========================
"#;
        let run = parse_pytest(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 2);
        assert_eq!(run.skipped, 1);
    }
}
