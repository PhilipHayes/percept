use serde::Deserialize;

use crate::model::{TestResult, TestRun, TestStatus, Format};

/// A single event from `cargo test --format json` (nightly) or `cargo test -- -Z unstable-options --format json`.
/// Each line is an NDJSON record like:
/// {"type":"test","event":"ok","name":"module::test_name","exec_time":0.001}
/// {"type":"suite","event":"started","test_count":5}
/// {"type":"suite","event":"ok","passed":5,"failed":0,"ignored":0,...}
#[derive(Deserialize)]
struct LibtestEvent {
    #[serde(rename = "type")]
    event_type: String,
    event: Option<String>,
    name: Option<String>,
    exec_time: Option<f64>,
    stdout: Option<String>,
}

/// Parse libtest JSON (nightly `--format json`) output.
pub fn parse_libtest_json(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(event) = serde_json::from_str::<LibtestEvent>(trimmed) else {
            continue;
        };

        if event.event_type == "test" {
            let Some(ref event_str) = event.event else { continue };
            let Some(ref name) = event.name else { continue };

            // "started" events just mark the beginning; skip them
            if event_str == "started" {
                continue;
            }

            let status = match event_str.as_str() {
                "ok" => TestStatus::Passed,
                "failed" => TestStatus::Failed,
                "ignored" => TestStatus::Skipped,
                _ => continue,
            };

            let duration_ms = event.exec_time.map(|t| (t * 1000.0) as u64);
            let message = if status == TestStatus::Failed {
                event.stdout.clone()
            } else {
                None
            };

            results.push(TestResult {
                name: name.clone(),
                status,
                duration_ms,
                file: None,
                line: None,
                message,
                stdout: event.stdout,
                stderr: None,
                suite: None,
            });
        }
    }

    TestRun::from_results(results, Some("cargo-test".into()), Format::LibtestJson)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_passing() {
        let input = r#"
{ "type": "suite", "event": "started", "test_count": 2 }
{ "type": "test", "event": "started", "name": "math::add" }
{ "type": "test", "event": "started", "name": "math::sub" }
{ "type": "test", "name": "math::add", "event": "ok", "exec_time": 0.001 }
{ "type": "test", "name": "math::sub", "event": "ok", "exec_time": 0.002 }
{ "type": "suite", "event": "ok", "passed": 2, "failed": 0, "ignored": 0, "measured": 0, "filtered_out": 0, "exec_time": 0.003 }
"#;
        let run = parse_libtest_json(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 2);
        assert_eq!(run.failed, 0);
    }

    #[test]
    fn parse_json_with_failure() {
        let input = r#"
{ "type": "suite", "event": "started", "test_count": 2 }
{ "type": "test", "event": "started", "name": "math::add" }
{ "type": "test", "event": "started", "name": "math::div" }
{ "type": "test", "name": "math::add", "event": "ok", "exec_time": 0.001 }
{ "type": "test", "name": "math::div", "event": "failed", "exec_time": 0.003, "stdout": "thread 'math::div' panicked at 'division by zero'" }
{ "type": "suite", "event": "failed", "passed": 1, "failed": 1, "ignored": 0, "measured": 0, "filtered_out": 0, "exec_time": 0.004 }
"#;
        let run = parse_libtest_json(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 1);
        assert_eq!(run.failed, 1);
        let failures = run.failures();
        assert_eq!(failures[0].name, "math::div");
        assert!(failures[0].message.as_ref().unwrap().contains("division by zero"));
    }

    #[test]
    fn parse_json_with_ignored() {
        let input = r#"
{ "type": "test", "event": "started", "name": "slow_test" }
{ "type": "test", "name": "slow_test", "event": "ignored" }
"#;
        let run = parse_libtest_json(input);
        assert_eq!(run.total, 1);
        assert_eq!(run.skipped, 1);
    }
}
