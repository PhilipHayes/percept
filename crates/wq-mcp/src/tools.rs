//! The 7 wq MCP tools (ADR-038 §CLI/MCP surface).

use std::path::PathBuf;

use serde_json::{json, Value};

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
    json!({
        "tools": [
            {
                "name": "wq_ticket_create",
                "description": "Create a work-graph node (ticket/epic/story/decision/note). Embeds title+body for semantic search.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "description": "node type (default: ticket)"},
                        "title": {"type": "string"},
                        "body": {"type": "string"},
                        "status": {"type": "string", "description": "e.g. open | in_progress | blocked | done"},
                        "metadata": {"type": "object", "description": "free-form JSON stored with the node"},
                        "global": {"type": "boolean", "description": "write to the global DB instead of the project DB"}
                    },
                    "required": ["title"]
                }
            },
            {
                "name": "wq_ticket_update",
                "description": "Update a node's title/body/status/metadata. Does not re-embed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "title": {"type": "string"},
                        "body": {"type": "string"},
                        "status": {"type": "string"},
                        "metadata": {"type": "object"},
                        "global": {"type": "boolean"}
                    },
                    "required": ["id"]
                }
            },
            {
                "name": "wq_query",
                "description": "Raw SQL escape hatch against the project DB. Full read/write access — prefer the typed tools for mutations.",
                "inputSchema": {
                    "type": "object",
                    "properties": {"sql": {"type": "string"}},
                    "required": ["sql"]
                }
            },
            {
                "name": "wq_search",
                "description": "Semantic search over node titles+bodies. Returns [{distance, node}] best-first.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"},
                        "k": {"type": "integer", "description": "max results (default 10)"}
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "wq_edge_create",
                "description": "Create a typed, weighted edge between two nodes (blocks/depends_on/part_of/relates_to/supersedes/discussed_in).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "from_id": {"type": "string"},
                        "to_id": {"type": "string"},
                        "kind": {"type": "string"},
                        "weight": {"type": "number"},
                        "global": {"type": "boolean"}
                    },
                    "required": ["from_id", "to_id", "kind"]
                }
            },
            {
                "name": "wq_traverse",
                "description": "Follow edges of one kind outward from a node. Returns [{depth, node}].",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "kind": {"type": "string"},
                        "depth": {"type": "integer", "description": "max hops (default 3)"}
                    },
                    "required": ["id", "kind"]
                }
            },
            {
                "name": "wq_rollup",
                "description": "Roll up nodes across this DB's registry tree (or the global registry with global=true). Returns {nodes, warnings}.",
                "inputSchema": {
                    "type": "object",
                    "properties": {"global": {"type": "boolean"}}
                }
            }
        ]
    })
}

fn open_db(state: &ServerState, global: bool) -> Result<wq_core::WqDb, String> {
    let path = wq_core::resolve_write_target(&state.project_root, global);
    wq_core::WqDb::open(&path).map_err(|e| format!("failed to open {}: {e}", path.display()))
}

impl ServerState {
    /// The one engine per server lifetime, constructed on first embed use.
    fn engine(&mut self) -> Result<&mut EmbedEngine, String> {
        if self.engine.is_none() {
            let engine = EmbedEngine::new(wq_core::ModelKind::BgeSmall)
                .map_err(|e| format!("embedding engine init failed: {e:#}"))?;
            self.engine = Some(engine);
        }
        Ok(self.engine.as_mut().expect("just initialized"))
    }
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn require_str(args: &Value, key: &str, tool: &str) -> Result<String, String> {
    arg_str(args, key).ok_or_else(|| format!("{tool}: missing required string param '{key}'"))
}

/// Dispatches one tool call. Returns a bare JSON value on success or a
/// human-readable error string (the serve loop wraps either in MCP's
/// content envelope; errors get isError=true, mirroring canopy).
pub fn handle_tool_call(
    state: &mut ServerState,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    match tool {
        "wq_ticket_create" => {
            let new = wq_core::NewNode {
                node_type: arg_str(args, "type").unwrap_or_else(|| "ticket".into()),
                title: require_str(args, "title", tool)?,
                body: arg_str(args, "body"),
                status: arg_str(args, "status"),
                harness_origin: Some(state.harness_origin.clone()),
                metadata: args.get("metadata").filter(|m| !m.is_null()).cloned(),
            };
            let db = open_db(state, arg_bool(args, "global"))?;
            let node = db
                .create_node(state.engine()?, new)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(node).map_err(|e| e.to_string())
        }
        "wq_ticket_update" => {
            let id = require_str(args, "id", tool)?;
            let db = open_db(state, arg_bool(args, "global"))?;
            let node = db
                .update_node(
                    &id,
                    wq_core::UpdateNode {
                        title: arg_str(args, "title"),
                        body: arg_str(args, "body"),
                        status: arg_str(args, "status"),
                        metadata: args.get("metadata").filter(|m| !m.is_null()).cloned(),
                    },
                )
                .map_err(|e| e.to_string())?;
            serde_json::to_value(node).map_err(|e| e.to_string())
        }
        "wq_query" => {
            let sql = require_str(args, "sql", tool)?;
            let db = open_db(state, false)?;
            let rows = db.query_json(&sql).map_err(|e| e.to_string())?;
            Ok(Value::Array(rows))
        }
        "wq_search" => {
            let text = require_str(args, "text", tool)?;
            let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let db = open_db(state, false)?;
            let results = db
                .search(state.engine()?, &text, k)
                .map_err(|e| e.to_string())?;
            let rows: Vec<Value> = results
                .into_iter()
                .map(|(node, distance)| json!({"distance": distance, "node": node}))
                .collect();
            Ok(Value::Array(rows))
        }
        "wq_edge_create" => {
            let db = open_db(state, arg_bool(args, "global"))?;
            let edge = db
                .create_edge(wq_core::NewEdge {
                    from_id: require_str(args, "from_id", tool)?,
                    to_id: require_str(args, "to_id", tool)?,
                    kind: require_str(args, "kind", tool)?,
                    weight: args.get("weight").and_then(|v| v.as_f64()),
                })
                .map_err(|e| e.to_string())?;
            serde_json::to_value(edge).map_err(|e| e.to_string())
        }
        "wq_traverse" => {
            let id = require_str(args, "id", tool)?;
            let kind = require_str(args, "kind", tool)?;
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
            let db = open_db(state, false)?;
            let hops = db.traverse(&id, &kind, depth).map_err(|e| e.to_string())?;
            let rows: Vec<Value> = hops
                .into_iter()
                .map(|(node, depth)| json!({"depth": depth, "node": node}))
                .collect();
            Ok(Value::Array(rows))
        }
        "wq_rollup" => {
            let db = open_db(state, arg_bool(args, "global"))?;
            let result = db.rollup().map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown tool: {other}")),
    }
}
