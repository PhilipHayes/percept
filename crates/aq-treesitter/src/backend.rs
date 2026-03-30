use aq_core::backend::{Backend, BackendError};
use aq_core::node::OwnedNode;

use crate::langs::Language;
use crate::parse::ParsedTree;

/// Tree-sitter backend — wraps `ParsedTree::parse()` behind the `Backend` trait.
pub struct TreeSitterBackend;

impl Backend for TreeSitterBackend {
    fn parse(
        &self,
        source: &str,
        language: &str,
        file_path: Option<&str>,
    ) -> Result<OwnedNode, BackendError> {
        let lang = Language::from_name(language)
            .ok_or_else(|| BackendError::from(format!("Unsupported language: {language}")))?;

        let parsed = ParsedTree::parse(source.to_string(), lang, file_path.map(|s| s.to_string()))
            .map_err(|e| BackendError::from(e.to_string()))?;

        Ok(parsed.to_owned_node())
    }

    fn supported_languages(&self) -> Vec<&str> {
        vec![
            "c",
            "cpp",
            "dart",
            "go",
            "java",
            "javascript",
            "json",
            "python",
            "rust",
            "swift",
            "tsx",
            "typescript",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aq_core::backend::Backend;

    #[test]
    fn treesitter_backend_parses_rust() {
        let backend = TreeSitterBackend;
        let source = "fn main() { println!(\"hello\"); }";
        let result = backend.parse(source, "rust", Some("test.rs"));
        assert!(result.is_ok());
        let root = result.unwrap();
        assert_eq!(root.node_type, "source_file");
    }

    #[test]
    fn treesitter_backend_rejects_unknown_language() {
        let backend = TreeSitterBackend;
        let result = backend.parse("hello", "klingon", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Unsupported language"));
    }

    #[test]
    fn treesitter_backend_matches_direct_parse() {
        let backend = TreeSitterBackend;
        let source = "def greet(name):\n    print(f'Hello {name}')";

        // Via Backend trait
        let trait_result = backend.parse(source, "python", Some("test.py")).unwrap();

        // Via direct ParsedTree
        let direct = ParsedTree::parse(
            source.to_string(),
            Language::from_name("python").unwrap(),
            Some("test.py".to_string()),
        )
        .unwrap();
        let direct_result = direct.to_owned_node();

        assert_eq!(trait_result.node_type, direct_result.node_type);
        assert_eq!(trait_result.children.len(), direct_result.children.len());
        assert_eq!(trait_result.start_line, direct_result.start_line);
        assert_eq!(trait_result.end_line, direct_result.end_line);
    }

    #[test]
    fn treesitter_backend_lists_languages() {
        let backend = TreeSitterBackend;
        let langs = backend.supported_languages();
        assert!(langs.contains(&"rust"));
        assert!(langs.contains(&"python"));
        assert!(langs.contains(&"dart"));
        assert!(!langs.contains(&"english"));
    }
}
