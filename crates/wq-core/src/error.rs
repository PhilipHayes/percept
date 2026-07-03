use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("edge not found: {0}")]
    EdgeNotFound(String),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("embedding failed: {0}")]
    Embed(String),
}

pub type Result<T> = std::result::Result<T, Error>;
