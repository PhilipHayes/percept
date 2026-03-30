pub mod drain;
pub mod filter;
pub mod pipeline;

// Re-export core types from lq-parse for convenience.
pub use drain::{Drain, Pattern};
pub use filter::{apply_filters, parse_query, Filter};
pub use lq_parse::{Level, LogEntry};
pub use pipeline::{execute_pipeline, parse_pipeline, Pipeline, Stage};
