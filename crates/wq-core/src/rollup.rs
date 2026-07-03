//! Multi-DB rollup (wq-3): live queries across a registry tree.
//!
//! Each registered child is opened as its own short-lived connection and
//! queried directly — identical semantics to the ADR's ATTACH framing
//! (live at query time, pointers not copies) with no SQLITE_MAX_ATTACHED
//! ceiling (10 in the bundled build — vec0-poc spike, experiment 3).
//! ATTACH becomes necessary only if a future feature needs ONE SQL
//! statement spanning DBs (e.g. cross-DB semantic search in one query);
//! the spike confirmed that works when the time comes. Nested registries
//! are followed recursively with a canonicalized-path visited set.

use std::collections::HashSet;
use std::path::PathBuf;

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

#[derive(Debug, Default, Serialize)]
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
        let mut result = RollupResult::default();
        let mut visited: HashSet<PathBuf> = HashSet::new();
        if let Some(own) = &self.path {
            if let Ok(canonical) = own.canonicalize() {
                visited.insert(canonical);
            }
        }
        self.rollup_into(&mut result, &mut visited)?;
        Ok(result)
    }

    fn rollup_into(&self, result: &mut RollupResult, visited: &mut HashSet<PathBuf>) -> Result<()> {
        result.nodes.extend(self.list_nodes(None, None)?);

        for entry in self.list_registered_children()? {
            let raw = PathBuf::from(&entry.db_path);
            // canonicalize doubles as the existence check (D-wq-3-2) and
            // normalizes the path for reliable cycle detection.
            let canonical = match raw.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    result.warnings.push(RollupWarning {
                        db_path: entry.db_path,
                        reason: format!("child db not reachable: {e}"),
                    });
                    continue;
                }
            };
            if !visited.insert(canonical.clone()) {
                continue; // already rolled up (cycle or duplicate registration)
            }
            match WqDb::open(&canonical) {
                Ok(child) => child.rollup_into(result, visited)?,
                Err(e) => result.warnings.push(RollupWarning {
                    db_path: entry.db_path,
                    reason: format!("child db failed to open: {e}"),
                }),
            }
        }
        Ok(())
    }
}
