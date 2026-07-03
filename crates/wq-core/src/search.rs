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
    ///
    /// KNN queries against vec0 require the bound as an explicit `k = ?`
    /// predicate on the virtual table — a downstream LIMIT is NOT sufficient
    /// once a JOIN is involved (vec0-poc spike, experiment 1b).
    pub fn search(
        &self,
        engine: &mut EmbedEngine,
        query: &str,
        k: usize,
    ) -> Result<Vec<(Node, f32)>> {
        let query_bytes = crate::embed::embed_to_bytes(engine, query)?;

        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.type, n.title, n.body, n.status, n.harness_origin,
                    n.created_at, n.updated_at, n.metadata_json, e.distance
             FROM node_embeddings e
             JOIN nodes n ON n.id = e.node_id
             WHERE e.embedding MATCH ?1 AND k = ?2
             ORDER BY e.distance",
        )?;
        let rows = stmt.query_map(rusqlite::params![query_bytes, k as i64], |row| {
            let node = crate::node::row_to_node(row)?;
            let distance: f64 = row.get(9)?;
            Ok((node, distance as f32))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(crate::error::Error::from)
    }
}
