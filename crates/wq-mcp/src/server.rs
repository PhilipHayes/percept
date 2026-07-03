//! JSON-RPC 2.0 stdio serve loop, mirroring canopy's mcp_server.rs:
//! newline-delimited requests on stdin, responses on stdout; tool results
//! wrapped in MCP's content envelope, tool failures as isError=true
//! results (never JSON-RPC protocol errors — those are reserved for
//! malformed requests / unknown methods).

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::tools::{handle_tool_call, tool_definitions, ServerState};

/// Runs the blocking stdio server loop until stdin closes.
pub fn serve() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let cwd = std::env::current_dir()?;
    let mut state = ServerState::new(cwd);

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_response(
                    &mut stdout,
                    json!({
                        "jsonrpc": "2.0", "id": null,
                        "error": {"code": -32700, "message": format!("Parse error: {e}")}
                    }),
                )?;
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {
                        "name": "wq",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            "notifications/initialized" => continue,
            "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": tool_definitions()}),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
                let tool = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match handle_tool_call(&mut state, tool, &args) {
                    Ok(value) => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"content": [{"type": "text", "text": value.to_string()}]}
                    }),
                    Err(e) => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {
                            "content": [{"type": "text", "text": json!({"error": e}).to_string()}],
                            "isError": true
                        }
                    }),
                }
            }
            other => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {other}")}
            }),
        };
        write_response(&mut stdout, response)?;
    }
    Ok(())
}

fn write_response(stdout: &mut io::Stdout, response: Value) -> io::Result<()> {
    writeln!(stdout, "{response}")?;
    stdout.flush()
}
