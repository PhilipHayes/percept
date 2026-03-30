use crate::Format;

/// Detect the test output format from sample lines.
pub fn detect_format(lines: &[&str]) -> Format {
    let sample_size = lines.len();
    if sample_size == 0 {
        return Format::Unknown;
    }

    let mut libtest_score = 0u32;
    let mut pytest_score = 0u32;
    let mut junit_score = 0u32;
    let mut jest_score = 0u32;
    let mut go_score = 0u32;
    let mut go_json_score = 0u32;
    let mut tap_score = 0u32;
    let mut libtest_json_score = 0u32;

    for line in &lines[..sample_size] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // libtest JSON: {"type":"suite",...} or {"type":"test",...}
        if trimmed.starts_with('{')
            && (trimmed.contains(r#""type":"suite""#) || trimmed.contains(r#""type":"test""#))
        {
            libtest_json_score += 3;
        }

        // JUnit XML: <testsuites or <testsuite or <?xml
        if trimmed.starts_with("<testsuites") || trimmed.starts_with("<testsuite") {
            junit_score += 5;
        }
        if trimmed.contains("</testcase>") || trimmed.contains("<testcase ") {
            junit_score += 3;
        }

        // Go test -json: {"Time":..., "Action":..., "Test":...}
        if trimmed.starts_with('{')
            && trimmed.contains(r#""Action""#)
            && trimmed.contains(r#""Test""#)
        {
            go_json_score += 3;
        }

        // TAP: starts with "ok " or "not ok " or "1..N"
        if trimmed.starts_with("ok ") || trimmed.starts_with("not ok ") {
            tap_score += 3;
        }
        if trimmed.starts_with("1..") && trimmed[3..].chars().all(|c| c.is_ascii_digit()) {
            tap_score += 2;
        }

        // libtest text: "test result:" or "running N test" or "test foo ... ok"
        if trimmed.starts_with("test result:") || trimmed.starts_with("running ") {
            libtest_score += 3;
        }
        if trimmed.starts_with("test ")
            && (trimmed.ends_with("... ok")
                || trimmed.ends_with("... FAILED")
                || trimmed.ends_with("... ignored"))
        {
            libtest_score += 2;
        }

        // pytest: lines with :: separators, PASSED/FAILED/ERROR
        if trimmed.contains("::")
            && (trimmed.contains("PASSED")
                || trimmed.contains("FAILED")
                || trimmed.contains("ERROR"))
        {
            pytest_score += 3;
        }
        if trimmed.starts_with("=")
            && (trimmed.contains("passed")
                || trimmed.contains("failed")
                || trimmed.contains("error"))
        {
            pytest_score += 2;
        }

        // Jest: "Tests:" summary or "PASS " / "FAIL " file prefixes
        if trimmed.starts_with("Tests:")
            && (trimmed.contains("passed") || trimmed.contains("failed"))
        {
            jest_score += 3;
        }
        if trimmed.starts_with("PASS ") || trimmed.starts_with("FAIL ") {
            jest_score += 2;
        }

        // Go test text: "--- FAIL:" or "--- PASS:" or "FAIL\t" / "ok  \t"
        if trimmed.starts_with("--- FAIL:") || trimmed.starts_with("--- PASS:") {
            go_score += 3;
        }
        if trimmed.starts_with("FAIL\t") || trimmed.starts_with("ok  \t") {
            go_score += 2;
        }
    }

    let scores = [
        (junit_score, Format::Junit),
        (libtest_json_score, Format::LibtestJson),
        (go_json_score, Format::GoTestJson),
        (tap_score, Format::Tap),
        (libtest_score, Format::Libtest),
        (pytest_score, Format::Pytest),
        (jest_score, Format::Jest),
        (go_score, Format::GoTest),
    ];

    scores
        .iter()
        .filter(|(score, _)| *score > 0)
        .max_by_key(|(score, _)| *score)
        .map(|(_, fmt)| *fmt)
        .unwrap_or(Format::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_libtest() {
        let lines = vec![
            "running 3 tests",
            "test model::tests::test_a ... ok",
            "test model::tests::test_b ... FAILED",
            "test model::tests::test_c ... ok",
            "",
            "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
        ];
        assert_eq!(detect_format(&lines), Format::Libtest);
    }

    #[test]
    fn detect_junit_xml() {
        let lines = vec![
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<testsuites tests="3" failures="1">"#,
            r#"<testsuite name="my_suite" tests="3">"#,
            r#"<testcase name="test_a" classname="module"/>"#,
        ];
        assert_eq!(detect_format(&lines), Format::Junit);
    }

    #[test]
    fn detect_pytest() {
        let lines = vec![
            "test_math.py::test_add PASSED",
            "test_math.py::test_sub PASSED",
            "test_math.py::test_div FAILED",
            "========================= 2 passed, 1 failed =========================",
        ];
        assert_eq!(detect_format(&lines), Format::Pytest);
    }

    #[test]
    fn detect_tap() {
        let lines = vec![
            "1..4",
            "ok 1 - addition works",
            "ok 2 - subtraction works",
            "not ok 3 - division by zero",
            "ok 4 - multiplication works",
        ];
        assert_eq!(detect_format(&lines), Format::Tap);
    }

    #[test]
    fn detect_unknown_empty() {
        let lines: Vec<&str> = vec![];
        assert_eq!(detect_format(&lines), Format::Unknown);
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    /// Cargo warnings can fill 20+ lines before any test output appears.
    /// Previously, detect_format only sampled the first 20 lines and returned
    /// Unknown when all 20 were warnings.
    #[test]
    fn detect_libtest_after_cargo_warnings() {
        let mut lines: Vec<&str> = Vec::new();
        // 25 lines of cargo warnings before any test output
        for _ in 0..25 {
            lines.push("warning: field `verb_idx` is never read");
        }
        lines.push("running 3 tests");
        lines.push("test model::tests::test_a ... ok");
        lines.push("test model::tests::test_b ... ok");
        lines.push("test model::tests::test_c ... ok");
        lines.push("test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s");
        assert_eq!(detect_format(&lines), Format::Libtest);
    }
}
