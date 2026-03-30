use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Normalized log level, ordered by severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Level {
    /// Parse a level string case-insensitively. Handles common aliases.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "trace" | "verbose" | "10" => Some(Self::Trace),
            "debug" | "20" => Some(Self::Debug),
            "info" | "information" | "30" => Some(Self::Info),
            "warn" | "warning" | "40" => Some(Self::Warn),
            "error" | "err" | "50" => Some(Self::Error),
            "fatal" | "critical" | "panic" | "60" => Some(Self::Fatal),
            _ => None,
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        };
        f.write_str(s)
    }
}

/// Common log model. Every log line normalizes to this regardless of source format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Parsed timestamp (None if unparseable or absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,

    /// Normalized severity level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<Level>,

    /// Logger name, module, or service identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// The log message text.
    pub message: String,

    /// Structured fields extracted from the log entry.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,

    /// The original raw line, preserved for context output.
    pub raw: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_parse_common() {
        assert_eq!(Level::parse("ERROR"), Some(Level::Error));
        assert_eq!(Level::parse("warn"), Some(Level::Warn));
        assert_eq!(Level::parse("Information"), Some(Level::Info));
        assert_eq!(Level::parse("50"), Some(Level::Error));
        assert_eq!(Level::parse("garbage"), None);
    }

    #[test]
    fn level_ordering() {
        assert!(Level::Trace < Level::Debug);
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
        assert!(Level::Error < Level::Fatal);
    }

    #[test]
    fn log_entry_serializes() {
        let entry = LogEntry {
            timestamp: None,
            level: Some(Level::Error),
            source: Some("api".into()),
            message: "connection refused".into(),
            fields: HashMap::new(),
            raw: "ERROR api: connection refused".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"level\":\"error\""));
        assert!(json.contains("\"message\":\"connection refused\""));
    }
}
