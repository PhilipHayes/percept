//! Semantic search over node embeddings (wq-2).
//!
//! Query text is embedded via the caller-owned [`EmbedEngine`], then matched
//! against the `node_embeddings` vec0 virtual table and joined back to full
//! `Node` records.

use mq_embed::engine::EmbedEngine;

use crate::db::WqDb;
use crate::error::Result;
use crate::node::Node;

impl WqDb {
    /// Returns up to `k` nodes ranked by semantic similarity to `query`,
    /// best match first, each paired with its vec0 distance (lower = closer).
    pub fn search(
        &self,
        engine: &mut EmbedEngine,
        query: &str,
        k: usize,
    ) -> Result<Vec<(Node, f32)>> {
        let _ = (engine, query, k);
        todo!("wq-2.2 GREEN")
    }
}
