use std::path::Path;
use std::sync::Once;

use crate::error::Result;

const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Registers sqlite-vec's vec0 module for every subsequently-opened
/// connection in this process. Must run before schema init (schema.sql
/// contains a CREATE VIRTUAL TABLE ... USING vec0 statement). Idempotent
/// via Once; the call shape is the one confirmed in the vec0-poc spike
/// (reviews/manual/wq-spikes/vec0-poc/FINDINGS.md).
fn register_vec0() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        // Inferred transmute target: sqlite3_auto_extension's callback type
        // (unsafe extern "C" fn(*mut sqlite3, *mut *mut c_char,
        // *const sqlite3_api_routines) -> c_int). Spelling it out would
        // couple this to rusqlite's ffi type aliases for zero safety gain —
        // the cast itself is the pattern sqlite-vec's own docs/tests use.
        #[allow(clippy::missing_transmute_annotations)]
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

pub struct WqDb {
    pub(crate) conn: rusqlite::Connection,
}

impl WqDb {
    pub fn open(path: &Path) -> Result<Self> {
        register_vec0();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = rusqlite::Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        register_vec0();
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
