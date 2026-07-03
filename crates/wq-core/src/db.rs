use std::path::Path;

use crate::error::Result;

pub struct WqDb {
    #[allow(dead_code)]
    conn: rusqlite::Connection,
}

impl WqDb {
    pub fn open(_path: &Path) -> Result<Self> {
        todo!()
    }

    pub fn open_in_memory() -> Result<Self> {
        todo!()
    }
}
