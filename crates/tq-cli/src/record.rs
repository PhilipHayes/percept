//! ADR-025 D6 test-record emission.
//!
//! Turns a parsed `TestRun` into the `test-record.json` shape that a contract
//! verifier reads to resolve `test_named` assertions. Without a record, those
//! assertions are `unknown` — never `green` — because a test that exists is not
//! the same claim as a test that passed (ADR-025 D2: no signal is not a pass).
//!
//! This exists so emitting a record is a property of the tool that already owns
//! test parsing, rather than of per-language shell scripts re-deriving runner
//! formats (the duplication ADR-028 flagged).

use serde::Serialize;
use tq_parse::{TestRun, TestStatus};

#[derive(Debug, Serialize)]
pub struct TestRecord {
    pub schema_version: u32,
    pub generated_at: String,
    pub head: String,
    pub language: String,
    pub runner: String,
    pub runner_completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_exit_code: Option<i32>,
    pub tests: Vec<TestRecordEntry>,
}

#[derive(Debug, Serialize)]
pub struct TestRecordEntry {
    pub suite: String,
    pub name: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Split a libtest-qualified name into a URI-native `(suite, name)` pair.
///
/// `crate_name` is the binary the test ran under (from the `Running …/deps/…`
/// header) and `qualified` is the module path as libtest prints it. Both are
/// joined with `/` because `::` is not URI-native and a verifier that builds
/// assertion URIs must reject it.
///
///   ("tq_core", "flaky::tests::detect_flaky_test")
///     -> ("tq_core/flaky/tests", "detect_flaky_test")
///
/// A name with no module path keeps the crate alone as its suite, which is the
/// common shape for integration tests in `tests/`.
pub fn split_suite_and_name(crate_name: Option<&str>, qualified: &str) -> (String, String) {
    let mut segments: Vec<&str> = qualified.split("::").collect();
    let leaf = segments.pop().unwrap_or(qualified);

    let mut suite_parts: Vec<String> = Vec::new();
    if let Some(c) = crate_name {
        suite_parts.push(c.to_string());
    }
    suite_parts.extend(segments.iter().map(|s| s.to_string()));

    (suite_parts.join("/"), leaf.to_string())
}

fn status_str(s: TestStatus) -> &'static str {
    // The record vocabulary is ADR-025 D6's, not tq's: a verifier distinguishes
    // ok / failed / ignored only. `errored` collapses to `failed` — from a
    // claim's point of view a test that blew up did not pass.
    match s {
        TestStatus::Passed => "ok",
        TestStatus::Failed | TestStatus::Errored => "failed",
        TestStatus::Skipped => "ignored",
    }
}

/// Build a record from a parsed run.
///
/// `runner_completed` is caller-supplied rather than inferred: a run that was
/// killed halfway still parses into a perfectly well-formed `TestRun`, and a
/// verifier must be able to tell "these tests passed" from "these tests passed
/// so far." Absent tests in a truncated record resolve to `unknown`, which is
/// the correct answer.
pub fn build_record(
    run: &TestRun,
    head: &str,
    generated_at: &str,
    runner_completed: bool,
    runner_exit_code: Option<i32>,
) -> TestRecord {
    let tests = run
        .tests
        .iter()
        .map(|t| {
            let (suite, name) = split_suite_and_name(t.suite.as_deref(), &t.name);
            TestRecordEntry {
                suite,
                name,
                status: status_str(t.status),
                detail: t.message.clone(),
            }
        })
        .collect();

    TestRecord {
        schema_version: 1,
        generated_at: generated_at.to_string(),
        head: head.to_string(),
        language: "rust".to_string(),
        runner: run.runner.clone().unwrap_or_else(|| "libtest".to_string()),
        runner_completed,
        runner_exit_code,
        tests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tq_parse::{Format, TestResult};

    fn result(name: &str, suite: Option<&str>, status: TestStatus) -> TestResult {
        TestResult {
            name: name.to_string(),
            status,
            duration_ms: None,
            file: None,
            line: None,
            message: None,
            stdout: None,
            stderr: None,
            suite: suite.map(|s| s.to_string()),
        }
    }

    #[test]
    fn splits_crate_and_module_path_into_uri_native_suite() {
        let (suite, name) = split_suite_and_name(Some("tq_core"), "flaky::tests::detect_flaky_test");
        assert_eq!(suite, "tq_core/flaky/tests");
        assert_eq!(name, "detect_flaky_test");
    }

    #[test]
    fn bare_test_name_keeps_crate_as_suite() {
        let (suite, name) = split_suite_and_name(Some("wq_cli"), "emits_json");
        assert_eq!(suite, "wq_cli");
        assert_eq!(name, "emits_json");
    }

    #[test]
    fn missing_crate_falls_back_to_module_path_only() {
        let (suite, name) = split_suite_and_name(None, "core::tests::test_c");
        assert_eq!(suite, "core/tests");
        assert_eq!(name, "test_c");
    }

    #[test]
    fn suite_never_contains_double_colon() {
        // A verifier building assertion URIs rejects `::` outright.
        let (suite, _) = split_suite_and_name(Some("a_crate"), "b::c::d::e");
        assert!(!suite.contains("::"));
        assert_eq!(suite, "a_crate/b/c/d");
    }

    #[test]
    fn errored_collapses_to_failed() {
        let run = TestRun::from_results(
            vec![result("m::tests::t", Some("k"), TestStatus::Errored)],
            None,
            Format::Libtest,
        );
        let rec = build_record(&run, "abc123", "2026-08-15T00:00:00Z", true, Some(101));
        assert_eq!(rec.tests[0].status, "failed");
    }

    #[test]
    fn maps_each_status_to_the_record_vocabulary() {
        let run = TestRun::from_results(
            vec![
                result("m::tests::p", Some("k"), TestStatus::Passed),
                result("m::tests::f", Some("k"), TestStatus::Failed),
                result("m::tests::s", Some("k"), TestStatus::Skipped),
            ],
            None,
            Format::Libtest,
        );
        let rec = build_record(&run, "abc123", "2026-08-15T00:00:00Z", true, Some(0));
        let got: Vec<&str> = rec.tests.iter().map(|t| t.status).collect();
        assert_eq!(got, vec!["ok", "failed", "ignored"]);
    }

    #[test]
    fn record_carries_schema_version_one_and_completion_flag() {
        let run = TestRun::from_results(
            vec![result("m::tests::p", Some("k"), TestStatus::Passed)],
            None,
            Format::Libtest,
        );
        let rec = build_record(&run, "deadbeef", "2026-08-15T00:00:00Z", false, None);
        assert_eq!(rec.schema_version, 1);
        assert_eq!(rec.head, "deadbeef");
        assert!(!rec.runner_completed);
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"schema_version\":1"));
        // runner_exit_code omitted rather than null when unknown
        assert!(!json.contains("runner_exit_code"));
    }
}
