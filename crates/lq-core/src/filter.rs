use chrono::{DateTime, Duration, Utc};
use lq_parse::{Level, LogEntry};

/// A filter predicate that can be applied to log entries.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Match entries with this level or higher.
    Level(Level),
    /// Match entries whose source contains this string (case-insensitive).
    Source(String),
    /// Match entries whose message contains this string (case-insensitive).
    Text(String),
    /// Match entries at or after this timestamp.
    Since(DateTime<Utc>),
    /// Match entries at or before this timestamp.
    Until(DateTime<Utc>),
}

impl Filter {
    pub fn matches(&self, entry: &LogEntry) -> bool {
        match self {
            Filter::Level(min) => entry.level.map_or(false, |l| l >= *min),
            Filter::Source(s) => entry
                .source
                .as_ref()
                .map_or(false, |src| src.to_lowercase().contains(&s.to_lowercase())),
            Filter::Text(t) => {
                let lower = t.to_lowercase();
                entry.message.to_lowercase().contains(&lower)
            }
            Filter::Since(ts) => entry.timestamp.map_or(false, |t| t >= *ts),
            Filter::Until(ts) => entry.timestamp.map_or(false, |t| t <= *ts),
        }
    }
}

/// Apply all filters (AND logic) to an entry.
pub fn apply_filters(filters: &[Filter], entry: &LogEntry) -> bool {
    filters.iter().all(|f| f.matches(entry))
}

/// Parse a simple query string into filters.
/// Supported syntax: `level:error`, `source:api`, `"text match"`, bare words.
pub fn parse_query(query: &str) -> Vec<Filter> {
    let mut filters = Vec::new();
    let mut chars = query.chars().peekable();
    let mut token = String::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '"' => {
                chars.next();
                let mut quoted = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    quoted.push(c);
                }
                if !quoted.is_empty() {
                    filters.push(Filter::Text(quoted));
                }
            }
            ' ' | '\t' => {
                if !token.is_empty() {
                    filters.push(parse_token(&token));
                    token.clear();
                }
                chars.next();
            }
            '|' => {
                // Pipeline separator — stop filter parsing here.
                // Downstream stages will handle the rest.
                if !token.is_empty() {
                    filters.push(parse_token(&token));
                }
                break;
            }
            _ => {
                token.push(ch);
                chars.next();
            }
        }
    }

    if !token.is_empty() {
        filters.push(parse_token(&token));
    }

    filters
}

fn parse_token(token: &str) -> Filter {
    if let Some(val) = token.strip_prefix("level:") {
        if let Some(level) = Level::parse(val) {
            return Filter::Level(level);
        }
    }
    if let Some(val) = token.strip_prefix("source:") {
        return Filter::Source(val.to_string());
    }
    if let Some(val) = token.strip_prefix("since:") {
        if let Some(ts) = parse_time_spec(val) {
            return Filter::Since(ts);
        }
    }
    if let Some(val) = token.strip_prefix("until:") {
        if let Some(ts) = parse_time_spec(val) {
            return Filter::Until(ts);
        }
    }
    Filter::Text(token.to_string())
}

/// Parse a time specification: either a relative duration like "1h", "30m", "2d"
/// (interpreted as "now minus duration") or an absolute ISO 8601 timestamp.
fn parse_time_spec(spec: &str) -> Option<DateTime<Utc>> {
    // Try relative duration first: 30s, 5m, 1h, 2d
    if let Some(dur) = parse_relative_duration(spec) {
        return Some(Utc::now() - dur);
    }
    // Try absolute ISO 8601
    if let Ok(ts) = spec.parse::<DateTime<Utc>>() {
        return Some(ts);
    }
    // Try date-only (assume start of day UTC)
    if let Ok(date) = chrono::NaiveDate::parse_from_str(spec, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc());
    }
    None
}

fn parse_relative_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let (num_str, suffix) = s.split_at(s.len() - 1);
    let n: i64 = num_str.parse().ok()?;
    match suffix {
        "s" => Some(Duration::seconds(n)),
        "m" => Some(Duration::minutes(n)),
        "h" => Some(Duration::hours(n)),
        "d" => Some(Duration::days(n)),
        "w" => Some(Duration::weeks(n)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lq_parse::LogEntry;
    use std::collections::HashMap;

    fn entry(level: Option<Level>, source: Option<&str>, msg: &str) -> LogEntry {
        LogEntry {
            timestamp: None,
            level,
            source: source.map(String::from),
            message: msg.into(),
            fields: HashMap::new(),
            raw: msg.into(),
        }
    }

    #[test]
    fn filter_level() {
        let f = Filter::Level(Level::Error);
        assert!(f.matches(&entry(Some(Level::Error), None, "boom")));
        assert!(f.matches(&entry(Some(Level::Fatal), None, "boom")));
        assert!(!f.matches(&entry(Some(Level::Warn), None, "hmm")));
        assert!(!f.matches(&entry(None, None, "no level")));
    }

    #[test]
    fn filter_source() {
        let f = Filter::Source("api".into());
        assert!(f.matches(&entry(None, Some("api-server"), "x")));
        assert!(!f.matches(&entry(None, Some("worker"), "x")));
    }

    #[test]
    fn filter_text() {
        let f = Filter::Text("refused".into());
        assert!(f.matches(&entry(None, None, "Connection refused")));
        assert!(!f.matches(&entry(None, None, "Connection reset")));
    }

    #[test]
    fn parse_query_mixed() {
        let filters = parse_query("level:error source:api \"connection refused\"");
        assert_eq!(filters.len(), 3);
    }

    #[test]
    fn parse_query_stops_at_pipe() {
        let filters = parse_query("level:error | count by source");
        assert_eq!(filters.len(), 1);
    }
}
