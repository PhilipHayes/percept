pub mod db;
pub mod doctor;
pub mod edge;
mod embed;
pub mod error;
pub mod node;
pub mod path;
pub mod registry;
pub mod reindex;
pub mod rollup;
pub mod search;
pub mod traverse;

// Re-exported so wq-cli / wq-mcp / tests construct the engine through one
// dependency (wq-core) instead of depending on mq-embed directly.
pub use mq_embed::engine::EmbedEngine;
pub use mq_embed::model::ModelKind;

pub use db::WqDb;
pub use doctor::{DoctorFinding, DoctorReport};
pub use edge::{Edge, NewEdge};
pub use error::{Error, Result};
pub use node::{NewNode, Node, UpdateNode};
pub use path::{resolve_global_db_path, resolve_project_db_path, resolve_write_target};
pub use registry::RegistryEntry;
pub use reindex::ReindexReport;
pub use rollup::{RollupResult, RollupWarning};
