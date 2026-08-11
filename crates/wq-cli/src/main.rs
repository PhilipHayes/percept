//! wq — CLI over wq-core (ADR-038 §CLI/MCP surface).
//!
//! Success: one JSON value on stdout, exit 0.
//! Failure: {"error": "..."} on stderr, exit 1 (contract pinned by
//! wq-4.1's ticket_update_unknown_id test).
//!
//! anyhow is acceptable at this binary boundary (wq-4 plan, scope note);
//! wq-core itself stays typed-error.

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use wq_core::{EmbedEngine, ModelKind, NewEdge, NewNode, UpdateNode, WqDb};

#[derive(Parser)]
#[command(
    name = "wq",
    about = "Work-graph query — tickets, edges, semantic search (ADR-038)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create or update ticket/epic/decision/note nodes
    Ticket {
        #[command(subcommand)]
        action: TicketAction,
    },
    /// Raw SQL escape hatch (full read/write access — no guardrails)
    Query { sql: String },
    /// Semantic search over node titles+bodies
    Search {
        text: String,
        #[arg(long, default_value_t = 10)]
        k: usize,
    },
    /// Create typed, weighted edges between nodes
    Edge {
        #[command(subcommand)]
        action: EdgeAction,
    },
    /// Follow edges of one kind outward from a node
    Traverse {
        id: String,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value_t = 3)]
        depth: u32,
    },
    /// Roll up nodes across this DB's registry (or the global registry)
    Rollup {
        #[arg(long)]
        global: bool,
    },
    /// Check the DB for silent integrity drift (never loads a model)
    Doctor {
        #[arg(long)]
        global: bool,
    },
    /// Backfill embeddings for nodes that have none, making them searchable
    Reindex {
        /// Report what would be indexed without loading a model or writing
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        global: bool,
    },
    /// Register a child DB as a federation pointer (Amendment 2026-07-03, closes FU-wq-4-2)
    Register {
        project_name: String,
        db_path: String,
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum TicketAction {
    Create {
        #[arg(long = "type", default_value = "ticket")]
        node_type: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        status: Option<String>,
        /// Which agent harness is writing (defaults to "cli" — a human at
        /// a terminal is a legitimate origin, per Q-wq-4-1)
        #[arg(long, default_value = "cli")]
        harness: String,
        #[arg(long)]
        global: bool,
    },
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum EdgeAction {
    Create {
        from_id: String,
        to_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        weight: Option<f64>,
        #[arg(long)]
        global: bool,
    },
}

fn open_db(global: bool) -> Result<WqDb> {
    let cwd = std::env::current_dir()?;
    let path: PathBuf = wq_core::resolve_write_target(&cwd, global);
    Ok(WqDb::open(&path)?)
}

/// Constructed only by the handlers that embed (ticket create, search) —
/// loading the ONNX model costs ~1s and must not tax read-only commands.
fn engine() -> Result<EmbedEngine> {
    EmbedEngine::new(ModelKind::BgeSmall).map_err(|e| anyhow::anyhow!("{e:#}"))
}

fn run(cli: Cli) -> Result<Value> {
    match cli.command {
        Command::Ticket { action } => match action {
            TicketAction::Create {
                node_type,
                title,
                body,
                status,
                harness,
                global,
            } => {
                let db = open_db(global)?;
                let node = db.create_node(
                    &mut engine()?,
                    NewNode {
                        node_type,
                        title,
                        body,
                        status,
                        harness_origin: Some(harness),
                        metadata: None,
                    },
                )?;
                Ok(serde_json::to_value(node)?)
            }
            TicketAction::Update {
                id,
                title,
                body,
                status,
                global,
            } => {
                let db = open_db(global)?;
                let node = db.update_node(
                    &id,
                    UpdateNode {
                        title,
                        body,
                        status,
                        metadata: None,
                    },
                )?;
                Ok(serde_json::to_value(node)?)
            }
        },
        Command::Query { sql } => {
            let db = open_db(false)?;
            Ok(Value::Array(db.query_json(&sql)?))
        }
        Command::Search { text, k } => {
            let db = open_db(false)?;
            // Warn on stderr rather than wrapping stdout: a caller piping
            // `wq search` into jq must keep getting a bare array. An unindexed
            // node is invisible to this query, so the result is short by an
            // unknown amount and nothing else would ever say so.
            let unindexed = db.unindexed_count()?;
            if unindexed > 0 {
                eprintln!(
                    "{}",
                    json!({
                        "warning": format!(
                            "{unindexed} node(s) have no embedding and cannot be \
                             returned by search; run `wq reindex`"
                        )
                    })
                );
            }
            let results = db.search(&mut engine()?, &text, k)?;
            let rows: Vec<Value> = results
                .into_iter()
                .map(|(node, distance)| json!({ "distance": distance, "node": node }))
                .collect();
            Ok(Value::Array(rows))
        }
        Command::Edge { action } => match action {
            EdgeAction::Create {
                from_id,
                to_id,
                kind,
                weight,
                global,
            } => {
                let db = open_db(global)?;
                let edge = db.create_edge(NewEdge {
                    from_id,
                    to_id,
                    kind,
                    weight,
                })?;
                Ok(serde_json::to_value(edge)?)
            }
        },
        Command::Traverse { id, kind, depth } => {
            let db = open_db(false)?;
            let hops = db.traverse(&id, &kind, depth)?;
            let rows: Vec<Value> = hops
                .into_iter()
                .map(|(node, depth)| json!({ "depth": depth, "node": node }))
                .collect();
            Ok(Value::Array(rows))
        }
        Command::Rollup { global } => {
            let db = open_db(global)?;
            Ok(serde_json::to_value(db.rollup()?)?)
        }
        Command::Doctor { global } => {
            let db = open_db(global)?;
            Ok(serde_json::to_value(db.doctor()?)?)
        }
        Command::Reindex { dry_run, global } => {
            let db = open_db(global)?;
            // The dry run deliberately never calls engine(): asking whether the
            // graph is fully searchable must not require loading an ONNX model.
            let report = if dry_run {
                db.reindex_dry_run()?
            } else {
                db.reindex(&mut engine()?)?
            };
            Ok(serde_json::to_value(report)?)
        }
        Command::Register {
            project_name,
            db_path,
            global,
        } => {
            let db = open_db(global)?;
            db.register_child(&project_name, Path::new(&db_path))?;
            let entry = db
                .list_registered_children()?
                .into_iter()
                .find(|e| e.project_name == project_name)
                .expect("just-registered entry must be present");
            Ok(serde_json::to_value(entry)?)
        }
    }
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(value) => {
            println!("{value}");
        }
        Err(e) => {
            eprintln!("{}", json!({ "error": format!("{e:#}") }));
            std::process::exit(1);
        }
    }
}
