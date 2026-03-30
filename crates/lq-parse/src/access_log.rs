use regex::Regex;
use std::sync::LazyLock;

use crate::model::{Level, LogEntry};

// Apache/Nginx Combined Log Format:
// 127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326 "http://www.example.com/start.html" "Mozilla/4.08"
static ACCESS_LOG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(\S+)\s+(\S+)\s+(\S+)\s+\[([^\]]+)\]\s+"([^"]+)"\s+(\d{3})\s+(\S+)(?:\s+"([^"]*)")?"#,
    )
    .unwrap()
});

/// Map HTTP status code to log level.
fn status_to_level(status: u16) -> Level {
    match status {
        200..=299 => Level::Info,
        300..=399 => Level::Info,
        400..=499 => Level::Warn,
        500..=599 => Level::Error,
        _ => Level::Info,
    }
}

/// Parse an Apache/Nginx combined access log line.
pub fn parse_access_line(line: &str, raw: &str) -> LogEntry {
    if let Some(caps) = ACCESS_LOG.captures(line) {
        let client_ip = &caps[1];
        let _ident = &caps[2];
        let _user = &caps[3];
        let timestamp_str = &caps[4];
        let request = &caps[5];
        let status: u16 = caps[6].parse().unwrap_or(0);
        let bytes = &caps[7];
        let referer = caps.get(8).map(|m| m.as_str());

        // Parse timestamp: 11/Mar/2026:10:00:00 +0000
        let timestamp = chrono::DateTime::parse_from_str(timestamp_str, "%d/%b/%Y:%H:%M:%S %z")
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let level = status_to_level(status);

        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "client_ip".to_string(),
            serde_json::Value::String(client_ip.to_string()),
        );
        fields.insert(
            "status".to_string(),
            serde_json::Value::Number(status.into()),
        );
        fields.insert(
            "bytes".to_string(),
            serde_json::Value::String(bytes.to_string()),
        );
        if let Some(ref_str) = referer {
            fields.insert(
                "referer".to_string(),
                serde_json::Value::String(ref_str.to_string()),
            );
        }

        LogEntry {
            timestamp,
            level: Some(level),
            source: None,
            message: request.to_string(),
            fields,
            raw: raw.to_string(),
        }
    } else {
        LogEntry {
            timestamp: None,
            level: None,
            source: None,
            message: line.to_string(),
            fields: Default::default(),
            raw: raw.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_combined_log() {
        let line = r#"127.0.0.1 - frank [11/Mar/2026:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 1234 "http://example.com" "Mozilla/5.0""#;
        let entry = parse_access_line(line, line);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.message, "GET /index.html HTTP/1.1");
        assert_eq!(entry.fields["status"], 200);
        assert_eq!(entry.fields["client_ip"], "127.0.0.1");
    }

    #[test]
    fn parse_404() {
        let line = r#"10.0.0.1 - - [11/Mar/2026:10:00:01 +0000] "GET /missing HTTP/1.1" 404 0"#;
        let entry = parse_access_line(line, line);
        assert_eq!(entry.level, Some(Level::Warn));
        assert_eq!(entry.fields["status"], 404);
    }

    #[test]
    fn parse_500() {
        let line = r#"10.0.0.2 - - [11/Mar/2026:10:00:02 +0000] "POST /api/data HTTP/1.1" 500 512"#;
        let entry = parse_access_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
    }
}
