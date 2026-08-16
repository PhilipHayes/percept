use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Format, TestResult, TestRun, TestStatus};

// "test module::test_name ... ok" or "... FAILED" or "... ignored"
static TEST_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)\s*(?:\(([^)]+)\))?$").unwrap()
});

// Failure block delimiter: "---- module::test_name stdout ----"
static FAILURE_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---- (\S+) stdout ----$").unwrap());

// Binary header naming the crate under test. cargo emits one before each
// target's block, and the crate name only appears here — never on the
// `test …` lines — so without it every result from every crate in a
// workspace run is indistinguishable. Two forms:
//   "     Running unittests src/lib.rs (target/debug/deps/wq_core-8a63511)"
//   "   Doc-tests wq-core"
static RUNNING_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*Running\s+.*\(.*[/\\]deps[/\\]([A-Za-z0-9_]+)-[0-9a-f]+").unwrap());
static DOC_TESTS_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*Doc-tests\s+(\S+)$").unwrap());

/// Parse cargo test (libtest) text output.
pub fn parse_libtest(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut failure_outputs: Vec<(String, String)> = Vec::new();
    let mut current_failure: Option<(String, Vec<String>)> = None;
    // Crate named by the most recent binary header; every subsequent test
    // belongs to it until the next header. None until the first one is seen,
    // so output that lacks headers still parses exactly as before.
    let mut current_suite: Option<String> = None;

    for line in input.lines() {
        // Binary headers are only meaningful outside a failure block — a
        // captured stdout could legitimately contain a line like "Running …".
        if current_failure.is_none() {
            if let Some(caps) = RUNNING_HEADER.captures(line) {
                current_suite = Some(caps[1].to_string());
                continue;
            }
            if let Some(caps) = DOC_TESTS_HEADER.captures(line) {
                // cargo prints the package name here (hyphens), while the
                // deps path uses the crate name (underscores). Normalise so a
                // crate's unit and doc tests share one suite.
                current_suite = Some(caps[1].replace('-', "_"));
                continue;
            }
        }

        // Check for failure block start
        if let Some(caps) = FAILURE_HEADER.captures(line) {
            // Save previous failure block
            if let Some((name, lines)) = current_failure.take() {
                failure_outputs.push((name, lines.join("\n")));
            }
            current_failure = Some((caps[1].to_string(), Vec::new()));
            continue;
        }

        // If inside a failure block, collect lines until next delimiter
        if let Some((_, ref mut lines)) = current_failure {
            if line.starts_with("---- ")
                || line.starts_with("failures:")
                || line.starts_with("test result:")
            {
                let (name, collected) = current_failure.take().unwrap();
                failure_outputs.push((name, collected.join("\n")));
                // Fall through to process this line normally
            } else {
                lines.push(line.to_string());
                continue;
            }
        }

        // Parse test result lines
        if let Some(caps) = TEST_LINE.captures(line) {
            let name = caps[1].to_string();
            let status = match &caps[2] {
                "ok" => TestStatus::Passed,
                "FAILED" => TestStatus::Failed,
                "ignored" => TestStatus::Skipped,
                _ => TestStatus::Passed,
            };
            let duration_ms = caps.get(3).and_then(|m| parse_duration_str(m.as_str()));
            results.push(TestResult {
                name,
                status,
                duration_ms,
                file: None,
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: current_suite.clone(),
            });
        }
    }

    // Flush last failure block
    if let Some((name, lines)) = current_failure.take() {
        failure_outputs.push((name, lines.join("\n")));
    }

    // Attach failure messages to their test results
    for (fail_name, output) in &failure_outputs {
        if let Some(result) = results.iter_mut().find(|r| r.name == *fail_name) {
            let trimmed = output.trim();
            if !trimmed.is_empty() {
                // Extract the last meaningful line as the message (often the assertion)
                let msg = extract_failure_message(trimmed);
                result.message = Some(msg);
                result.stdout = Some(trimmed.to_string());
            }
        }
    }

    TestRun::from_results(results, Some("cargo-test".into()), Format::Libtest)
}

/// Extract the most meaningful failure message from captured output.
fn extract_failure_message(output: &str) -> String {
    // Look for assertion lines
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("thread '") && trimmed.contains("panicked at") {
            return trimmed.to_string();
        }
        if trimmed.starts_with("assertion") || trimmed.starts_with("called `") {
            return trimmed.to_string();
        }
    }
    // Fall back to last non-empty line
    output
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(output)
        .trim()
        .to_string()
}

/// Parse a duration string like "0.01s" or "1.234s" into milliseconds.
fn parse_duration_str(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('s');
    s.parse::<f64>().ok().map(|secs| (secs * 1000.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_passing_suite() {
        let input = r#"
running 3 tests
test model::tests::test_a ... ok
test model::tests::test_b ... ok
test model::tests::test_c ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 3);
        assert_eq!(run.failed, 0);
        assert_eq!(run.runner, Some("cargo-test".into()));
        assert_eq!(run.format, Format::Libtest);
    }

    #[test]
    fn parse_with_failures() {
        let input = r#"
running 3 tests
test math::add ... ok
test math::sub ... FAILED
test math::mul ... ok

failures:

---- math::sub stdout ----
thread 'math::sub' panicked at 'assertion failed: `(left == right)`
  left: `5`,
 right: `3`'
note: run with `RUST_BACKTRACE=1` for a backtrace


failures:
    math::sub

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 2);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "math::sub");
        assert!(failures[0].message.is_some());
        assert!(failures[0].stdout.is_some());
    }

    #[test]
    fn parse_with_ignored() {
        let input = r#"
running 2 tests
test expensive_test ... ignored
test quick_test ... ok

test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.skipped, 1);
    }

    #[test]
    fn parse_multiple_crates() {
        let input = r#"
running 2 tests
test parse::tests::test_a ... ok
test parse::tests::test_b ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 1 test
test core::tests::test_c ... FAILED

failures:

---- core::tests::test_c stdout ----
assertion failed: expected true

failures:
    core::tests::test_c

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 2);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures[0].name, "core::tests::test_c");
    }

    #[test]
    fn running_header_assigns_crate_as_suite() {
        let input = r#"
     Running unittests src/lib.rs (target/debug/deps/tq_core-8a635111c9254a31)

running 2 tests
test diff::tests::diff_new_pass ... ok
test flaky::tests::detect_flaky_test ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.total, 2);
        assert!(run.tests.iter().all(|t| t.suite.as_deref() == Some("tq_core")));
    }

    #[test]
    fn suite_switches_at_each_binary_header() {
        let input = r#"
     Running unittests src/lib.rs (target/debug/deps/wq_core-1111111111111111)

running 1 test
test path::tests::walks_up ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli.rs (target/debug/deps/wq_cli-2222222222222222)

running 1 test
test emits_json ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.tests[0].suite.as_deref(), Some("wq_core"));
        assert_eq!(run.tests[1].suite.as_deref(), Some("wq_cli"));
    }

    #[test]
    fn doc_tests_header_normalises_package_name_to_crate_name() {
        // cargo prints the *package* name (hyphens) on this header, unlike the
        // deps path which carries the crate name (underscores).
        let input = r#"
   Doc-tests tq-parse

running 1 test
test lib::tests::a_normal_looking_test ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.tests[0].suite.as_deref(), Some("tq_parse"));
    }

    #[test]
    fn doc_test_result_lines_are_not_parsed() {
        // Documents a pre-existing limitation rather than asserting it is good:
        // TEST_LINE requires a whitespace-free name, and libtest prints doc
        // tests as "src/lib.rs - item (line N)". They are silently dropped, so
        // a doc test can never back a `test_named` claim. Recognising the
        // Doc-tests header is still worthwhile — it stops the previous crate's
        // suite from leaking onto anything that follows.
        let input = r#"
   Doc-tests tq-parse

running 1 test
test src/lib.rs - parse_output (line 12) ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.total, 0, "doc-test result lines are not yet parsed");
    }

    #[test]
    fn suite_is_none_when_output_has_no_binary_header() {
        // Pre-existing behaviour: bare libtest output still parses, with no suite.
        let input = r#"
running 1 test
test core::tests::test_c ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.tests[0].suite, None);
    }

    #[test]
    fn running_line_inside_failure_output_does_not_change_suite() {
        // A test that prints "Running ..." must not be mistaken for a header.
        let input = r#"
     Running unittests src/lib.rs (target/debug/deps/wq_core-1111111111111111)

running 1 test
test exec::tests::spawns ... FAILED

failures:

---- exec::tests::spawns stdout ----
     Running unittests src/lib.rs (target/debug/deps/impostor-9999999999999999)
assertion failed

failures:
    exec::tests::spawns

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#;
        let run = parse_libtest(input);
        assert_eq!(run.tests.len(), 1);
        assert_eq!(run.tests[0].suite.as_deref(), Some("wq_core"));
    }
}
