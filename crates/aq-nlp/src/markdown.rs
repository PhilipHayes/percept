//! Markdown preprocessing for corpus NLP pipeline.
//!
//! Strips formatting artifacts that would confuse spaCy entity extraction:
//! YAML frontmatter, wiki-links, headers, bold/italic, code blocks/inline.

/// Preprocess markdown text for NLP parsing.
/// Returns `(normalized_text, raw_copy)` where `normalized_text` has markup stripped
/// and `raw_copy` is the original input preserved for provenance.
pub fn preprocess_markdown(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return (String::new(), String::new());
    }

    let content = strip_frontmatter(raw);
    let content = strip_code_blocks(&content);
    let content = strip_wiki_links(&content);
    let content = strip_headers(&content);
    let content = strip_formatting(&content);
    let content = strip_inline_code(&content);

    (content, raw.to_string())
}

fn strip_frontmatter(s: &str) -> String {
    // Only strip if document starts with ---
    if !s.starts_with("---") {
        return s.to_string();
    }
    // Find closing --- (must be on its own line after the opening)
    let after_open = &s[3..];
    let rest = after_open.trim_start_matches(|c: char| c != '\n');
    if let Some(end) = rest.find("\n---") {
        let after_close = &rest[end + 4..];
        // Skip any trailing newline right after the closing ---
        after_close
            .strip_prefix('\n')
            .unwrap_or(after_close)
            .to_string()
    } else {
        s.to_string()
    }
}

fn strip_code_blocks(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_block = false;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if !in_block {
            result.push_str(line);
            result.push('\n');
        }
    }
    // Remove trailing newline we added
    if result.ends_with('\n') && !s.ends_with('\n') {
        result.pop();
    }
    result
}

fn strip_wiki_links(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    let bytes = s.as_bytes();
    let len = bytes.len();
    while i < len {
        if i + 1 < len && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            i += 2; // skip [[
            let start = i;
            // Find closing ]]
            while i + 1 < len && !(bytes[i] == b']' && bytes[i + 1] == b']') {
                i += 1;
            }
            let link_content = &s[start..i];
            if i + 1 < len {
                i += 2; // skip ]]
            }
            // [[target|display]] → display; [[target]] → target
            if let Some(pos) = link_content.find('|') {
                result.push_str(&link_content[pos + 1..]);
            } else {
                result.push_str(link_content);
            }
        } else {
            result.push(s[i..].chars().next().unwrap());
            i += s[i..].chars().next().unwrap().len_utf8();
        }
    }
    result
}

fn strip_headers(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for line in s.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let stripped = trimmed.trim_start_matches('#').trim_start();
            result.push_str(stripped);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    if result.ends_with('\n') && !s.ends_with('\n') {
        result.pop();
    }
    result
}

fn strip_formatting(s: &str) -> String {
    // Strip **bold**, *italic*, __bold__, _italic_
    // Process longer markers first to avoid partial matches
    let s = strip_marker(s, "**");
    let s = strip_marker(&s, "__");
    let s = strip_marker(&s, "*");
    strip_marker(&s, "_")
}

fn strip_marker(s: &str, marker: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(marker) {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + marker.len()..];
        if let Some(end) = after_open.find(marker) {
            result.push_str(&after_open[..end]);
            rest = &after_open[end + marker.len()..];
        } else {
            // No closing marker — keep the opening marker as-is
            result.push_str(marker);
            rest = after_open;
        }
    }
    result.push_str(rest);
    result
}

fn strip_inline_code(s: &str) -> String {
    strip_marker(s, "`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter() {
        let input = "---\ntags: [x]\ntitle: Test\n---\nHello world";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_strip_frontmatter_no_frontmatter() {
        let input = "Just a plain document.";
        let (result, raw) = preprocess_markdown(input);
        assert_eq!(result, "Just a plain document.");
        assert_eq!(raw, "Just a plain document.");
    }

    #[test]
    fn test_strip_frontmatter_mid_document() {
        let input = "Before\n---\nAfter";
        let (result, _) = preprocess_markdown(input);
        assert!(result.contains("Before"));
        assert!(result.contains("---"));
        assert!(result.contains("After"));
    }

    #[test]
    fn test_wiki_link_with_alias() {
        let input = "See [[Joseph (patriarch)|Joseph]] for details.";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "See Joseph for details.");
    }

    #[test]
    fn test_wiki_link_without_alias() {
        let input = "See [[Joseph]] for details.";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "See Joseph for details.");
    }

    #[test]
    fn test_strip_headers() {
        let input = "# Title\n## Subtitle\nParagraph text.";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "Title\nSubtitle\nParagraph text.");
    }

    #[test]
    fn test_strip_bold_italic() {
        let input = "This is **bold** and *italic* text.";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "This is bold and italic text.");
    }

    #[test]
    fn test_strip_code_blocks() {
        let input = "Before\n```python\nprint('hello')\n```\nAfter";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "Before\nAfter");
    }

    #[test]
    fn test_strip_inline_code() {
        let input = "Use the `nq index` command.";
        let (result, _) = preprocess_markdown(input);
        assert_eq!(result, "Use the nq index command.");
    }

    #[test]
    fn test_preserves_paragraph_structure() {
        let input = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let (result, _) = preprocess_markdown(input);
        assert!(result.contains("Paragraph one.\n\nParagraph two.\n\nParagraph three."));
    }

    #[test]
    fn test_empty_input() {
        let (result, raw) = preprocess_markdown("");
        assert_eq!(result, "");
        assert_eq!(raw, "");
    }

    #[test]
    fn test_complex_obsidian_note() {
        let input = "---\ntags: [genesis, narrative]\ndate: 2026-03-15\n---\n\
                     # Joseph's Journey\n\n\
                     **Joseph** dreamed a dream and told it to his [[brothers|siblings]].\n\n\
                     ## The Pit\n\n\
                     They cast him into a `pit` in [[Dothan]].\n\n\
                     ```\nSome code block\nto remove\n```\n\n\
                     *Jacob* mourned for many days.";
        let (result, raw) = preprocess_markdown(input);
        assert!(!result.contains("tags:"), "frontmatter should be stripped");
        assert!(!result.contains("# "), "headers should be stripped");
        assert!(result.contains("Joseph's Journey"), "header text preserved");
        assert!(result.contains("Joseph dreamed"), "bold stripped");
        assert!(result.contains("siblings"), "wiki-link alias extracted");
        assert!(result.contains("Dothan"), "wiki-link target extracted");
        assert!(result.contains("pit"), "inline code stripped");
        assert!(!result.contains("Some code block"), "code block removed");
        assert!(result.contains("Jacob mourned"), "italic stripped");
        assert_eq!(raw, input, "raw copy preserved");
    }
}
