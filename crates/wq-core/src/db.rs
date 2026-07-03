use std::path::Path;

use crate::error::Result;

const SCHEMA_SQL: &str = include_str!("schema.sql");

pub struct WqDb {
    pub(crate) conn: rusqlite::Connection,
}

impl WqDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = rusqlite::Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(mut conn: rusqlite::Connection) -> Result<Self> {
        // Enable FK enforcement: simplest mechanism to reject orphan edges
        // (wq-1.3's edge CRUD will rely on this rather than app-layer checks).
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        tx.commit()?;
        Ok(Self { conn })
    }

    /// Read-level access to the underlying connection.
    ///
    /// Exists for ADR-038's raw-SQL escape hatch (`wq query "SELECT ..."`,
    /// phase wq-4) and for tests that assert on storage-level invariants.
    /// Mutations should go through the typed CRUD methods, which own
    /// invariants (timestamps, embedding rows) raw SQL would bypass.
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }
}
