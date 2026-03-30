use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{Level, LogEntry};

/// Parse a JSON structured log line.
///
/// Handles common JSON log formats:
/// - Pino/Bunyan: `{"level": 30, "msg": "...", "time": 1234567890}`
/// - tracing-subscriber: `{"timestamp": "...", "level": "INFO", "fields": {"message": "..."}}`
/// - Zap: `{"level": "info", "ts": 1234567890.123, "msg": "..."}`
/// - Generic: any JSON with level/msg/message/timestamp fields
pub fn parse_json_line(clean: &str, raw: &str) -> LogEntry {
    let parsed: Value = match serde_json::from_str(clean) {
        Ok(v) => v,
        Err(_) => {
            return LogEntry {
                timestamp: None,
                level: None,
                source: None,
                message: clean.to_string(),
                fields: HashMap::new(),
                raw: raw.to_string(),
            };
        }
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => {
            return LogEntry {
                timestamp: None,
                level: None,
                source: None,
                message: clean.to_string(),
                fields: HashMap::new(),
                raw: raw.to_string(),
            };
        }
    };

    // Extract level — try string first, then numeric (Pino/Bunyan convention)
    let level = extract_level(obj);

    // Extract message — check msg, message, fields.message
    let message = extract_message(obj);

    // Extract timestamp
    let timestamp = extract_timestamp(obj);

    // Extract source — check name, source, logger, module, component
    let source = ["name", "source", "logger", "module", "component"]
        .iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_str()).map(String::from));

    // Remaining fields (exclude already-extracted keys)
    let extracted_keys = [
        "level", "lvl", "severity",
        "msg", "message",
        "timestamp", "time", "ts", "@timestamp", "datetime", "date",
        "name", "source", "logger", "module", "component",
    ];
    let fields: HashMap<String, Value> = obj
        .iter()
        .filter(|(k, _)| !extracted_keys.contains(&k.as_str()))
        .filter(|(k, _)| {
            // Also skip "fields" if we extracted message from it
            k.as_str() != "fields"
                || !obj
                    .get("fields")
                    .and_then(|f| f.as_object())
                    .and_then(|f| f.get("message"))
                    .is_some()
        })
        .map(|(k, v)| (k.clone(), v.clone()))
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

fn extract_level(obj: &serde_json::Map<String, Value>) -> Option<Level> {
    // Try string-based level fields
    for key in &["level", "lvl", "severity"] {
        if let Some(val) = obj.get(*key) {
            // String level
            if let Some(s) = val.as_str() {
                if let Some(l) = Level::parse(s) {
                    return Some(l);
                }
            }
            // Numeric level (Pino/Bunyan: 10=trace, 20=debug, 30=info, 40=warn, 50=error, 60=fatal)
            if let Some(n) = val.as_u64() {
                if let Some(l) = Level::parse(&n.to_string()) {
                    return Some(l);
                }
            }
        }
    }
    None
}

fn extract_message(obj: &serde_json::Map<String, Value>) -> String {
    // Direct fields
    for key in &["msg", "message"] {
        if let Some(val) = obj.get(*key) {
            if let Some(s) = val.as_str() {
                return s.to_string();
            }
        }
    }
    // tracing-subscriber: fields.message
    if let Some(fields) = obj.get("fields").and_then(|v| v.as_object()) {
        if let Some(msg) = fields.get("message").and_then(|v| v.as_str()) {
            return msg.to_string();
        }
    }
    // Fallback: stringify the entire object
    serde_json::to_string(obj).unwrap_or_default()
}

fn extract_timestamp(obj: &serde_json::Map<String, Value>) -> Option<DateTime<Utc>> {
    for key in &["timestamp", "time", "ts", "@timestamp", "datetime", "date"] {
        if let Some(val) = obj.get(*key) {
            // ISO 8601 string
            if let Some(s) = val.as_str() {
                if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                    return Some(dt);
                }
                // Try common formats
                if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                    return Some(dt.with_timezone(&Utc));
                }
            }
            // Unix epoch seconds (Zap style: 1234567890.123)
            if let Some(f) = val.as_f64() {
                let secs = f as i64;
                let nanos = ((f - secs as f64) * 1_000_000_000.0) as u32;
                if let Some(dt) = DateTime::from_timestamp(secs, nanos) {
                    return Some(dt);
                }
            }
            // Unix epoch millis (Pino/Bunyan)
            if let Some(n) = val.as_i64() {
                // Heuristic: if > 1e12, it's millis; otherwise seconds
                if n > 1_000_000_000_000 {
                    if let Some(dt) = DateTime::from_timestamp_millis(n) {
                        return Some(dt);
                    }
                } else if let Some(dt) = DateTime::from_timestamp(n, 0) {
                    return Some(dt);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_generic_json() {
        let line = r#"{"timestamp":"2026-03-11T10:00:00Z","level":"error","msg":"connection refused","service":"api"}"#;
        let entry = parse_json_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.message, "connection refused");
        assert!(entry.timestamp.is_some());
        assert!(entry.fields.contains_key("service"));
    }

    #[test]
    fn parse_pino_numeric_level() {
        let line = r#"{"level":50,"time":1710151200000,"msg":"disk full"}"#;
        let entry = parse_json_line(line, line);
        assert_eq!(entry.level, Some(Level::Error));
        assert_eq!(entry.message, "disk full");
        assert!(entry.timestamp.is_some());
    }

    #[test]
    fn parse_tracing_subscriber() {
        let line = r#"{"timestamp":"2026-03-11T10:00:00Z","level":"INFO","fields":{"message":"request handled"},"target":"api::handler"}"#;
        let entry = parse_json_line(line, line);
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.message, "request handled");
    }

    #[test]
    fn parse_zap_float_timestamp() {
        let line = r#"{"level":"info","ts":1710151200.123,"msg":"started"}"#;
        let entry = parse_json_line(line, line);
        assert_eq!(entry.level, Some(Level::Info));
        assert_eq!(entry.message, "started");
        assert!(entry.timestamp.is_some());
    }

    #[test]
    fn parse_invalid_json_fallback() {
        let line = "not json at all";
        let entry = parse_json_line(line, line);
        assert_eq!(entry.message, "not json at all");
        assert!(entry.level.is_none());
    }
}
