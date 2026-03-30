use regex::Regex;
use std::sync::LazyLock;

use crate::model::{TestResult, TestRun, TestStatus, Format};

// "00:05 +3: test widget renders correctly"
// "00:05 +3 -1: test widget fails to render"
// "00:05 +3 ~1: test widget skipped"
static FLUTTER_RESULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{2}:\d{2})\s+\+(\d+)(?:\s+-(\d+))?(?:\s+~(\d+))?\s*:\s+(.+)$"
    ).unwrap()
});

// "  test widget renders correctly" (final summary line with status)

/// Parse `flutter test` output.
/// Flutter test output uses an incremental counter format:
/// "00:01 +1: test description" (passing)
/// "00:01 +1 -1: test description" (failing, shown when failure occurs)
pub fn parse_flutter(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut seen_tests: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_passed = 0u32;
    let mut last_failed = 0u32;

    // Flutter output is incremental — each line shows cumulative state.
    // We collect the test name from each line and determine status by
    // watching the counter changes.
    for line in input.lines() {
        if let Some(caps) = FLUTTER_RESULT.captures(line) {
            let cur_passed: u32 = caps[2].parse().unwrap_or(0);
            let cur_failed: u32 = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let test_name = caps[5].trim().to_string();

            // Skip "loading" and summary lines
            if test_name.starts_with("loading")
                || test_name == "All tests passed!"
                || test_name.starts_with("Some tests failed")
            {
                last_passed = cur_passed;
                last_failed = cur_failed;
                continue;
            }

            if seen_tests.contains(&test_name) {
                last_passed = cur_passed;
                last_failed = cur_failed;
                continue;
            }
            seen_tests.insert(test_name.clone());

            let status = if cur_failed > last_failed {
                TestStatus::Failed
            } else if cur_passed > last_passed {
                TestStatus::Passed
            } else {
                TestStatus::Skipped
            };

            last_passed = cur_passed;
            last_failed = cur_failed;

            results.push(TestResult {
                name: test_name,
                status,
                duration_ms: None,
                file: None,
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: None,
            });
        }
    }

    TestRun::from_results(results, Some("flutter-test".into()), Format::Flutter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flutter_passing() {
        let input = r#"
00:01 +0: loading /test/widget_test.dart
00:03 +1: test widget renders title
00:04 +2: test widget renders body
00:04 +2: All tests passed!
"#;
        let run = parse_flutter(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 2);
    }

    #[test]
    fn parse_flutter_failure() {
        let input = r#"
00:01 +0: loading /test/widget_test.dart
00:03 +1: test widget renders title
00:04 +1 -1: test widget renders wrong color
00:04 +1 -1: Some tests failed.
"#;
        let run = parse_flutter(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
    }

    #[test]
    fn parse_flutter_mixed() {
        let input = r#"
00:01 +0: loading /test/widget_test.dart
00:02 +1: test add
00:03 +2: test sub
00:04 +2 -1: test div
00:05 +3 -1: test mul
"#;
        let run = parse_flutter(input);
        assert_eq!(run.total, 4);
        assert_eq!(run.passed, 3);
        assert_eq!(run.failed, 1);
    }
}
