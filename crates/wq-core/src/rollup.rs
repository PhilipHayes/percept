//! Multi-DB rollup (wq-3): live queries across a registry tree.
//!
//! Children are attached ONE AT A TIME (ATTACH → query → DETACH), with
//! results merged in Rust — the bundled sqlite's SQLITE_MAX_ATTACHED is 10
//! (vec0-poc spike, experiment 3), so attach-all-then-UNION would hard-fail
//! at 11+ registered projects. Nested registries are followed recursively
//! with a visited-set guard against cycles.

use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::Result;
use crate::node::Node;

/// A non-fatal problem encountered during rollup (e.g. a registered child
/// whose db_path no longer exists). Rollup never fails wholesale because
/// one of N children went stale — see D-wq-3-2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollupWarning {
    pub db_path: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct RollupResult {
    /// Nodes from this DB and every (transitively) registered child.
    pub nodes: Vec<Node>,
    pub warnings: Vec<RollupWarning>,
}

impl WqDb {
    /// Rolls up nodes from this DB plus all registered children, live at
    /// query time. Registry rows pointing at other registry DBs are
    /// followed recursively; cycles terminate via a visited-path set.
    pub fn rollup(&self) -> Result<RollupResult> {
        todo!("wq-3.2 GREEN")
    }
}
