use std::fmt;

use crate::node::OwnedNode;

/// Error returned by a [`Backend`] when parsing fails.
#[derive(Debug, Clone)]
pub struct BackendError {
    pub message: String,
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

impl From<String> for BackendError {
    fn from(s: String) -> Self {
        Self { message: s }
    }
}

impl From<&str> for BackendError {
    fn from(s: &str) -> Self {
        Self {
            message: s.to_string(),
        }
    }
}

/// Trait abstracting over parse backends (tree-sitter, NLP, etc.).
///
/// Each backend takes source text and a language identifier string,
/// and returns an `OwnedNode` tree. The language identifier is a plain
/// `&str` so different backends can accept identifiers the other doesn't
/// know about (e.g. `"english"` for NLP, `"rust"` for tree-sitter).
pub trait Backend: Send + Sync {
    /// Parse source text into an `OwnedNode` tree.
    fn parse(
        &self,
        source: &str,
        language: &str,
        file_path: Option<&str>,
    ) -> Result<OwnedNode, BackendError>;

    /// List of language identifiers this backend supports.
    fn supported_languages(&self) -> Vec<&str>;
}
