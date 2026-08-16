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
use tq_parse::{Format, TestRun, TestStatus};

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
    // The record vocabulary is ADR-025 D6's, not tq's.
    //
    // `errored` is emitted verbatim rather than collapsed into `failed`. An
    // earlier version of this function mapped it to `failed` because the record
    // had no fourth value; that was wrong. A failed assertion is a verdict —
    // known false. A test that errored (panicked in setup, OOM-killed, harness
    // crash) ran and produced *no verdict*, which under the four-valued status
    // model is `unknown`, not `red`. Collapsing the two is exactly the
    // boolean-flattening the model exists to prevent.
    match s {
        TestStatus::Passed => "ok",
        TestStatus::Failed => "failed",
        TestStatus::Errored => "errored",
        TestStatus::Skipped => "ignored",
    }
}

/// Best-effort language tag for the record, from the runner format.
///
/// `language` is a claim about the corpus the record describes, and hardcoding
/// it made this emitter silently Rust-only — wrong the moment tq is pointed at
/// a JUnit, pytest or Flutter run. Formats that serve several languages resolve
/// to "unknown" rather than guessing; pass `--language` to be explicit.
pub fn infer_language(format: Format) -> &'static str {
    match format {
        Format::Libtest | Format::LibtestJson => "rust",
        Format::Pytest => "python",
        Format::GoTest | Format::GoTestJson => "go",
        Format::Jest => "javascript",
        Format::Flutter => "dart",
        // JUnit XML and TAP are interchange formats, not language markers —
        // .NET, Java, Kotlin and others all emit JUnit. `Unknown` is tq's own
        // "could not detect the runner" value and is likewise not a language.
        Format::Junit | Format::Tap | Format::Unknown => "unknown",
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
    language: Option<&str>,
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
        // v2, not v1: this emitter can produce `errored`, which a v1 consumer
        // rejects. Adding a value to an interchange enum is forward-breaking,
        // and declaring the version is what makes that visible rather than a
        // parse failure at the far end (ADR-025 D6).
        schema_version: 2,
        generated_at: generated_at.to_string(),
        head: head.to_string(),
        language: language
            .map(|l| l.to_string())
            .unwrap_or_else(|| infer_language(run.format).to_string()),
        runner: run.runner.clone().unwrap_or_else(|| "libtest".to_string()),
        runner_completed,
        runner_exit_code,
        tests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tq_parse::TestResult;

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
    fn errored_is_distinct_from_failed() {
        // Regression: this previously collapsed to "failed", making a test that
        // produced no verdict indistinguishable from one that produced a false
        // verdict. A verifier reads the first as unknown and the second as red.
        let run = TestRun::from_results(
            vec![result("m::tests::t", Some("k"), TestStatus::Errored)],
            None,
            Format::Libtest,
        );
        let rec = build_record(&run, "abc123", "2026-08-15T00:00:00Z", true, Some(101), None);
        assert_eq!(rec.tests[0].status, "errored");
    }

    #[test]
    fn language_is_inferred_from_the_runner_format() {
        let mk = |f: Format| {
            let run = TestRun::from_results(
                vec![result("t", Some("s"), TestStatus::Passed)],
                None,
                f,
            );
            build_record(&run, "h", "2026-08-15T00:00:00Z", true, Some(0), None).language
        };
        assert_eq!(mk(Format::Libtest), "rust");
        assert_eq!(mk(Format::Pytest), "python");
        assert_eq!(mk(Format::Flutter), "dart");
        assert_eq!(mk(Format::GoTest), "go");
        // JUnit and TAP are emitted by many ecosystems — guessing would be worse
        // than admitting we do not know.
        assert_eq!(mk(Format::Junit), "unknown");
        assert_eq!(mk(Format::Tap), "unknown");
    }

    #[test]
    fn explicit_language_overrides_inference() {
        let run = TestRun::from_results(
            vec![result("MyTests.TestAdd", Some("MyTests"), TestStatus::Passed)],
            None,
            Format::Junit,
        );
        let rec = build_record(
            &run,
            "h",
            "2026-08-15T00:00:00Z",
            true,
            Some(0),
            Some("vb.net"),
        );
        assert_eq!(rec.language, "vb.net");
    }

    #[test]
    fn non_rust_names_without_double_colon_keep_the_runner_suite() {
        // JUnit-style: the runner already supplies a real suite and the name has
        // no module path. The `::` split is a Rust-specific enhancement that
        // must degrade to a no-op, not corrupt the pair.
        let (suite, name) = split_suite_and_name(Some("MyProject.CalculatorTests"), "TestAddition");
        assert_eq!(suite, "MyProject.CalculatorTests");
        assert_eq!(name, "TestAddition");
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
        let rec = build_record(&run, "abc123", "2026-08-15T00:00:00Z", true, Some(0), None);
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
        let rec = build_record(&run, "deadbeef", "2026-08-15T00:00:00Z", false, None, None);
        assert_eq!(rec.schema_version, 2);
        assert_eq!(rec.head, "deadbeef");
        assert!(!rec.runner_completed);
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"schema_version\":2"));
        // runner_exit_code omitted rather than null when unknown
        assert!(!json.contains("runner_exit_code"));
    }
}
