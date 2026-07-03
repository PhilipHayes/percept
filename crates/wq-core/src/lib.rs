pub mod db;
pub mod edge;
mod embed;
pub mod error;
pub mod node;
pub mod path;
pub mod search;
pub mod traverse;

// Re-exported so wq-cli / wq-mcp / tests construct the engine through one
// dependency (wq-core) instead of depending on mq-embed directly.
pub use mq_embed::engine::EmbedEngine;
pub use mq_embed::model::ModelKind;

pub use db::WqDb;
pub use edge::{Edge, NewEdge};
pub use error::{Error, Result};
pub use node::{NewNode, Node, UpdateNode};
pub use path::resolve_project_db_path;
