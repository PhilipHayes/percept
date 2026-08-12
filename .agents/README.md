# `.agents/` — project work graph

`wq.db` is percept's own backlog, in the schema `wq` defines
([ADR-038](https://github.com/PhilipHayes/gestalt/blob/main/philip-hayes/adr/038-wq-work-graph-query.md))
— `nodes` + `edges`, at the per-project path that ADR specifies. percept
eating its own output: the tool that queries work graphs keeps its work in one.

```bash
wq query "SELECT title FROM nodes WHERE type = 'ticket' AND status = 'open'"
wq traverse <id> --kind part_of --depth 3
wq doctor                     # schema + index drift
wq reindex                    # backfill embeddings for unindexed nodes
```

It is an ordinary SQLite file, so anything that speaks SQLite works too:

```bash
sqlite3 .agents/wq.db "SELECT type, status, title FROM nodes"
```

`wq-seed.sql` is the generated text form of the same content — committed so the
board is diffable in review, since a binary `.db` is not. Once you start editing
tickets through `wq`, the DB is the source of truth and the seed is a historical
snapshot; regenerate it rather than letting the two disagree:

```bash
for t in nodes edges registry; do sqlite3 .agents/wq.db ".mode insert $t" "SELECT * FROM $t;"; done
```

The seed omits `node_embeddings` (the `vec0` virtual table): it needs the
sqlite-vec extension loaded, and its contents are derived, not authored. Every
statement in wq's schema is `CREATE ... IF NOT EXISTS`, so wq adds the table on
first open and `wq reindex` fills it.

## What's on it

Five nodes, seeded 2026-08-11 from work that surfaced while building `wq
reindex` and `wq doctor` — deferred deliberately rather than scope-creeping
those changes:

| Ticket | Why it's here |
| --- | --- |
| CI does not check the wq crates at all | The workflow excludes `wq-core`, `wq-cli`, `wq-mcp` from build, test *and* clippy, so nothing in this crate family was ever verified by CI. Carries the proven local workaround for the `ort` build (pypi `onnxruntime` wheel + `ORT_LIB_LOCATION` + `ORT_PREFER_DYNAMIC_LINK=1`). |
| Pin the Rust toolchain | Local 1.94.1 vs CI stable 1.97.x meant `unnecessary_sort_by` didn't exist locally — the clippy breakage was invisible until CI ran. A `rust-toolchain.toml` closes the gap. |
| wq doctor detects drift but nothing prevents it | Residual risk named in ADR-038 Amendment (2): `doctor` reports, `reindex` repairs, and nothing stops the drift happening again. |
| aq-cli declares main.rs in two build targets | Cargo warns that `main.rs` is claimed by both the `aq` and `nq` bin targets. |
