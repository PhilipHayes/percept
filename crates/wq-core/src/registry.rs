//! Registry CRUD (wq-3): pointer rows to child DBs — never copies.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub project_name: String,
    pub db_path: String,
    pub parent_registry_id: Option<String>,
    pub last_seen: Option<String>,
}

impl WqDb {
    /// Registers `child_db_path` under `project_name` in this DB's registry.
    /// Stores a pointer only — the child's rows are never copied.
    pub fn register_child(&self, project_name: &str, child_db_path: &Path) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        self.conn.execute(
            "INSERT OR REPLACE INTO registry (project_name, db_path, parent_registry_id, last_seen)
             VALUES (?1, ?2, NULL, ?3)",
            rusqlite::params![project_name, child_db_path.to_string_lossy(), now],
        )?;
        Ok(())
    }

    pub fn list_registered_children(&self) -> Result<Vec<RegistryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT project_name, db_path, parent_registry_id, last_seen
             FROM registry ORDER BY project_name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RegistryEntry {
                project_name: row.get(0)?,
                db_path: row.get(1)?,
                parent_registry_id: row.get(2)?,
                last_seen: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(crate::error::Error::from)
    }
}
