use serde::Serialize;
use std::collections::HashMap;

use tq_parse::{TestRun, TestStatus};

/// Result of diffing two test runs (before vs after).
#[derive(Debug, Serialize)]
pub struct DiffResult {
    /// Tests that were passing before but now fail.
    pub new_failures: Vec<String>,
    /// Tests that were failing before but now pass.
    pub new_passes: Vec<String>,
    /// Tests that failed in both runs.
    pub still_failing: Vec<String>,
    /// Tests that are new in the "after" run.
    pub added: Vec<String>,
    /// Tests that were in "before" but missing from "after".
    pub removed: Vec<String>,
}

/// Diff two test runs: before (a) and after (b).
pub fn diff_runs(before: &TestRun, after: &TestRun) -> DiffResult {
    let before_map: HashMap<&str, &TestStatus> = before
        .tests
        .iter()
        .map(|t| (t.name.as_str(), &t.status))
        .collect();

    let after_map: HashMap<&str, &TestStatus> = after
        .tests
        .iter()
        .map(|t| (t.name.as_str(), &t.status))
        .collect();

    let mut new_failures = Vec::new();
    let mut new_passes = Vec::new();
    let mut still_failing = Vec::new();
    let mut added = Vec::new();

    for (name, after_status) in &after_map {
        match before_map.get(name) {
            Some(before_status) => {
                let was_fail = matches!(before_status, TestStatus::Failed | TestStatus::Errored);
                let now_fail = matches!(after_status, TestStatus::Failed | TestStatus::Errored);
                match (was_fail, now_fail) {
                    (false, true) => new_failures.push(name.to_string()),
                    (true, false) => new_passes.push(name.to_string()),
                    (true, true) => still_failing.push(name.to_string()),
                    _ => {}
                }
            }
            None => added.push(name.to_string()),
        }
    }

    let removed: Vec<String> = before_map
        .keys()
        .filter(|name| !after_map.contains_key(*name))
        .map(|name| name.to_string())
        .collect();

    // Sort for deterministic output
    let mut result = DiffResult {
        new_failures,
        new_passes,
        still_failing,
        added,
        removed,
    };
    result.new_failures.sort();
    result.new_passes.sort();
    result.still_failing.sort();
    result.added.sort();
    result.removed.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tq_parse::{TestResult, Format};

    fn make_run(tests: Vec<(&str, TestStatus)>) -> TestRun {
        let results: Vec<TestResult> = tests
            .into_iter()
            .map(|(name, status)| TestResult {
                name: name.to_string(),
                status,
                duration_ms: None,
                file: None,
                line: None,
                message: None,
                stdout: None,
                stderr: None,
                suite: None,
            })
            .collect();
        TestRun::from_results(results, None, Format::Unknown)
    }

    #[test]
    fn diff_new_failure() {
        let before = make_run(vec![("a", TestStatus::Passed), ("b", TestStatus::Passed)]);
        let after = make_run(vec![("a", TestStatus::Passed), ("b", TestStatus::Failed)]);
        let d = diff_runs(&before, &after);
        assert_eq!(d.new_failures, vec!["b"]);
        assert!(d.new_passes.is_empty());
        assert!(d.still_failing.is_empty());
    }

    #[test]
    fn diff_new_pass() {
        let before = make_run(vec![("a", TestStatus::Failed)]);
        let after = make_run(vec![("a", TestStatus::Passed)]);
        let d = diff_runs(&before, &after);
        assert!(d.new_failures.is_empty());
        assert_eq!(d.new_passes, vec!["a"]);
    }

    #[test]
    fn diff_still_failing() {
        let before = make_run(vec![("a", TestStatus::Failed)]);
        let after = make_run(vec![("a", TestStatus::Failed)]);
        let d = diff_runs(&before, &after);
        assert_eq!(d.still_failing, vec!["a"]);
    }

    #[test]
    fn diff_added_removed() {
        let before = make_run(vec![("a", TestStatus::Passed)]);
        let after = make_run(vec![("b", TestStatus::Passed)]);
        let d = diff_runs(&before, &after);
        assert_eq!(d.added, vec!["b"]);
        assert_eq!(d.removed, vec!["a"]);
    }
}
