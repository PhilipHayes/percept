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
}
