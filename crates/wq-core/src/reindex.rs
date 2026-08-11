//! Backfill embeddings for nodes that have none.
//!
//! [`create_node`](crate::WqDb::create_node) writes a node and its embedding in
//! one transaction, so nodes created *through wq* always have one (D-wq-2-2).
//! Nodes that arrive any other way do not: a seed script, a restored dump, a
//! migration from another tracker, or a hand-written `INSERT` via
//! `wq query`. Those nodes are fully present in `nodes` and completely absent
//! from `search` — the DB looks correct and its semantic index silently is not.
//!
//! Silent is the operative word: nothing in wq reports the discrepancy, so the
//! failure mode is a search that quietly returns fewer results than it should,
//! forever. [`WqDb::unindexed_nodes`] makes it observable and
//! [`WqDb::reindex`] closes it.

use mq_embed::engine::EmbedEngine;
use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::Result;
use crate::node::Node;

/// Outcome of a [`WqDb::reindex`] run, or of a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexReport {
    /// Every node in the DB, indexed or not.
    pub total_nodes: usize,
    /// Nodes without an embedding when the run started.
    pub missing_before: usize,
    /// Nodes embedded by this run. Zero for a dry run.
    pub indexed: usize,
    /// True when nothing was written.
    pub dry_run: bool,
}

impl ReindexReport {
    /// True when every node has an embedding, i.e. `search` can see the whole
    /// graph.
    pub fn fully_indexed(&self) -> bool {
        self.missing_before == self.indexed
    }
}

impl WqDb {
    /// Nodes with no row in `node_embeddings`, oldest first.
    ///
    /// Deliberately a plain anti-join rather than anything vec0-specific: a
    /// `vec0` table supports an ordinary scan of its primary key, which is all
    /// this needs, and keeping it ordinary means it does not break when the
    /// extension's KNN surface changes.
    pub fn unindexed_nodes(&self) -> Result<Vec<Node>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, title, body, status, harness_origin,
                    created_at, updated_at, metadata_json
             FROM nodes
             WHERE id NOT IN (SELECT node_id FROM node_embeddings)
             ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], crate::node::row_to_node)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(crate::error::Error::from)
    }

    /// Count of nodes missing an embedding, without materializing them.
    pub fn unindexed_count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM nodes
             WHERE id NOT IN (SELECT node_id FROM node_embeddings)",
            [],
            |row| row.get(0),
        )?;
        Ok(n as usize)
    }

    /// Reports what [`reindex`](Self::reindex) would do, without an
    /// `EmbedEngine` and without writing anything.
    ///
    /// Separate from `reindex` on purpose: answering "is my graph fully
    /// searchable?" should not require loading an ONNX model, which is the
    /// expensive and network-dependent part. This is the call that works in a
    /// constrained environment.
    pub fn reindex_dry_run(&self) -> Result<ReindexReport> {
        let total_nodes: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
        Ok(ReindexReport {
            total_nodes: total_nodes as usize,
            missing_before: self.unindexed_count()?,
            indexed: 0,
            dry_run: true,
        })
    }

    /// Embeds every node missing an embedding.
    ///
    /// Each node is committed in its own transaction rather than the whole run
    /// in one. Embedding is the slow part and a large backfill is exactly where
    /// an interruption is likely; per-node commits mean a killed run leaves the
    /// nodes it finished indexed and the rest still findable by a later run,
    /// instead of rolling back an hour of inference.
    ///
    /// `engine` is caller-owned for the same reason as
    /// [`create_node`](Self::create_node) — wq-core never constructs one.
    pub fn reindex(&self, engine: &mut EmbedEngine) -> Result<ReindexReport> {
        let pending = self.unindexed_nodes()?;
        let total_nodes: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;

        let missing_before = pending.len();
        let mut indexed = 0usize;

        for node in pending {
            let embedding = crate::embed::embed_to_bytes(
                engine,
                &crate::embed::node_embed_text(&node.title, node.body.as_deref()),
            )?;
            self.conn.execute(
                "INSERT INTO node_embeddings (node_id, embedding) VALUES (?1, ?2)",
                rusqlite::params![node.id, embedding],
            )?;
            indexed += 1;
        }

        Ok(ReindexReport {
            total_nodes: total_nodes as usize,
            missing_before,
            indexed,
            dry_run: false,
        })
    }
}
