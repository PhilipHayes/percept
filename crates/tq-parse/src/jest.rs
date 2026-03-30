use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Format, TestResult, TestRun, TestStatus};

// "  ✓ should add numbers (5ms)"
// "  ✕ should divide by zero (3ms)"
// "  ○ skipped pending test"
static JEST_PASS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[✓✔√]\s+(.+?)(?:\s+\((\d+)\s*ms\))?\s*$").unwrap());

static JEST_FAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[✕✗×]\s+(.+?)(?:\s+\((\d+)\s*ms\))?\s*$").unwrap());

static JEST_SKIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[○◌]\s+(skipped\s+)?(.+)$").unwrap());

// "  ● should divide by zero" — failure detail header
static JEST_FAILURE_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*●\s+(.+)$").unwrap());

// "Tests:   1 failed, 2 passed, 3 total"
static JEST_SUMMARY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Tests:\s+(.+)total").unwrap());

// "PASS src/math.test.js" or "FAIL src/math.test.js"
static JEST_SUITE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(PASS|FAIL)\s+(.+)$").unwrap());

/// Parse Jest text output.
pub fn parse_jest(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut current_suite: Option<String> = None;
    let mut failure_blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_failure: Option<(String, Vec<String>)> = None;

    for line in input.lines() {
        // Suite header
        if let Some(caps) = JEST_SUITE.captures(line) {
            current_suite = Some(caps[2].trim().to_string());
            continue;
        }

        // Failure detail header "● test name"
        if let Some(caps) = JEST_FAILURE_HEADER.captures(line) {
            if let Some(prev) = current_failure.take() {
                failure_blocks.push(prev);
            }
            current_failure = Some((caps[1].trim().to_string(), Vec::new()));
            continue;
        }

        // Collecting failure body
        if let Some((_, ref mut lines)) = current_failure {
            if line.trim().is_empty() && lines.is_empty() {
                continue; // skip leading blank
            }
            if JEST_PASS.is_match(line) || JEST_FAIL.is_match(line) || JEST_SUMMARY.is_match(line) {
                let prev = current_failure.take().unwrap();
                failure_blocks.push(prev);
                // fall through to parse the line below
            } else {
                lines.push(line.to_string());
                continue;
            }
        }

        // Passing test
        if let Some(caps) = JEST_PASS.captures(line) {
            let name = caps[1].trim().to_string();
            let dur = caps.get(2).and_then(|m| m.as_str().parse::<u64>().ok());
            results.push(TestResult {
                name,
                status: TestStatus::Passed,
                duration_ms: dur,
                file: current_suite.clone(),
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: current_suite.clone(),
            });
            continue;
        }

        // Failing test
        if let Some(caps) = JEST_FAIL.captures(line) {
            let name = caps[1].trim().to_string();
            let dur = caps.get(2).and_then(|m| m.as_str().parse::<u64>().ok());
            results.push(TestResult {
                name,
                status: TestStatus::Failed,
                duration_ms: dur,
                file: current_suite.clone(),
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: current_suite.clone(),
            });
            continue;
        }

        // Skipped test
        if let Some(caps) = JEST_SKIP.captures(line) {
            let name = caps[2].trim().to_string();
            results.push(TestResult {
                name,
                status: TestStatus::Skipped,
                duration_ms: None,
                file: current_suite.clone(),
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: current_suite.clone(),
            });
        }
    }

    // Flush last failure
    if let Some(prev) = current_failure.take() {
        failure_blocks.push(prev);
    }

    // Attach failure messages
    for (fail_name, lines) in &failure_blocks {
        if let Some(result) = results
            .iter_mut()
            .find(|r| r.name == *fail_name || fail_name.ends_with(&r.name))
        {
            let body = lines.join("\n").trim().to_string();
            if result.message.is_none() && !body.is_empty() {
                let msg = body
                    .lines()
                    .find(|l| l.contains("expect") || l.contains("Error") || l.contains("assert"))
                    .unwrap_or(body.lines().next().unwrap_or(""))
                    .trim()
                    .to_string();
                result.message = Some(msg);
                result.stdout = Some(body);
            }
        }
    }

    TestRun::from_results(results, Some("jest".into()), Format::Jest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_passing_jest() {
        let input = r#"
PASS src/math.test.js
  Math
    ✓ should add numbers (5ms)
    ✓ should subtract numbers (2ms)

Tests:   2 passed, 2 total
"#;
        let run = parse_jest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 2);
        assert_eq!(run.tests[0].duration_ms, Some(5));
    }

    #[test]
    fn parse_jest_with_failure() {
        let input = r#"
FAIL src/math.test.js
  Math
    ✓ should add (1ms)
    ✕ should divide (3ms)

  ● should divide

    expect(received).toBe(expected)

    Expected: 0
    Received: Infinity

Tests:   1 failed, 1 passed, 2 total
"#;
        let run = parse_jest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures[0].name, "should divide");
        assert!(failures[0].message.as_ref().unwrap().contains("expect"));
    }

    #[test]
    fn parse_jest_with_skipped() {
        let input = r#"
PASS src/math.test.js
  Math
    ✓ should add (1ms)
    ○ skipped pending test

Tests:   1 passed, 1 skipped, 2 total
"#;
        let run = parse_jest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.skipped, 1);
    }
}
