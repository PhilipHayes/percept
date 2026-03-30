use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use lq_parse::LogEntry;

use crate::drain::Drain;
use crate::filter::{parse_query, Filter};

/// A stage in the processing pipeline.
#[derive(Debug, Clone)]
pub enum Stage {
    /// Filter entries (AND of all filters).
    Filter(Vec<Filter>),
    /// Count entries grouped by a field.
    CountBy(String),
    /// Time-bucketed rate per window.
    Rate(Duration),
    /// Include N context lines around matches.
    Context(usize),
    /// Extract log template patterns via Drain algorithm.
    Patterns,
    /// Merge-sort entries by timestamp (for multi-file trace correlation).
    Timeline,
}

/// A full pipeline: filter stage followed by optional aggregation/output stages.
#[derive(Debug, Clone)]
pub struct Pipeline {
    pub stages: Vec<Stage>,
}

/// Parse a full query string including pipe operators.
/// Example: `level:error source:api | count by source`
/// Example: `level:error | rate 1m`
/// Example: `level:error | context 3`
pub fn parse_pipeline(query: &str) -> Pipeline {
    let parts: Vec<&str> = query.split('|').collect();
    let mut stages = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }

        if i == 0 {
            // First segment is always filters
            let filters = parse_query(trimmed);
            if !filters.is_empty() {
                stages.push(Stage::Filter(filters));
            }
        } else {
            // Subsequent segments are aggregation/output stages
            if let Some(stage) = parse_stage(trimmed) {
                stages.push(stage);
            }
        }
    }

    Pipeline { stages }
}

fn parse_stage(s: &str) -> Option<Stage> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    match tokens[0] {
        "count" => {
            // count by <field>
            if tokens.len() >= 3 && tokens[1] == "by" {
                Some(Stage::CountBy(tokens[2].to_string()))
            } else {
                Some(Stage::CountBy("level".to_string()))
            }
        }
        "rate" => {
            // rate <window>  e.g. rate 1m, rate 5s, rate 1h
            if tokens.len() >= 2 {
                parse_window(tokens[1]).map(Stage::Rate)
            } else {
                Some(Stage::Rate(Duration::minutes(1)))
            }
        }
        "context" | "ctx" => {
            // context N
            if tokens.len() >= 2 {
                tokens[1].parse::<usize>().ok().map(Stage::Context)
            } else {
                Some(Stage::Context(3))
            }
        }
        "patterns" | "templates" => Some(Stage::Patterns),
        "timeline" => Some(Stage::Timeline),
        _ => None,
    }
}

fn parse_window(s: &str) -> Option<Duration> {
    if s.len() < 2 {
        return None;
    }
    let (num_str, suffix) = s.split_at(s.len() - 1);
    let n: i64 = num_str.parse().ok()?;
    match suffix {
        "s" => Some(Duration::seconds(n)),
        "m" => Some(Duration::minutes(n)),
        "h" => Some(Duration::hours(n)),
        _ => None,
    }
}

/// Execute a pipeline against a vec of log entries, returning JSON output lines.
pub fn execute_pipeline(entries: &[LogEntry], pipeline: &Pipeline) -> Vec<Value> {
    let mut current: Vec<&LogEntry> = entries.iter().collect();
    let mut context_n: Option<usize> = None;

    // Apply filter + context stages
    for stage in &pipeline.stages {
        match stage {
            Stage::Filter(filters) => {
                current.retain(|e| filters.iter().all(|f| f.matches(e)));
            }
            Stage::Context(n) => {
                context_n = Some(*n);
            }
            _ => {}
        }
    }

    // Check for aggregation stages
    for stage in &pipeline.stages {
        match stage {
            Stage::CountBy(field) => {
                return vec![count_by(&current, field)];
            }
            Stage::Rate(window) => {
                return rate_bucket(&current, *window);
            }
            Stage::Patterns => {
                return extract_patterns(&current);
            }
            Stage::Timeline => {
                return timeline_sort(&current);
            }
            _ => {}
        }
    }

    // If context is requested, expand matches
    if let Some(n) = context_n {
        return apply_context(entries, &current, n);
    }

    // Default: output matched entries as JSON
    current
        .iter()
        .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
        .collect()
}

fn count_by(entries: &[&LogEntry], field: &str) -> Value {
    let mut counts: HashMap<String, u64> = HashMap::new();

    for entry in entries {
        let key = match field {
            "level" => entry
                .level
                .map(|l| format!("{:?}", l).to_lowercase())
                .unwrap_or_else(|| "unknown".into()),
            "source" => entry.source.clone().unwrap_or_else(|| "unknown".into()),
            other => entry
                .fields
                .get(other)
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_else(|| "unknown".into()),
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    serde_json::json!({
        "aggregation": "count",
        "field": field,
        "total": entries.len(),
        "groups": counts,
    })
}

fn rate_bucket(entries: &[&LogEntry], window: Duration) -> Vec<Value> {
    if entries.is_empty() {
        return vec![];
    }

    // Find time range
    let timestamps: Vec<DateTime<Utc>> = entries.iter().filter_map(|e| e.timestamp).collect();
    if timestamps.is_empty() {
        return vec![serde_json::json!({"error": "no timestamps for rate calculation"})];
    }

    let min_ts = *timestamps.iter().min().unwrap();
    let max_ts = *timestamps.iter().max().unwrap();

    let mut buckets: Vec<Value> = Vec::new();
    let mut bucket_start = min_ts;

    while bucket_start <= max_ts {
        let bucket_end = bucket_start + window;
        let count = timestamps
            .iter()
            .filter(|&&ts| ts >= bucket_start && ts < bucket_end)
            .count();

        buckets.push(serde_json::json!({
            "bucket": bucket_start.to_rfc3339(),
            "count": count,
            "window": format_duration(window),
        }));

        bucket_start = bucket_end;
    }

    buckets
}

fn format_duration(d: Duration) -> String {
    let secs = d.num_seconds();
    if secs >= 3600 && secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 && secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

fn extract_patterns(entries: &[&LogEntry]) -> Vec<Value> {
    let mut drain = Drain::new();
    for entry in entries {
        drain.process(&entry.message);
    }

    drain
        .patterns()
        .into_iter()
        .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
        .collect()
}

fn timeline_sort(entries: &[&LogEntry]) -> Vec<Value> {
    let mut sorted: Vec<&LogEntry> = entries.to_vec();
    sorted.sort_by(|a, b| {
        let ta = a.timestamp.unwrap_or(DateTime::<Utc>::MIN_UTC);
        let tb = b.timestamp.unwrap_or(DateTime::<Utc>::MIN_UTC);
        ta.cmp(&tb)
    });
    sorted
        .iter()
        .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
        .collect()
}

fn apply_context(all: &[LogEntry], matched: &[&LogEntry], n: usize) -> Vec<Value> {
    // Find indices of matched entries by comparing raw strings
    let match_raws: Vec<&str> = matched.iter().map(|e| e.raw.as_str()).collect();
    let match_indices: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, entry)| match_raws.contains(&entry.raw.as_str()))
        .map(|(i, _)| i)
        .collect();

    // Expand to include context lines
    let mut included = vec![false; all.len()];
    for &idx in &match_indices {
        let start = idx.saturating_sub(n);
        let end = (idx + n + 1).min(all.len());
        included[start..end].fill(true);
    }

    all.iter()
        .enumerate()
        .filter(|(i, _)| included[*i])
        .map(|(_, e)| serde_json::to_value(e).unwrap_or(Value::Null))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lq_parse::Level;
    use std::collections::HashMap;

    fn make_entry(
        ts: Option<&str>,
        level: Option<Level>,
        source: Option<&str>,
        msg: &str,
    ) -> LogEntry {
        LogEntry {
            timestamp: ts.map(|t| t.parse::<DateTime<Utc>>().unwrap()),
            level,
            source: source.map(String::from),
            message: msg.into(),
            fields: HashMap::new(),
            raw: msg.into(),
        }
    }

    #[test]
    fn parse_simple_pipeline() {
        let p = parse_pipeline("level:error | count by source");
        assert_eq!(p.stages.len(), 2);
        assert!(matches!(p.stages[0], Stage::Filter(_)));
        assert!(matches!(p.stages[1], Stage::CountBy(ref f) if f == "source"));
    }

    #[test]
    fn parse_rate_pipeline() {
        let p = parse_pipeline("level:error | rate 5m");
        assert_eq!(p.stages.len(), 2);
        assert!(matches!(p.stages[1], Stage::Rate(d) if d == Duration::minutes(5)));
    }

    #[test]
    fn parse_context_pipeline() {
        let p = parse_pipeline("\"panic\" | context 5");
        assert_eq!(p.stages.len(), 2);
        assert!(matches!(p.stages[1], Stage::Context(5)));
    }

    #[test]
    fn count_by_level() {
        let entries = vec![
            make_entry(None, Some(Level::Error), None, "boom"),
            make_entry(None, Some(Level::Error), None, "crash"),
            make_entry(None, Some(Level::Info), None, "ok"),
        ];
        let p = parse_pipeline("| count by level");
        let results = execute_pipeline(&entries, &p);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["total"], 3);
        assert_eq!(results[0]["groups"]["error"], 2);
        assert_eq!(results[0]["groups"]["info"], 1);
    }

    #[test]
    fn count_by_source_with_filter() {
        let entries = vec![
            make_entry(None, Some(Level::Error), Some("api"), "boom"),
            make_entry(None, Some(Level::Error), Some("db"), "crash"),
            make_entry(None, Some(Level::Info), Some("api"), "ok"),
        ];
        let p = parse_pipeline("level:error | count by source");
        let results = execute_pipeline(&entries, &p);
        assert_eq!(results[0]["total"], 2);
        assert_eq!(results[0]["groups"]["api"], 1);
        assert_eq!(results[0]["groups"]["db"], 1);
    }

    #[test]
    fn rate_buckets() {
        let entries = vec![
            make_entry(Some("2026-03-11T10:00:00Z"), Some(Level::Error), None, "a"),
            make_entry(Some("2026-03-11T10:00:30Z"), Some(Level::Error), None, "b"),
            make_entry(Some("2026-03-11T10:01:15Z"), Some(Level::Error), None, "c"),
        ];
        let p = parse_pipeline("| rate 1m");
        let results = execute_pipeline(&entries, &p);
        assert_eq!(results.len(), 2); // two 1-minute buckets
        assert_eq!(results[0]["count"], 2);
        assert_eq!(results[1]["count"], 1);
    }

    #[test]
    fn context_lines() {
        let entries: Vec<LogEntry> = (0..10)
            .map(|i| make_entry(None, Some(Level::Info), None, &format!("line {}", i)))
            .collect();
        // Only line 5 has "line 5"
        let p = parse_pipeline("\"line 5\" | context 2");
        let results = execute_pipeline(&entries, &p);
        // Should include lines 3,4,5,6,7
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn since_until_filters() {
        let entries = vec![
            make_entry(
                Some("2026-03-11T08:00:00Z"),
                Some(Level::Info),
                None,
                "early",
            ),
            make_entry(
                Some("2026-03-11T12:00:00Z"),
                Some(Level::Info),
                None,
                "noon",
            ),
            make_entry(
                Some("2026-03-11T18:00:00Z"),
                Some(Level::Info),
                None,
                "late",
            ),
        ];
        let filters = parse_query("since:2026-03-11T10:00:00Z until:2026-03-11T14:00:00Z");
        let matched: Vec<&LogEntry> = entries
            .iter()
            .filter(|e| filters.iter().all(|f| f.matches(e)))
            .collect();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].message, "noon");
    }

    #[test]
    fn patterns_extraction() {
        let entries = vec![
            make_entry(
                None,
                Some(Level::Error),
                None,
                "Connection refused to host 10.0.0.1",
            ),
            make_entry(
                None,
                Some(Level::Error),
                None,
                "Connection refused to host 10.0.0.2",
            ),
            make_entry(
                None,
                Some(Level::Error),
                None,
                "Connection refused to host 10.0.0.3",
            ),
            make_entry(None, Some(Level::Info), None, "Disk full on /dev/sda1"),
            make_entry(None, Some(Level::Info), None, "Disk full on /dev/sdb1"),
        ];
        let p = parse_pipeline("| patterns");
        let results = execute_pipeline(&entries, &p);
        assert_eq!(results.len(), 2);
        // Most frequent pattern first
        assert_eq!(results[0]["count"], 3);
        assert!(results[0]["template"].as_str().unwrap().contains("<*>"));
    }

    #[test]
    fn timeline_sorts_by_timestamp() {
        let entries = vec![
            make_entry(
                Some("2026-03-11T12:00:00Z"),
                Some(Level::Info),
                None,
                "second",
            ),
            make_entry(
                Some("2026-03-11T08:00:00Z"),
                Some(Level::Info),
                None,
                "first",
            ),
            make_entry(
                Some("2026-03-11T18:00:00Z"),
                Some(Level::Info),
                None,
                "third",
            ),
        ];
        let p = parse_pipeline("| timeline");
        let results = execute_pipeline(&entries, &p);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["message"], "first");
        assert_eq!(results[1]["message"], "second");
        assert_eq!(results[2]["message"], "third");
    }
}
