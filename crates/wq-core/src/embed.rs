//! Embedding plumbing shared by the write path (node.rs) and the query
//! path (search.rs): text → BgeSmall vector → little-endian f32 bytes in
//! the exact byte layout vec0 expects (confirmed in the vec0-poc spike).

use mq_embed::engine::EmbedEngine;

use crate::error::{Error, Result};

/// Dimension of the node_embeddings vec0 table (schema.sql). Coupled to
/// mq-embed's BgeSmall default — enforced at runtime by `embed_to_bytes`,
/// since an existing FLOAT[384] table cannot absorb other sizes.
pub(crate) const EMBEDDING_DIMS: usize = 384;

/// The text a node is embedded from: title + blank line + body (if any).
pub(crate) fn node_embed_text(title: &str, body: Option<&str>) -> String {
    match body {
        Some(b) if !b.is_empty() => format!("{title}\n\n{b}"),
        _ => title.to_string(),
    }
}

pub(crate) fn embed_to_bytes(engine: &mut EmbedEngine, text: &str) -> Result<Vec<u8>> {
    let vector = engine
        .embed_one(text)
        .map_err(|e| Error::Embed(format!("{e:#}")))?;
    if vector.len() != EMBEDDING_DIMS {
        return Err(Error::Embed(format!(
            "embedding dimension mismatch: engine produced {}, schema requires {} \
             (node_embeddings is FLOAT[{}]; see schema.sql)",
            vector.len(),
            EMBEDDING_DIMS,
            EMBEDDING_DIMS,
        )));
    }
    Ok(vector.iter().flat_map(|f| f.to_le_bytes()).collect())
}
