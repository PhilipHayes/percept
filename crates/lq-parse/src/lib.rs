pub mod access_log;
pub mod ansi;
pub mod bracket;
pub mod build_tool;
pub mod detect;
pub mod docker_cri;
pub mod json;
pub mod logfmt;
pub mod model;
pub mod syslog;

pub use detect::{detect_format, Format};
pub use model::{Level, LogEntry};

/// Parse a single raw line into a LogEntry using the given format.
pub fn parse_line(line: &str, format: Format) -> LogEntry {
    let clean = ansi::strip_ansi(line);
    match format {
        Format::Json => json::parse_json_line(&clean, line),
        Format::Logfmt => logfmt::parse_logfmt_line(&clean, line),
        Format::Bracket => bracket::parse_bracket_line(&clean, line),
        Format::DockerCri => docker_cri::parse_cri_line(&clean, line),
        Format::Syslog => syslog::parse_syslog_line(&clean, line),
        Format::AccessLog => access_log::parse_access_line(&clean, line),
        Format::BuildTool => build_tool::parse_build_line(&clean, line),
        Format::Unknown => LogEntry {
            timestamp: None,
            level: None,
            source: None,
            message: clean.to_string(),
            fields: Default::default(),
            raw: line.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_format_preserves_message() {
        let entry = parse_line("just plain text", Format::Unknown);
        assert_eq!(entry.message, "just plain text");
    }
}
