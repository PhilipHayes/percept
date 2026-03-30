use serde::Deserialize;

use crate::model::{TestResult, TestRun, TestStatus, Format};

/// A single event from `go test -json`.
/// {"Time":"...","Action":"run","Package":"math","Test":"TestAdd"}
/// {"Time":"...","Action":"pass","Package":"math","Test":"TestAdd","Elapsed":0.01}
/// {"Time":"...","Action":"output","Package":"math","Test":"TestAdd","Output":"..."}
#[derive(Deserialize)]
struct GoTestEvent {
    #[serde(rename = "Action")]
    action: String,
    #[serde(rename = "Test")]
    test: Option<String>,
    #[serde(rename = "Elapsed")]
    elapsed: Option<f64>,
    #[serde(rename = "Output")]
    output: Option<String>,
    #[serde(rename = "Package")]
    package: Option<String>,
}

/// Parse `go test -json` NDJSON output.
pub fn parse_gotest_json(input: &str) -> TestRun {
    let mut results: Vec<TestResult> = Vec::new();
    let mut outputs: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(event) = serde_json::from_str::<GoTestEvent>(trimmed) else {
            continue;
        };

        let Some(ref test_name) = event.test else {
            continue;
        };

        match event.action.as_str() {
            "output" => {
                if let Some(ref out) = event.output {
                    outputs
                        .entry(test_name.clone())
                        .or_default()
                        .push(out.clone());
                }
            }
            "pass" | "fail" | "skip" => {
                let status = match event.action.as_str() {
                    "pass" => TestStatus::Passed,
                    "fail" => TestStatus::Failed,
                    "skip" => TestStatus::Skipped,
                    _ => TestStatus::Passed,
                };
                let duration_ms = event.elapsed.map(|e| (e * 1000.0) as u64);
                let test_output = outputs.remove(test_name);
                let stdout = test_output.as_ref().map(|lines| lines.join("").trim().to_string());
                let message = if status == TestStatus::Failed {
                    stdout.clone()
                } else {
                    None
                };
                let suite = event.package.clone();

                results.push(TestResult {
                    name: test_name.clone(),
                    status,
                    duration_ms,
                    file: None,
                    line: None,
                    message,
                    stdout,
                    stderr: None,
                    suite,
                });
            }
            _ => {}
        }
    }

    TestRun::from_results(results, Some("go-test".into()), Format::GoTestJson)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_go_json_pass() {
        let input = r#"
{"Time":"2024-01-01T00:00:00Z","Action":"run","Package":"math","Test":"TestAdd"}
{"Time":"2024-01-01T00:00:00Z","Action":"pass","Package":"math","Test":"TestAdd","Elapsed":0.01}
{"Time":"2024-01-01T00:00:00Z","Action":"run","Package":"math","Test":"TestSub"}
{"Time":"2024-01-01T00:00:00Z","Action":"pass","Package":"math","Test":"TestSub","Elapsed":0.02}
"#;
        let run = parse_gotest_json(input);
        assert_eq!(run.total, 2);
        assert_eq!(run.passed, 2);
    }

    #[test]
    fn parse_go_json_fail() {
        let input = r#"
{"Time":"2024-01-01T00:00:00Z","Action":"run","Package":"math","Test":"TestDiv"}
{"Time":"2024-01-01T00:00:00Z","Action":"output","Package":"math","Test":"TestDiv","Output":"    math_test.go:15: expected 0\n"}
{"Time":"2024-01-01T00:00:00Z","Action":"fail","Package":"math","Test":"TestDiv","Elapsed":0.01}
"#;
        let run = parse_gotest_json(input);
        assert_eq!(run.total, 1);
        assert_eq!(run.failed, 1);
        assert!(run.tests[0].message.is_some());
    }
}
