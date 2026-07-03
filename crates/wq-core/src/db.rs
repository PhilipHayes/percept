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
    /// The file this DB was opened from; None for in-memory DBs. Used by
    /// rollup()'s cycle guard to seed the visited set with "self".
    pub(crate) path: Option<std::path::PathBuf>,
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
        Self::init(conn, Some(path.to_path_buf()))
    }

    pub fn open_in_memory() -> Result<Self> {
        register_vec0();
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::init(conn, None)
    }

    fn init(mut conn: rusqlite::Connection, path: Option<std::path::PathBuf>) -> Result<Self> {
        // Enable FK enforcement: simplest mechanism to reject orphan edges
        // (wq-1.3's edge CRUD will rely on this rather than app-layer checks).
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        tx.commit()?;
        Ok(Self { conn, path })
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

    /// Executes arbitrary SQL, returning rows as JSON objects keyed by
    /// column name — the shared backend for the `wq query` escape hatch
    /// (CLI, phase wq-4) and the `wq_query` MCP tool (phase wq-5).
    /// Blobs are summarized, not dumped (embeddings are 1.5KB each).
    pub fn query_json(&self, sql: &str) -> Result<Vec<serde_json::Value>> {
        use serde_json::{json, Value};

        let mut stmt = self.conn.prepare(sql)?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut obj = serde_json::Map::new();
            for (i, name) in column_names.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f) => json!(f),
                    rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                    rusqlite::types::ValueRef::Blob(b) => {
                        json!(format!("<blob {} bytes>", b.len()))
                    }
                };
                obj.insert(name.clone(), value);
            }
            out.push(Value::Object(obj));
        }
        Ok(out)
    }
}
