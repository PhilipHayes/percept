use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::model::{Level, LogEntry};

/// Parse a logfmt line: `key=value key=value key="quoted value"`.
pub fn parse_logfmt_line(clean: &str, raw: &str) -> LogEntry {
    let pairs = parse_pairs(clean);

    let level = pairs
        .get("level")
        .or_else(|| pairs.get("lvl"))
        .or_else(|| pairs.get("severity"))
        .and_then(|v| Level::parse(v));

    let message = pairs
        .get("msg")
        .or_else(|| pairs.get("message"))
        .cloned()
        .unwrap_or_default();

    let timestamp = pairs
        .get("time")
        .or_else(|| pairs.get("timestamp"))
        .or_else(|| pairs.get("ts"))
        .and_then(|v| v.parse::<DateTime<Utc>>().ok());

    let source = pairs
        .get("source")
        .or_else(|| pairs.get("logger"))
        .or_else(|| pairs.get("component"))
        .or_else(|| pairs.get("caller"))
        .cloned();

    let extracted_keys = [
        "level",
        "lvl",
        "severity",
        "msg",
        "message",
        "time",
        "timestamp",
        "ts",
        "source",
        "logger",
        "component",
        "caller",
    ];
    let fields: HashMap<String, serde_json::Value> = pairs
        .iter()
        .filter(|(k, _)| !extracted_keys.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();

    LogEntry {
        timestamp,
        level,
        source,
        message,
        fields,
        raw: raw.to_string(),
    }
}

/// Parse key=value pairs from a logfmt line.
/// Handles quoted values: `key="value with spaces"`.
fn parse_pairs(line: &str) -> HashMap<String, String> {
    let mut pairs = HashMap::new();
    let mut chars = line.chars().peekable();

    loop {
        // Skip whitespace
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        // Read key (up to '=')
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }

        // Expect '='
        if chars.peek() != Some(&'=') {
            // Not a key=value pair — skip this token
            while chars.peek().is_some_and(|c| !c.is_whitespace()) {
                chars.next();
            }
            continue;
        }
        chars.next(); // consume '='

        // Read value
        let value = if chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            let mut val = String::new();
            let iter = chars.by_ref();
            while let Some(c) = iter.next() {
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    if let Some(escaped) = iter.next() {
                        val.push(escaped);
                        continue;
                    }
                }
                val.push(c);
            }
            val
        } else {
            let mut val = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                val.push(c);
                chars.next();
            }
            val
        };

        if !key.is_empty() {
            pairs.insert(key, value);
        }
    }

    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_logfmt() {
        let line = "time=2026-03-11T10:00:00Z level=error msg=\"connection refused\" service=api";
        let entry = parse_logfmt_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.message, "connection refused");
        assert!(entry.timestamp.is_some());
        assert!(entry.fields.contains_key("service"));
    }

    #[test]
    fn parse_unquoted_values() {
        let line = "level=info msg=started service=api port=8080";
        let entry = parse_logfmt_line(line, line);
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.message, "started");
    }

    #[test]
    fn parse_pairs_handles_quotes() {
        let pairs = parse_pairs(r#"key="value with spaces" other=simple"#);
        assert_eq!(pairs.get("key").unwrap(), "value with spaces");
        assert_eq!(pairs.get("other").unwrap(), "simple");
    }

    #[test]
    fn parse_pairs_handles_escaped_quotes() {
        let pairs = parse_pairs(r#"msg="said \"hello\"" level=info"#);
        assert_eq!(pairs.get("msg").unwrap(), "said \"hello\"");
    }
}
