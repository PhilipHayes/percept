//! The 7 wq MCP tools (ADR-038 §CLI/MCP surface) — RED stubs.

use std::path::PathBuf;

use serde_json::Value;

use wq_core::EmbedEngine;

/// Per-process server state.
///
/// - `harness_origin`: read ONCE from WQ_HARNESS_ORIGIN at construction
///   (per-process attribution, Q-wq-5-1 resolved); "unknown" if unset.
/// - `engine`: ONE lazily-initialized embed engine for the server's whole
///   lifetime — constructed on the first embed-requiring call, never for
///   query/traverse/rollup/edge/update calls (wq-5 plan drift note).
/// - `project_root`: where project-DB resolution starts (cwd in
///   production; a TempDir in tests).
pub struct ServerState {
    pub project_root: PathBuf,
    pub harness_origin: String,
    pub(crate) engine: Option<EmbedEngine>,
}

impl ServerState {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            harness_origin: std::env::var("WQ_HARNESS_ORIGIN")
                .unwrap_or_else(|_| "unknown".to_string()),
            engine: None,
        }
    }

    /// Test/embedding-time constructor that bypasses env reads.
    pub fn with_harness(project_root: PathBuf, harness_origin: &str) -> Self {
        Self {
            project_root,
            harness_origin: harness_origin.to_string(),
            engine: None,
        }
    }
}

/// MCP tools/list payload: the 7 wq tools with JSON-schema inputs.
pub fn tool_definitions() -> Value {
    todo!("wq-5.2 GREEN")
}

/// Dispatches one tool call. Returns a bare JSON value on success or a
/// human-readable error string (the serve loop wraps either in MCP's
/// content envelope; errors get isError=true, mirroring canopy).
pub fn handle_tool_call(
    state: &mut ServerState,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    let _ = (state, tool, args);
    todo!("wq-5.2 GREEN")
}
