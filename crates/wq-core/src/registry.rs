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
        let _ = (project_name, child_db_path);
        todo!("wq-3.2 GREEN")
    }

    pub fn list_registered_children(&self) -> Result<Vec<RegistryEntry>> {
        todo!("wq-3.2 GREEN")
    }
}
