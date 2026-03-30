use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Level, LogEntry};

/// Parse a bracket-formatted log line.
///
/// Common patterns:
/// - `[2026-03-11 10:00:00] ERROR api: connection refused`
/// - `[2026-03-11T10:00:00Z] INFO  Server started`
/// - `2026-03-11 10:00:00.123 ERROR [api] connection refused`
/// - `[ERROR] 2026-03-11 10:00:00 message`
pub fn parse_bracket_line(clean: &str, raw: &str) -> LogEntry {
    // Strategy: try multiple regex patterns, take the first that matches.
    if let Some(entry) = try_timestamp_source_level(clean, raw) {
        return entry;
    }
    if let Some(entry) = try_timestamp_level_source(clean, raw) {
        return entry;
    }
    if let Some(entry) = try_level_first(clean, raw) {
        return entry;
    }
    if let Some(entry) = try_bare_level(clean, raw) {
        return entry;
    }

    // Fallback: treat the whole line as message
    LogEntry {
        timestamp: None,
        level: None,
        source: None,
        message: clean.to_string(),
        fields: HashMap::new(),
        raw: raw.to_string(),
    }
}

/// Pattern: `TIMESTAMP [SOURCE] [LEVEL] message` (e.g. Claude MCP logs)
/// Example: `2025-07-20T00:10:07.796Z [Filesystem] [info] Server started { metadata: undefined }`
fn try_timestamp_source_level(clean: &str, raw: &str) -> Option<LogEntry> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^(\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)\s+\[([^\]]+)\]\s+\[((?i)trace|debug|info|warn(?:ing)?|error|err|fatal|critical|verbose)\]\s+(.+)$"
        )
        .unwrap()
    });

    let caps = RE.captures(clean)?;
    let timestamp = parse_timestamp(caps.get(1)?.as_str());
    let source = Some(caps.get(2)?.as_str().to_string());
    let level = Level::parse(caps.get(3)?.as_str());
    let message = caps.get(4)?.as_str().trim().to_string();

    Some(LogEntry {
        timestamp,
        level,
        source,
        message,
        fields: HashMap::new(),
        raw: raw.to_string(),
    })
}

/// Pattern: `[timestamp] LEVEL source: message` or `[timestamp] LEVEL message`
fn try_timestamp_level_source(clean: &str, raw: &str) -> Option<LogEntry> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^\[?(\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)\]?\s+(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|ERR|FATAL|CRITICAL)\s*(?:(\S+?):\s+)?(.+)$"
        )
        .unwrap()
    });

    let caps = RE.captures(clean)?;
    let timestamp = parse_timestamp(caps.get(1)?.as_str());
    let level = Level::parse(caps.get(2)?.as_str());
    let source = caps.get(3).map(|m| m.as_str().to_string());
    let message = caps.get(4)?.as_str().trim().to_string();

    Some(LogEntry {
        timestamp,
        level,
        source,
        message,
        fields: HashMap::new(),
        raw: raw.to_string(),
    })
}

/// Pattern: `[LEVEL] timestamp message` or `[LEVEL] message`
fn try_level_first(clean: &str, raw: &str) -> Option<LogEntry> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^\[(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|ERR|FATAL|CRITICAL)\]\s+(?:(\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)\s+)?(.+)$"
        )
        .unwrap()
    });

    let caps = RE.captures(clean)?;
    let level = Level::parse(caps.get(1)?.as_str());
    let timestamp = caps.get(2).and_then(|m| parse_timestamp(m.as_str()));
    let message = caps.get(3)?.as_str().trim().to_string();

    Some(LogEntry {
        timestamp,
        level,
        source: None,
        message,
        fields: HashMap::new(),
        raw: raw.to_string(),
    })
}

/// Pattern: bare `LEVEL message` (no brackets, no timestamp)
fn try_bare_level(clean: &str, raw: &str) -> Option<LogEntry> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|ERR|FATAL|CRITICAL)\s+(?:(\S+?):\s+)?(.+)$"
        )
        .unwrap()
    });

    let caps = RE.captures(clean)?;
    let level = Level::parse(caps.get(1)?.as_str());
    let source = caps.get(2).map(|m| m.as_str().to_string());
    let message = caps.get(3)?.as_str().trim().to_string();

    Some(LogEntry {
        timestamp: None,
        level,
        source,
        message,
        fields: HashMap::new(),
        raw: raw.to_string(),
    })
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Try RFC 3339 / ISO 8601
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try common patterns
    for fmt in &[
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y/%m/%d %H:%M:%S%.f",
        "%Y/%m/%d %H:%M:%S",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(ndt.and_utc());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bracketed_timestamp_level_source() {
        let line = "[2026-03-11 10:00:00] ERROR api: connection refused";
        let entry = parse_bracket_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.source.as_deref(), Some("api"));
        assert_eq!(entry.message, "connection refused");
        assert!(entry.timestamp.is_some());
    }

    #[test]
    fn parse_bracketed_timestamp_level_no_source() {
        let line = "[2026-03-11T10:00:00Z] INFO  Server started on port 8080";
        let entry = parse_bracket_line(line, line);
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.message, "Server started on port 8080");
    }

    #[test]
    fn parse_level_first_bracket() {
        let line = "[ERROR] 2026-03-11 10:00:00 Something went wrong";
        let entry = parse_bracket_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.message, "Something went wrong");
    }

    #[test]
    fn parse_bare_level() {
        let line = "ERROR api: disk full";
        let entry = parse_bracket_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.source.as_deref(), Some("api"));
        assert_eq!(entry.message, "disk full");
    }

    #[test]
    fn parse_fallback_plain_text() {
        let line = "no recognizable format here";
        let entry = parse_bracket_line(line, line);
        assert!(entry.level.is_none());
        assert_eq!(entry.message, "no recognizable format here");
    }

    #[test]
    fn parse_timestamp_source_level_claude_mcp() {
        let line = "2025-07-20T00:10:07.796Z [Filesystem] [info] Initializing server... { metadata: undefined }";
        let entry = parse_bracket_line(line, line);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.source.as_deref(), Some("Filesystem"));
        assert!(entry.message.starts_with("Initializing server..."));
    }

    #[test]
    fn parse_timestamp_source_level_error() {
        let line = "2026-03-15T20:30:31.492Z [Filesystem] [error] Server disconnected. For troubleshooting guidance, please visit our [debugging documentation](https://modelcontextprotocol.io/docs/tools/debugging) { metadata: { context: 'connection', stack: undefined } }";
        let entry = parse_bracket_line(line, line);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.source.as_deref(), Some("Filesystem"));
        assert!(entry.message.starts_with("Server disconnected."));
    }

    #[test]
    fn parse_timestamp_source_verbose_level() {
        let line = "2025-07-20T00:10:07.796Z [MyApp] [verbose] Extra detail here";
        let entry = parse_bracket_line(line, line);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.level, Some(Level::Trace));
        assert_eq!(entry.source.as_deref(), Some("MyApp"));
        assert_eq!(entry.message, "Extra detail here");
    }
}
