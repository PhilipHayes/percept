use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Level, LogEntry};

// RFC 3164: <priority>Mon DD HH:MM:SS hostname app[pid]: message
static SYSLOG_3164: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^<(\d+)>(\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})\s+(\S+)\s+(\S+?)(?:\[(\d+)\])?:\s*(.*)$",
    )
    .unwrap()
});

// RFC 5424: <priority>version timestamp hostname app-name procid msgid structured-data msg
static SYSLOG_5424: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<(\d+)>(\d+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(?:\[.*?\]|-)\s*(.*)$")
        .unwrap()
});

/// Map syslog priority to a Level.
/// Priority = facility * 8 + severity.
/// Severity 0-2 = Fatal, 3 = Error, 4 = Warn, 5 = Info (notice), 6 = Info, 7 = Debug
fn severity_to_level(priority: u8) -> Level {
    let severity = priority % 8;
    match severity {
        0..=2 => Level::Fatal,
        3 => Level::Error,
        4 => Level::Warn,
        5 | 6 => Level::Info,
        7 => Level::Debug,
        _ => Level::Info,
    }
}

/// Parse a syslog line (RFC 3164 or RFC 5424).
pub fn parse_syslog_line(line: &str, raw: &str) -> LogEntry {
    // Try RFC 5424 first (has version number)
    if let Some(caps) = SYSLOG_5424.captures(line) {
        let priority: u8 = caps[1].parse().unwrap_or(14);
        let timestamp_str = &caps[3];
        let hostname = &caps[4];
        let app = &caps[5];
        let message = caps[8].to_string();

        let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let level = severity_to_level(priority);
        let source = if app != "-" {
            Some(app.to_string())
        } else {
            None
        };

        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "hostname".to_string(),
            serde_json::Value::String(hostname.to_string()),
        );

        return LogEntry {
            timestamp,
            level: Some(level),
            source,
            message,
            fields,
            raw: raw.to_string(),
        };
    }

    // Try RFC 3164
    if let Some(caps) = SYSLOG_3164.captures(line) {
        let priority: u8 = caps[1].parse().unwrap_or(14);
        let hostname = &caps[3];
        let app = &caps[4];
        let message = caps[6].to_string();

        let level = severity_to_level(priority);
        let source = Some(app.to_string());

        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "hostname".to_string(),
            serde_json::Value::String(hostname.to_string()),
        );

        return LogEntry {
            timestamp: None, // RFC 3164 timestamps lack year, skip parsing
            level: Some(level),
            source,
            message,
            fields,
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
    fn parse_rfc3164() {
        let line = "<34>Mar 11 10:00:00 myhost sshd[12345]: Accepted publickey for user";
        let entry = parse_syslog_line(line, line);
        assert_eq!(entry.level, Some(Level::Fatal)); // severity 2 = critical
        assert_eq!(entry.source, Some("sshd".to_string()));
        assert_eq!(entry.message, "Accepted publickey for user");
        assert_eq!(entry.fields["hostname"], "myhost");
    }

    #[test]
    fn parse_rfc3164_info() {
        let line = "<14>Mar 11 10:00:00 myhost app: Service started";
        let entry = parse_syslog_line(line, line);
        assert_eq!(entry.level, Some(Level::Info)); // priority 14 = facility 1, severity 6
        assert_eq!(entry.source, Some("app".to_string()));
    }

    #[test]
    fn parse_rfc5424() {
        let line = "<165>1 2026-03-11T10:00:00Z myhost myapp 1234 ID47 - Application started";
        let entry = parse_syslog_line(line, line);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.source, Some("myapp".to_string()));
        assert_eq!(entry.message, "Application started");
        assert_eq!(entry.level, Some(Level::Info)); // priority 165 = 20*8+5
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(severity_to_level(0), Level::Fatal); // emerg
        assert_eq!(severity_to_level(3), Level::Error); // err
        assert_eq!(severity_to_level(4), Level::Warn); // warning
        assert_eq!(severity_to_level(6), Level::Info); // info
        assert_eq!(severity_to_level(7), Level::Debug); // debug
    }
}
