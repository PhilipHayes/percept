use regex::Regex;
use std::sync::LazyLock;

/// Strip ANSI escape codes (colors, cursor control) from a string.
pub fn strip_ansi(input: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());
    RE.replace_all(input, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_color_codes() {
        let input = "\x1b[31mERROR\x1b[0m something failed";
        assert_eq!(strip_ansi(input), "ERROR something failed");
    }

    #[test]
    fn passes_through_clean_text() {
        assert_eq!(strip_ansi("no colors here"), "no colors here");
    }

    #[test]
    fn strips_bold_and_multi() {
        let input = "\x1b[1;33mWARN\x1b[0m: \x1b[36mapi\x1b[0m timeout";
        assert_eq!(strip_ansi(input), "WARN: api timeout");
    }
}
