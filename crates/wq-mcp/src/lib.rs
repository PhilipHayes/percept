//! wq-mcp — MCP stdio server over wq-core (ADR-038, phase wq-5).
//!
//! Hand-rolled JSON-RPC 2.0 over stdin/stdout, mirroring canopy's
//! mcp_server.rs (no MCP SDK crate exists in this ecosystem — Q-wq-5-2).
//! Tool results are wrapped in MCP's content envelope by the serve loop;
//! `handle_tool_call` itself returns bare JSON values (testable directly).

pub mod server;
pub mod tools;

pub use tools::{handle_tool_call, tool_definitions, ServerState};
