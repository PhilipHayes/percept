# wq spike findings — vec0 schema shape + ATTACH/MATCH interaction

Ran 2026-07-03 against `rustc 1.94.0` / `cargo 1.94.0` (stable, macOS arm64).
Scratch crate: `vec0-poc/` (standalone — deliberately its own `[workspace]`,
NOT a member of percept's real workspace). Run with `cargo run` from that
directory; deletes its own temp dbs on exit.

Resolves the two hard blockers flagged by the wq-2 and wq-3 phase plans
(`R-wq-2-1` / `Q-wq-2-2` and `R-wq-3-1` / `Q-wq-3-1`).

## Version pin that works

- `rusqlite = "0.31"` (features = ["bundled"]) — matches canopy's existing pin.
- `sqlite-vec = "0.1.9"` — its own `dev-dependencies` also pin `rusqlite 0.31`, so this isn't a coincidence; it's the version sqlite-vec's own test suite is built against.
- **`rusqlite@0.40` (latest) does NOT build on this toolchain** — pulls in `libsqlite3-sys 0.38.1`, whose build script uses the unstable `cfg_select` feature (rust-lang/rust#115585), which fails on stable. Do not let `cargo add` float rusqlite to latest in wq-core's real Cargo.toml — pin `0.31` explicitly, same as canopy.

## Experiment 1 — TEXT PRIMARY KEY on vec0

**Result: PASS.** `CREATE VIRTUAL TABLE node_embeddings USING vec0(node_id TEXT PRIMARY KEY, embedding FLOAT[384])` is accepted verbatim by sqlite-vec 0.1.9.

**This overturns the wq-2 plan's stated risk** ("some vec0 versions are rowid-only and need an auxiliary mapping table"). Not true for 0.1.9 — ADR-038's draft schema is correct as written. No aux `node_embedding_keys` mapping table is needed. Insert/query directly against `node_embeddings(node_id, embedding)`.

## Experiment 1b — KNN query through a JOIN

**Result: the obvious query shape fails; a small syntax fix passes.**

```sql
-- FAILS: "A LIMIT or 'k = ?' constraint is required on vec0 knn queries."
SELECT n.title FROM node_embeddings e
JOIN nodes n ON n.id = e.node_id
WHERE e.embedding MATCH ?1
ORDER BY e.distance
LIMIT 1;

-- PASSES:
SELECT n.title FROM node_embeddings e
JOIN nodes n ON n.id = e.node_id
WHERE e.embedding MATCH ?1 AND k = 1
ORDER BY e.distance;
```

vec0 requires the row-count bound as an explicit `k = N` predicate in the `WHERE` clause on the vec0 table itself — a downstream `LIMIT` (even `LIMIT 1`) after a join is not sufficient, sqlite-vec's query planner needs the bound local to the MATCH predicate. **Action for wq-2 GREEN:** every `search()` implementation must build `... WHERE embedding MATCH ?1 AND k = ?2 ...`, never rely on `LIMIT` alone once a JOIN is involved.

## Experiment 2 — ATTACH DATABASE + vec0 MATCH across schemas

**Result: PASS**, no caveats found. From a fresh connection with a `child` DB attached via `ATTACH DATABASE '<path>' AS child`, a qualified query —

```sql
SELECT node_id FROM child.node_embeddings
WHERE embedding MATCH ?1 AND k = 1
ORDER BY distance;
```

— resolves correctly against the attached schema's vec0 virtual table. **This resolves the wq-3 architectural fork**: rollup's semantic-search path can be one SQL statement (`ATTACH` each registered child, `UNION ALL` across `child1.node_embeddings`/`child2.node_embeddings`/... each with its own `k = N`, or attach one at a time and merge in Rust if N children is large) — it does **not** require a Rust-side per-connection merge as a hard requirement. Whether to merge via SQL `UNION ALL` vs. Rust-side top-k merging across attached children is now a performance/ergonomics choice, not a "does this work at all" blocker.

## Experiment 3 — SQLITE_MAX_ATTACHED (added 2026-07-03, wq-3 spike remainder)

**Result: 10** (the stock SQLite default) for rusqlite 0.31's bundled build.
Attach #11 fails with `too many attached databases - max 10`.

**Design consequence for wq-3's rollup()**: do NOT attach all registered
children simultaneously — a federation of >10 projects would hard-fail.
Attach one child at a time (ATTACH → query → DETACH loop), merging results
in Rust. Identical semantics (live at query time, no copies), no ceiling,
and the per-child loop is also where the cyclic-registry visited-set guard
naturally lives.

## Net effect on the phase plans

- wq-2: drop the aux-mapping-table contingency; drop rowid-only concern. Pin `rusqlite = "0.31"` explicitly in `wq-core`'s `Cargo.toml` (don't let it float to 0.40+). Bake the `k = N` requirement into every KNN query, not just a top-level one.
- wq-3: `ATTACH` + `vec0 MATCH` fork is resolved — proceed with SQL-level rollup as the default design; only escalate to a Rust-side merge if profiling later shows it's needed for many attached children.
