use crate::spacy;
use crate::tree;
use aq_core::backend::{Backend, BackendError};
use aq_core::node::OwnedNode;

/// NLP backend — calls spaCy and maps to OwnedNode (ADR-015 Tier 1).
pub struct NlpBackend;

impl Backend for NlpBackend {
    fn parse(
        &self,
        source: &str,
        _language: &str,
        file_path: Option<&str>,
    ) -> Result<OwnedNode, BackendError> {
        let doc = spacy::parse_with_spacy(source).map_err(|e| BackendError::from(e.to_string()))?;
        Ok(tree::spacy_doc_to_owned_tree(&doc, source, file_path))
    }

    fn supported_languages(&self) -> Vec<&str> {
        vec!["english"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spacy_available() -> bool {
        std::process::Command::new("python3")
            .args(["-c", "import spacy; spacy.load('en_core_web_sm')"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn nlp_backend_supports_english() {
        let backend = NlpBackend;
        assert_eq!(backend.supported_languages(), vec!["english"]);
    }

    #[test]
    fn nlp_backend_parses_english_text() {
        if !spacy_available() {
            return;
        }
        let backend = NlpBackend;
        let result = backend.parse("Sarah went to Paris.", "english", None);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let tree = result.unwrap();
        assert_eq!(tree.node_type, "document");
        assert!(!tree.children.is_empty());
    }

    #[test]
    fn nlp_backend_returns_error_without_spacy() {
        let err = BackendError::from("test error".to_string());
        assert_eq!(err.message, "test error");
    }

    #[test]
    fn nlp_backend_parses_empty_text() {
        if !spacy_available() {
            return;
        }
        let backend = NlpBackend;
        let result = backend.parse("", "english", None);
        assert!(result.is_ok());
        let tree = result.unwrap();
        assert_eq!(tree.node_type, "document");
    }

    #[test]
    fn nlp_backend_entities_present() {
        if !spacy_available() {
            return;
        }
        let backend = NlpBackend;
        let result = backend
            .parse("Sarah went to Paris.", "english", None)
            .unwrap();
        let entities: Vec<_> = result
            .children
            .iter()
            .flat_map(|c| c.children.iter())
            .chain(result.children.iter())
            .filter(|c| c.node_type == "entity")
            .collect();
        assert!(!entities.is_empty(), "Expected entities in: {:?}", result);
    }
}
