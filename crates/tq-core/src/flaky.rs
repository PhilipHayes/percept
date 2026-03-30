use serde::Serialize;
use std::collections::HashMap;

use tq_parse::{TestRun, TestStatus};

/// Per-test flakiness stats aggregated across runs.
#[derive(Debug, Serialize)]
pub struct FlakyTest {
    pub name: String,
    pub runs: u32,
    pub failures: u32,
    pub fail_rate: f64,
}

/// Summary of flaky-detection analysis.
#[derive(Debug, Serialize)]
pub struct FlakyReport {
    pub total_runs: usize,
    /// Tests that both passed and failed across runs, sorted by fail_rate descending.
    pub flaky_tests: Vec<FlakyTest>,
    /// Tests that failed in every run.
    pub always_failing: Vec<String>,
}

/// Analyze multiple test runs for flaky tests.
/// A test is considered flaky if it fails in some runs but passes in others.
pub fn detect_flaky(runs: &[TestRun]) -> FlakyReport {
    let mut stats: HashMap<String, (u32, u32)> = HashMap::new(); // (runs, failures)

    for run in runs {
        for test in &run.tests {
            let entry = stats.entry(test.name.clone()).or_insert((0, 0));
            entry.0 += 1;
            if matches!(test.status, TestStatus::Failed | TestStatus::Errored) {
                entry.1 += 1;
            }
        }
    }

    let mut flaky_tests: Vec<FlakyTest> = Vec::new();
    let mut always_failing: Vec<String> = Vec::new();

    for (name, (run_count, fail_count)) in &stats {
        if *fail_count == 0 {
            continue;
        }
        if *fail_count == *run_count {
            always_failing.push(name.clone());
        } else {
            flaky_tests.push(FlakyTest {
                name: name.clone(),
                runs: *run_count,
                failures: *fail_count,
                fail_rate: *fail_count as f64 / *run_count as f64,
            });
        }
    }

    flaky_tests.sort_by(|a, b| b.fail_rate.partial_cmp(&a.fail_rate).unwrap());
    always_failing.sort();

    FlakyReport {
        total_runs: runs.len(),
        flaky_tests,
        always_failing,
    }
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
    fn detect_flaky_test() {
        let runs = vec![
            make_run(vec![("a", TestStatus::Passed), ("b", TestStatus::Passed)]),
            make_run(vec![("a", TestStatus::Failed), ("b", TestStatus::Passed)]),
            make_run(vec![("a", TestStatus::Passed), ("b", TestStatus::Passed)]),
        ];
        let report = detect_flaky(&runs);
        assert_eq!(report.total_runs, 3);
        assert_eq!(report.flaky_tests.len(), 1);
        assert_eq!(report.flaky_tests[0].name, "a");
        assert_eq!(report.flaky_tests[0].failures, 1);
        assert!((report.flaky_tests[0].fail_rate - 1.0 / 3.0).abs() < 0.01);
        assert!(report.always_failing.is_empty());
    }

    #[test]
    fn detect_always_failing() {
        let runs = vec![
            make_run(vec![("a", TestStatus::Failed)]),
            make_run(vec![("a", TestStatus::Failed)]),
        ];
        let report = detect_flaky(&runs);
        assert!(report.flaky_tests.is_empty());
        assert_eq!(report.always_failing, vec!["a"]);
    }

    #[test]
    fn no_failures_means_no_flaky() {
        let runs = vec![
            make_run(vec![("a", TestStatus::Passed)]),
            make_run(vec![("a", TestStatus::Passed)]),
        ];
        let report = detect_flaky(&runs);
        assert!(report.flaky_tests.is_empty());
        assert!(report.always_failing.is_empty());
    }
}
