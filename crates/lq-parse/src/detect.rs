use serde::Serialize;

/// Detected log format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Logfmt,
    Bracket,
    DockerCri,
    Syslog,
    AccessLog,
    BuildTool,
    Unknown,
}

/// Auto-detect the log format by inspecting a sample of lines.
/// Returns the most likely format based on content heuristics.
pub fn detect_format(lines: &[&str]) -> Format {
    if lines.is_empty() {
        return Format::Unknown;
    }

    let mut json_score = 0u32;
    let mut logfmt_score = 0u32;
    let mut bracket_score = 0u32;
    let mut cri_score = 0u32;
    let mut syslog_score = 0u32;
    let mut access_score = 0u32;
    let mut build_score = 0u32;
    let sample_size = lines.len().min(20);

    for line in &lines[..sample_size] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // JSON: starts with {
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            json_score += 2;
        } else if trimmed.starts_with('{') {
            json_score += 1;
        }

        // Docker CRI: timestamp stdout/stderr F/P message
        if trimmed.len() > 30 {
            let parts: Vec<&str> = trimmed.splitn(4, ' ').collect();
            if parts.len() >= 4
                && (parts[1] == "stdout" || parts[1] == "stderr")
                && (parts[2] == "F" || parts[2] == "P")
            {
                cri_score += 3;
            }
        }

        // Syslog: starts with <priority>
        if trimmed.starts_with('<') && trimmed.len() > 3 {
            if let Some(end) = trimmed.find('>') {
                if end <= 4 && trimmed[1..end].chars().all(|c| c.is_ascii_digit()) {
                    syslog_score += 3;
                }
            }
        }

        // Access log: IP - user [timestamp] "request" status
        if trimmed.contains("] \"") && trimmed.contains("HTTP/") {
            access_score += 3;
        }

        // Build tool: error[Exxxx]: or warning: or --> file:line:col
        if trimmed.starts_with("error") || trimmed.starts_with("warning:") || trimmed.trim_start().starts_with("--> ") {
            build_score += 2;
        }

        // Bracket: [timestamp] LEVEL or [LEVEL] message
        if trimmed.starts_with('[') && contains_level_keyword(trimmed) {
            bracket_score += 2;
        }

        // Bracket variant: TIMESTAMP [source] [level] message (e.g. Claude MCP logs)
        if trimmed.len() > 20
            && trimmed.as_bytes()[0].is_ascii_digit()
            && trimmed.contains("] [")
            && contains_bracketed_level(trimmed)
        {
            bracket_score += 2;
        }

        // logfmt: key=value key=value (at least 2 pairs)
        let kv_count = trimmed
            .split_whitespace()
            .filter(|w| {
                let parts: Vec<&str> = w.splitn(2, '=').collect();
                parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty()
            })
            .count();
        if kv_count >= 2 {
            logfmt_score += 2;
        }
    }

    let scores = [
        (cri_score, Format::DockerCri),
        (syslog_score, Format::Syslog),
        (access_score, Format::AccessLog),
        (json_score, Format::Json),
        (build_score, Format::BuildTool),
        (logfmt_score, Format::Logfmt),
        (bracket_score, Format::Bracket),
    ];

    scores
        .iter()
        .filter(|(score, _)| *score > 0)
        .max_by_key(|(score, _)| *score)
        .map(|(_, fmt)| *fmt)
        .unwrap_or(Format::Unknown)
}

fn contains_level_keyword(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    ["TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL", "CRITICAL"]
        .iter()
        .any(|kw| upper.contains(kw))
}

/// Check for level keywords inside brackets, e.g. `[info]`, `[ERROR]`.
fn contains_bracketed_level(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    ["[TRACE]", "[DEBUG]", "[INFO]", "[WARN]", "[WARNING]", "[ERROR]", "[ERR]", "[FATAL]", "[CRITICAL]", "[VERBOSE]"]
        .iter()
        .any(|kw| upper.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_json() {
        let lines = vec![
            r#"{"timestamp":"2026-03-11T10:00:00Z","level":"info","msg":"started"}"#,
            r#"{"timestamp":"2026-03-11T10:00:01Z","level":"error","msg":"failed"}"#,
        ];
        assert_eq!(detect_format(&lines), Format::Json);
    }

    #[test]
    fn detect_logfmt() {
        let lines = vec![
            "time=2026-03-11T10:00:00Z level=info msg=started service=api",
            "time=2026-03-11T10:00:01Z level=error msg=failed service=api",
        ];
        assert_eq!(detect_format(&lines), Format::Logfmt);
    }

    #[test]
    fn detect_bracket() {
        let lines = vec![
            "[2026-03-11 10:00:00] INFO  api: Server started on port 8080",
            "[2026-03-11 10:00:01] ERROR api: Connection refused",
        ];
        assert_eq!(detect_format(&lines), Format::Bracket);
    }

    #[test]
    fn detect_empty_is_unknown() {
        assert_eq!(detect_format(&[]), Format::Unknown);
    }

    #[test]
    fn detect_plain_text_is_unknown() {
        let lines = vec!["just some text", "another line"];
        assert_eq!(detect_format(&lines), Format::Unknown);
    }

    #[test]
    fn detect_docker_cri() {
        let lines = vec![
            "2026-03-11T10:00:00.123456789Z stdout F Server started",
            "2026-03-11T10:00:01.000Z stderr F Error occurred",
        ];
        assert_eq!(detect_format(&lines), Format::DockerCri);
    }

    #[test]
    fn detect_syslog() {
        let lines = vec![
            "<14>Mar 11 10:00:00 myhost app: Service started",
            "<11>Mar 11 10:00:01 myhost app: Error occurred",
        ];
        assert_eq!(detect_format(&lines), Format::Syslog);
    }

    #[test]
    fn detect_access_log() {
        let lines = vec![
            r#"127.0.0.1 - frank [11/Mar/2026:10:00:00 +0000] "GET /index.html HTTP/1.1" 200 1234"#,
            r#"10.0.0.1 - - [11/Mar/2026:10:00:01 +0000] "POST /api HTTP/1.1" 201 512"#,
        ];
        assert_eq!(detect_format(&lines), Format::AccessLog);
    }

    #[test]
    fn detect_build_tool() {
        let lines = vec![
            "error[E0308]: mismatched types",
            "  --> src/main.rs:10:5",
            "warning: unused variable",
        ];
        assert_eq!(detect_format(&lines), Format::BuildTool);
    }

    #[test]
    fn detect_bracket_timestamp_source_level() {
        let lines = vec![
            "2025-07-20T00:10:07.796Z [Filesystem] [info] Initializing server... { metadata: undefined }",
            "2025-07-20T00:10:07.801Z [Filesystem] [info] Using built-in Node.js for MCP server: Filesystem { metadata: undefined }",
            "2025-07-20T00:10:07.804Z [Filesystem] [error] Server disconnected. { metadata: { context: 'connection' } }",
        ];
        assert_eq!(detect_format(&lines), Format::Bracket);
    }
}
