use regex::Regex;
use std::sync::LazyLock;

use crate::model::{TestResult, TestRun, TestStatus, Format};

// "ok 1 - test description"
// "not ok 2 - test description"
// "ok 3 # SKIP reason"
// "ok 4 # TODO not yet implemented"
static TAP_RESULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(ok|not ok)\s+(\d+)\s*(?:-\s+(.+?))?(?:\s*#\s*(SKIP|TODO|skip|todo)\s*(.*)?)?$").unwrap()
});


// "Bail out!" — abort
static TAP_BAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Bail out!\s*(.*)$").unwrap()
});

// "# Diagnostic message" — YAML-like diagnostic (TAP 13+)
static TAP_DIAGNOSTIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#\s+(.+)$").unwrap()
});

/// Parse TAP (Test Anything Protocol) output.
pub fn parse_tap(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut last_fail: Option<usize> = None;
    let mut diagnostics: Vec<String> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();

        // Bail out
        if let Some(caps) = TAP_BAIL.captures(trimmed) {
            let msg = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            results.push(TestResult {
                name: "Bail out!".to_string(),
                status: TestStatus::Errored,
                duration_ms: None,
                file: None,
                line: None,
                message: if msg.is_empty() { None } else { Some(msg) },
                stdout: None,
                stderr: None,
                suite: None,
            });
            break;
        }

        // Test result line
        if let Some(caps) = TAP_RESULT.captures(trimmed) {
            // Flush diagnostics to previous failure
            if let Some(idx) = last_fail {
                if !diagnostics.is_empty() {
                    results[idx].stdout = Some(diagnostics.join("\n"));
                    if results[idx].message.is_none() {
                        results[idx].message = diagnostics.last().cloned();
                    }
                    diagnostics.clear();
                }
            }
            last_fail = None;

            let ok = &caps[1] == "ok";
            let num: u32 = caps[2].parse().unwrap_or(0);
            let description = caps.get(3).map(|m| m.as_str().to_string());
            let directive = caps.get(4).map(|m| m.as_str().to_uppercase());

            let status = match directive.as_deref() {
                Some("SKIP") | Some("TODO") => TestStatus::Skipped,
                _ if ok => TestStatus::Passed,
                _ => TestStatus::Failed,
            };

            let name = description.unwrap_or_else(|| format!("test {}", num));

            if status == TestStatus::Failed {
                last_fail = Some(results.len());
            }

            results.push(TestResult {
                name,
                status,
                duration_ms: None,
                file: None,
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: None,
            });
            continue;
        }

        // Diagnostic line after a failure
        if last_fail.is_some() {
            if let Some(caps) = TAP_DIAGNOSTIC.captures(trimmed) {
                diagnostics.push(caps[1].to_string());
            }
        }
    }

    // Flush trailing diagnostics
    if let Some(idx) = last_fail {
        if !diagnostics.is_empty() {
            results[idx].stdout = Some(diagnostics.join("\n"));
            if results[idx].message.is_none() {
                results[idx].message = diagnostics.last().cloned();
            }
        }
    }

    TestRun::from_results(results, Some("tap".into()), Format::Tap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tap_passing() {
        let input = r#"1..3
ok 1 - addition works
ok 2 - subtraction works
ok 3 - multiplication works
"#;
        let run = parse_tap(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 3);
    }

    #[test]
    fn parse_tap_with_failure() {
        let input = r#"1..2
ok 1 - addition
not ok 2 - division
# expected 0
# got Infinity
"#;
        let run = parse_tap(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        assert!(run.tests[1].message.is_some());
    }

    #[test]
    fn parse_tap_skip_todo() {
        let input = r#"1..3
ok 1 - basic test
ok 2 - slow test # SKIP too slow
not ok 3 - future test # TODO not implemented
"#;
        let run = parse_tap(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 1);
        assert_eq!(run.skipped, 2);
    }

    #[test]
    fn parse_tap_bail() {
        let input = r#"1..5
ok 1 - first
Bail out! Database unavailable
"#;
        let run = parse_tap(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.errored, 1);
    }
}
