use crate::model::{Level, LogEntry};

/// Parse a Docker CRI log line.
/// Format: `<timestamp> <stream> <flags> <message>`
/// Example: `2026-03-11T10:00:00.123456789Z stdout F Server started`
pub fn parse_cri_line(line: &str, raw: &str) -> LogEntry {
    let parts: Vec<&str> = line.splitn(4, ' ').collect();
    if parts.len() < 4 {
        return LogEntry {
            timestamp: None,
            level: None,
            source: None,
            message: line.to_string(),
            fields: Default::default(),
            raw: raw.to_string(),
        };
    }

    let timestamp = chrono::DateTime::parse_from_rfc3339(parts[0])
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let stream = parts[1]; // stdout or stderr
    let _flags = parts[2]; // F (full) or P (partial)
    let message = parts[3].to_string();

    // Infer level from stream + message content
    let level = if stream == "stderr" {
        Some(Level::Error)
    } else {
        // Try to extract level from first word of message
        message
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .and_then(Level::parse)
    };

    let mut fields = std::collections::HashMap::new();
    fields.insert(
        "stream".to_string(),
        serde_json::Value::String(stream.to_string()),
    );

    LogEntry {
        timestamp,
        level,
        source: None,
        message,
        fields,
        raw: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cri_stdout() {
        let line = "2026-03-11T10:00:00.123456789Z stdout F Server started on port 8080";
        let entry = parse_cri_line(line, line);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.message, "Server started on port 8080");
        assert_eq!(entry.fields["stream"], "stdout");
    }

    #[test]
    fn parse_cri_stderr() {
        let line = "2026-03-11T10:00:01.000Z stderr F Connection refused";
        let entry = parse_cri_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.message, "Connection refused");
    }

    #[test]
    fn parse_cri_with_level_in_message() {
        let line = "2026-03-11T10:00:02.000Z stdout F WARN: Disk usage high";
        let entry = parse_cri_line(line, line);
        assert_eq!(entry.level, Some(Level::Warn));
    }
}
