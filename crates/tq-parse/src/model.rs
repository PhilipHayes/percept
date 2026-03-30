use serde::{Deserialize, Serialize};

/// Status of a single test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    Errored,
}

/// A single test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Fully qualified test name (e.g. "module::submodule::test_name").
    pub name: String,
    /// Pass/fail/skip/error status.
    pub status: TestStatus,
    /// Duration in milliseconds, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Source file path, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Source line number, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Failure/error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Captured stdout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Captured stderr.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Test suite or module name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
}

/// Aggregate results for a complete test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRun {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub errored: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub tests: Vec<TestResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    pub format: Format,
}

impl TestRun {
    /// Build a TestRun from a list of results, computing totals.
    pub fn from_results(tests: Vec<TestResult>, runner: Option<String>, format: Format) -> Self {
        let total = tests.len() as u32;
        let passed = tests.iter().filter(|t| t.status == TestStatus::Passed).count() as u32;
        let failed = tests.iter().filter(|t| t.status == TestStatus::Failed).count() as u32;
        let skipped = tests.iter().filter(|t| t.status == TestStatus::Skipped).count() as u32;
        let errored = tests.iter().filter(|t| t.status == TestStatus::Errored).count() as u32;
        let duration_ms = {
            let sum: u64 = tests.iter().filter_map(|t| t.duration_ms).sum();
            if sum > 0 { Some(sum) } else { None }
        };
        Self {
            total,
            passed,
            failed,
            skipped,
            errored,
            duration_ms,
            tests,
            runner,
            format,
        }
    }

    /// Return only the failing tests.
    pub fn failures(&self) -> Vec<&TestResult> {
        self.tests
            .iter()
            .filter(|t| t.status == TestStatus::Failed || t.status == TestStatus::Errored)
            .collect()
    }
}

/// Detected test output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// cargo test text output (libtest)
    Libtest,
    /// cargo test --format json (nightly NDJSON)
    LibtestJson,
    /// pytest text output
    Pytest,
    /// JUnit XML
    Junit,
    /// Jest text output
    Jest,
    /// Go test text output
    GoTest,
    /// Go test -json
    GoTestJson,
    /// TAP (Test Anything Protocol)
    Tap,
    /// Flutter test runner
    Flutter,
    /// Unknown format
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_from_results() {
        let tests = vec![
            TestResult {
                name: "test_a".into(),
                status: TestStatus::Passed,
                duration_ms: Some(10),
                file: None, line: None, message: None,
                stdout: None, stderr: None, suite: None,
            },
            TestResult {
                name: "test_b".into(),
                status: TestStatus::Failed,
                duration_ms: Some(20),
                file: None, line: None,
                message: Some("assertion failed".into()),
                stdout: None, stderr: None, suite: None,
            },
            TestResult {
                name: "test_c".into(),
                status: TestStatus::Skipped,
                duration_ms: None,
                file: None, line: None, message: None,
                stdout: None, stderr: None, suite: None,
            },
        ];
        let run = TestRun::from_results(tests, Some("cargo".into()), Format::Libtest);
        assert_eq!(run.total, 3);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        assert_eq!(run.skipped, 1);
        assert_eq!(run.errored, 0);
        assert_eq!(run.duration_ms, Some(30));
        assert_eq!(run.failures().len(), 1);
        assert_eq!(run.failures()[0].name, "test_b");
    }

    #[test]
    fn test_status_serialization() {
        let json = serde_json::to_string(&TestStatus::Failed).unwrap();
        assert_eq!(json, r#""failed""#);
        let back: TestStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TestStatus::Failed);
    }
}
