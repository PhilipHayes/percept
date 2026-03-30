use regex::Regex;
use std::sync::LazyLock;

use crate::model::{TestResult, TestRun, TestStatus, Format};

// Match <testcase> elements: name, classname, time
static TESTCASE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<testcase\s+([^>]*)>"#).unwrap()
});

// Self-closing: <testcase ... />
static TESTCASE_SELF_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<testcase\s+([^/]*)/>"#).unwrap()
});

static ATTR_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"name="([^"]*)""#).unwrap()
});

static ATTR_CLASSNAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"classname="([^"]*)""#).unwrap()
});

static ATTR_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"time="([^"]*)""#).unwrap()
});

static ATTR_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"file="([^"]*)""#).unwrap()
});

static ATTR_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"line="([^"]*)""#).unwrap()
});

// <failure ...> or <error ...>
static FAILURE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<(failure|error)\b[^>]*>"#).unwrap()
});

// message="..." attribute inside failure/error tags
static FAILURE_MSG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"message="([^"]*)""#).unwrap()
});

// <skipped>
static SKIPPED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<skipped\b"#).unwrap()
});

/// Parse JUnit XML test results.
/// This is a regex-based parser that handles the common JUnit XML schema
/// without requiring a full XML parser dependency.
pub fn parse_junit(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut current_tc: Option<PendingTestCase> = None;
    let mut collecting_failure: bool = false;
    let mut failure_text: Vec<String> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();

        // Try self-closing testcase first: <testcase ... />
        if let Some(caps) = TESTCASE_SELF_CLOSE.captures(trimmed) {
            let attrs = &caps[1];
            if let Some(result) = parse_testcase_attrs(attrs, TestStatus::Passed) {
                results.push(result);
            }
            continue;
        }

        // Opening <testcase ...>
        if let Some(caps) = TESTCASE_OPEN.captures(trimmed) {
            // Flush previous if somehow not closed
            if let Some(tc) = current_tc.take() {
                results.push(tc.into_result());
            }
            let attrs = &caps[1];
            current_tc = Some(PendingTestCase::from_attrs(attrs));
            collecting_failure = false;
            failure_text.clear();
            continue;
        }

        // Inside a testcase block
        if let Some(ref mut tc) = current_tc {
            // Check for <failure> or <error>
            if let Some(caps) = FAILURE_TAG.captures(trimmed) {
                let tag = &caps[1];
                tc.status = if tag == "error" {
                    TestStatus::Errored
                } else {
                    TestStatus::Failed
                };
                if let Some(msg_caps) = FAILURE_MSG.captures(trimmed) {
                    tc.message = Some(msg_caps[1].to_string());
                }
                collecting_failure = true;
                continue;
            }

            // Check for <skipped>
            if SKIPPED_TAG.is_match(trimmed) {
                tc.status = TestStatus::Skipped;
                continue;
            }

            // Collecting failure body text
            if collecting_failure {
                if trimmed.contains("</failure>") || trimmed.contains("</error>") {
                    collecting_failure = false;
                    if tc.message.is_none() && !failure_text.is_empty() {
                        tc.message = Some(failure_text.join("\n").trim().to_string());
                    }
                    tc.stdout = Some(failure_text.join("\n").trim().to_string());
                    failure_text.clear();
                } else {
                    failure_text.push(line.to_string());
                }
                continue;
            }

            // </testcase>
            if trimmed.contains("</testcase>") {
                let tc = current_tc.take().unwrap();
                results.push(tc.into_result());
                continue;
            }
        }
    }

    // Flush if file ends without closing tag
    if let Some(tc) = current_tc.take() {
        results.push(tc.into_result());
    }

    TestRun::from_results(results, Some("junit".into()), Format::Junit)
}

struct PendingTestCase {
    name: String,
    classname: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    duration_ms: Option<u64>,
    status: TestStatus,
    message: Option<String>,
    stdout: Option<String>,
}

impl PendingTestCase {
    fn from_attrs(attrs: &str) -> Self {
        let name = ATTR_NAME
            .captures(attrs)
            .map(|c| c[1].to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let classname = ATTR_CLASSNAME.captures(attrs).map(|c| c[1].to_string());
        let file = ATTR_FILE.captures(attrs).map(|c| c[1].to_string());
        let line = ATTR_LINE
            .captures(attrs)
            .and_then(|c| c[1].parse().ok());
        let duration_ms = ATTR_TIME
            .captures(attrs)
            .and_then(|c| c[1].parse::<f64>().ok())
            .map(|t| (t * 1000.0) as u64);

        Self {
            name,
            classname,
            file,
            line,
            duration_ms,
            status: TestStatus::Passed,
            message: None,
            stdout: None,
        }
    }

    fn into_result(self) -> TestResult {
        let suite = self.classname.clone();
        let full_name = match &self.classname {
            Some(cls) if !cls.is_empty() => format!("{}::{}", cls, self.name),
            _ => self.name,
        };
        TestResult {
            name: full_name,
            status: self.status,
            duration_ms: self.duration_ms,
            file: self.file,
            line: self.line,
            message: self.message,
            stdout: self.stdout,
            stderr: None,
            suite,
        }
    }
}

fn parse_testcase_attrs(attrs: &str, default_status: TestStatus) -> Option<TestResult> {
    let tc = PendingTestCase::from_attrs(attrs);
    Some(TestResult {
        name: match &tc.classname {
            Some(cls) if !cls.is_empty() => format!("{}::{}", cls, tc.name),
            _ => tc.name,
        },
        status: default_status,
        duration_ms: tc.duration_ms,
        file: tc.file,
        line: tc.line,
        message: None,
        stdout: None,
        stderr: None,
        suite: tc.classname,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_passing_junit() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites tests="3" failures="0" time="0.123">
  <testsuite name="math" tests="3" failures="0">
    <testcase name="test_add" classname="math" time="0.01"/>
    <testcase name="test_sub" classname="math" time="0.02"/>
    <testcase name="test_mul" classname="math" time="0.03"/>
  </testsuite>
</testsuites>"#;
        let run = parse_junit(input);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 3);
        assert_eq!(run.failed, 0);
        assert_eq!(run.runner, Some("junit".into()));
        assert!(run.tests[0].suite.as_ref().unwrap() == "math");
    }

    #[test]
    fn parse_with_failure() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="math" tests="2" failures="1">
    <testcase name="test_add" classname="math" time="0.01"/>
    <testcase name="test_div" classname="math" time="0.05">
      <failure message="division by zero">
        expected: 0, got: ZeroDivisionError
      </failure>
    </testcase>
  </testsuite>
</testsuites>"#;
        let run = parse_junit(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures[0].name, "math::test_div");
        assert_eq!(failures[0].message.as_deref(), Some("division by zero"));
    }

    #[test]
    fn parse_with_error_and_skip() {
        let input = r#"
<testsuite name="integration">
  <testcase name="test_connect" classname="integration" time="1.0">
    <error message="timeout">connection timed out</error>
  </testcase>
  <testcase name="test_pending" classname="integration">
    <skipped/>
  </testcase>
</testsuite>"#;
        let run = parse_junit(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.errored, 1);
        assert_eq!(run.skipped, 1);
        assert_eq!(run.failures()[0].status, TestStatus::Errored);
    }
}
