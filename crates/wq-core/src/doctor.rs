//! Integrity checks over a wq DB.
//!
//! wq owns its substrate but does not have it to itself: `wq query` is a
//! documented raw-SQL escape hatch, and a project DB is an ordinary SQLite file
//! that seed scripts, migrations and other tools can write. Every invariant the
//! write path maintains is therefore an invariant something else can break.
//!
//! [`WqDb::doctor`] reports the three that are actually reachable. Each is
//! silent — none produces an error, all three degrade a query's answer rather
//! than failing it — which is exactly why they need a command that goes looking.
//!
//! Deliberately no `--fix`. Two of these have no safe automatic repair (see
//! [`DoctorReport`]), and a checker you trust is worth more than a repairer you
//! have to audit. [`crate::WqDb::reindex`] fixes the one that has an obvious
//! answer.

use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::Result;

/// A single failed check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorFinding {
    /// Stable machine-readable key.
    pub check: String,
    /// How many rows are affected.
    pub count: usize,
    /// What it means and what breaks because of it.
    pub detail: String,
    /// What to do about it.
    pub remedy: String,
}

/// Result of a [`WqDb::doctor`] run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub total_embeddings: usize,
    pub findings: Vec<DoctorFinding>,
}

impl DoctorReport {
    pub fn healthy(&self) -> bool {
        self.findings.is_empty()
    }
}

impl WqDb {
    /// Runs every integrity check and reports what is wrong.
    ///
    /// Pure SQL — no `EmbedEngine`, no model load, no network. Same rule as
    /// [`reindex_dry_run`](Self::reindex_dry_run) (D-wq-8-1): a health check
    /// that cannot run in a constrained environment cannot tell you the one
    /// thing you need to know there.
    pub fn doctor(&self) -> Result<DoctorReport> {
        let total_nodes = self.count("SELECT COUNT(*) FROM nodes")?;
        let total_edges = self.count("SELECT COUNT(*) FROM edges")?;
        let total_embeddings = self.count("SELECT COUNT(*) FROM node_embeddings")?;

        let mut findings = Vec::new();

        // 1. Nodes with no embedding. Reachable through any write that did not
        //    go via create_node: a seed, a restored dump, a raw INSERT.
        let unindexed = self.unindexed_count()?;
        if unindexed > 0 {
            findings.push(DoctorFinding {
                check: "unindexed_nodes".into(),
                count: unindexed,
                detail: "nodes with no embedding — `search` cannot return them, \
                         and reports no error when it silently skips them"
                    .into(),
                remedy: "wq reindex".into(),
            });
        }

        // 2. Embeddings whose node is gone. `node_embeddings` is a vec0 virtual
        //    table, so it carries no foreign key and nothing cascades: deleting
        //    a node through `wq query` leaves its vector behind. A stale vector
        //    can still match a KNN query, whose JOIN back to `nodes` then drops
        //    the row — so the effect is a search that returns fewer than `k`
        //    results with no indication why.
        let orphans = self.count(
            "SELECT COUNT(*) FROM node_embeddings
             WHERE node_id NOT IN (SELECT id FROM nodes)",
        )?;
        if orphans > 0 {
            findings.push(DoctorFinding {
                check: "orphan_embeddings".into(),
                count: orphans,
                detail: "embeddings whose node no longer exists — they consume \
                         KNN slots and are then dropped by the join, so `search` \
                         quietly returns fewer than k results"
                    .into(),
                remedy: "DELETE FROM node_embeddings WHERE node_id NOT IN \
                         (SELECT id FROM nodes)"
                    .into(),
            });
        }

        // 3. Edges pointing at missing nodes. wq's own connections set
        //    `PRAGMA foreign_keys = ON`, so wq cannot create these — but the FK
        //    is per-connection, and a DB written by any other tool (a seed
        //    script, a migration) can. Traversals silently stop early at one.
        let dangling = self.count(
            "SELECT COUNT(*) FROM edges
             WHERE from_id NOT IN (SELECT id FROM nodes)
                OR to_id NOT IN (SELECT id FROM nodes)",
        )?;
        if dangling > 0 {
            findings.push(DoctorFinding {
                check: "dangling_edges".into(),
                count: dangling,
                detail: "edges referencing a node that does not exist — \
                         `traverse` stops early at one and reports a shorter \
                         path rather than an error. wq's own writes cannot \
                         create these (foreign_keys is ON); another tool's can"
                    .into(),
                remedy: "repair the referenced nodes, or delete the edges — no \
                         automatic fix, since which side is wrong is not \
                         knowable from here"
                    .into(),
            });
        }

        Ok(DoctorReport {
            total_nodes,
            total_edges,
            total_embeddings,
            findings,
        })
    }

    fn count(&self, sql: &str) -> Result<usize> {
        let n: i64 = self.conn.query_row(sql, [], |row| row.get(0))?;
        Ok(n as usize)
    }
}
