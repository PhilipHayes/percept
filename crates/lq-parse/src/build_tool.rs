use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Level, LogEntry};

// Cargo: error[E0308]: or warning: or note:
static CARGO_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(error|warning|note)(?:\[E\d+\])?:\s*(.*)$").unwrap());

// Cargo location: --> src/main.rs:10:5
static CARGO_LOCATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*-->\s*(\S+:\d+:\d+)").unwrap());

// Gradle: > Task :build FAILED or BUILD SUCCESSFUL
static GRADLE_STATUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:>?\s*Task\s+:?\S+\s+)?(FAILED|SUCCESSFUL|UP-TO-DATE|SKIPPED)").unwrap()
});

// xcodebuild: error: or warning: with file locations
static XCODE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*?):\s*(error|warning|note):\s*(.*)$").unwrap());

/// Parse a build tool output line (cargo, gradle, xcodebuild).
pub fn parse_build_line(line: &str, raw: &str) -> LogEntry {
    // Try cargo first
    if let Some(caps) = CARGO_LINE.captures(line) {
        let level = match &caps[1] {
            "error" => Level::Error,
            "warning" => Level::Warn,
            "note" => Level::Info,
            _ => Level::Info,
        };
        return LogEntry {
            timestamp: None,
            level: Some(level),
            source: Some("cargo".to_string()),
            message: caps[2].to_string(),
            fields: Default::default(),
            raw: raw.to_string(),
        };
    }

    // Try cargo location line
    if let Some(caps) = CARGO_LOCATION.captures(line) {
        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "location".to_string(),
            serde_json::Value::String(caps[1].to_string()),
        );
        return LogEntry {
            timestamp: None,
            level: None,
            source: Some("cargo".to_string()),
            message: line.trim().to_string(),
            fields,
            raw: raw.to_string(),
        };
    }

    // Try xcodebuild
    if let Some(caps) = XCODE_LINE.captures(line) {
        let location = &caps[1];
        let level = match &caps[2] {
            "error" => Level::Error,
            "warning" => Level::Warn,
            "note" => Level::Info,
            _ => Level::Info,
        };
        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "location".to_string(),
            serde_json::Value::String(location.to_string()),
        );
        return LogEntry {
            timestamp: None,
            level: Some(level),
            source: Some("xcodebuild".to_string()),
            message: caps[3].to_string(),
            fields,
            raw: raw.to_string(),
        };
    }

    // Try gradle status
    if let Some(caps) = GRADLE_STATUS.captures(line) {
        let status = &caps[1];
        let level = if status.eq_ignore_ascii_case("FAILED") {
            Level::Error
        } else {
            Level::Info
        };
        return LogEntry {
            timestamp: None,
            level: Some(level),
            source: Some("gradle".to_string()),
            message: line.to_string(),
            fields: Default::default(),
            raw: raw.to_string(),
        };
    }

    // Fallback
    LogEntry {
        timestamp: None,
        level: None,
        source: None,
        message: line.to_string(),
        fields: Default::default(),
        raw: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cargo_error() {
        let line = "error[E0308]: mismatched types";
        let entry = parse_build_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.source, Some("cargo".to_string()));
        assert_eq!(entry.message, "mismatched types");
    }

    #[test]
    fn parse_cargo_warning() {
        let line = "warning: unused variable: `x`";
        let entry = parse_build_line(line, line);
        assert_eq!(entry.level, Some(Level::Warn));
        assert_eq!(entry.source, Some("cargo".to_string()));
    }

    #[test]
    fn parse_cargo_location() {
        let line = "  --> src/main.rs:10:5";
        let entry = parse_build_line(line, line);
        assert_eq!(entry.source, Some("cargo".to_string()));
        assert_eq!(entry.fields["location"], "src/main.rs:10:5");
    }

    #[test]
    fn parse_xcode_error() {
        let line = "/path/to/File.swift:42:10: error: use of unresolved identifier 'foo'";
        let entry = parse_build_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.source, Some("xcodebuild".to_string()));
        assert_eq!(entry.message, "use of unresolved identifier 'foo'");
    }

    #[test]
    fn parse_gradle_failed() {
        let line = "> Task :compileJava FAILED";
        let entry = parse_build_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.source, Some("gradle".to_string()));
    }
}
