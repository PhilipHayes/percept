use serde::{Deserialize, Serialize};

/// A single embedding vector with its associated key and optional metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedItem {
    /// Unique key for this item (e.g. "src/auth.rs::verify_credentials")
    pub key: String,
    /// The embedding vector
    pub embedding: Vec<f32>,
    /// The original text that was embedded
    pub text: String,
    /// Optional JSON metadata carried alongside the embedding
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A search result with similarity score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub key: String,
    pub score: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A match result pairing items from two sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub left: String,
    pub right: String,
    pub score: f32,
}

/// Supported embedding models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelKind {
    /// BGE-small-en-v1.5 — 33MB, 384 dims, ~5ms/embed (default)
    BgeSmall,
    /// Nomic-embed-code — 137MB, 768 dims, code-specialized (NOT YET AVAILABLE via fastembed)
    NomicCode,
}

impl ModelKind {
    pub fn dims(self) -> usize {
        match self {
            ModelKind::BgeSmall => 384,
            ModelKind::NomicCode => 768,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ModelKind::BgeSmall => "bge-small-en-v1.5",
            ModelKind::NomicCode => "nomic-embed-code",
        }
    }
}

impl Default for ModelKind {
    fn default() -> Self {
        ModelKind::BgeSmall
    }
}

/// Collection metadata stored alongside the vector index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMeta {
    pub name: String,
    pub model: ModelKind,
    pub dims: usize,
    pub item_count: usize,
}
