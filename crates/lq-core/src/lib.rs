pub mod drain;
pub mod filter;
pub mod pipeline;

// Re-export core types from lq-parse for convenience.
pub use lq_parse::{LogEntry, Level};
pub use drain::{Drain, Pattern};
pub use filter::{Filter, apply_filters, parse_query};
pub use pipeline::{Pipeline, Stage, parse_pipeline, execute_pipeline};
