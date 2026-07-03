# Contrarian review — wq family (ADR-038), 2026-07-03

5 parallel specialist reviews (architectural-skeptic, fault-finder, performance-pessimist,
api-ux-adversary, maintainability-cynic) against wq-core/wq-cli/wq-mcp as shipped through
commit `bc733f9` (wq-5 sprint close) + `357e540`/`efabc6a` (FU-wq-4-2, project_root).

No `contract.yml` exists for this feature — ADR-038 + `phases/phase-wq-{1..5}-plan.yml` +
the vec0-poc spike findings served as ground truth per the contrarian-review skill's
fallback convention.

## Verdicts

| Reviewer | Verdict |
|---|---|
| architectural-skeptic | concerns |
| fault-finder | concerns (no reachable panics; silent-wrong-output + operational failures instead) |
| performance-pessimist | concerns (honest that most of it is fine at stated scale) |
| api-ux-adversary | concerns |
| maintainability-cynic | **pass** — "unusually clean same-day build" |

## Fixed same-session (commit `0e9372b`)

- **FAULT-001** (critical, fault-finder, reproduced): `rollup()` resolved a registered
  child's relative `db_path` against the *calling process's cwd at rollup time*, not the
  registering DB's own directory — `cd` into any subdirectory before `wq rollup` silently
  dropped correctly-registered children. Fixed: `register_child` now absolutizes relative
  paths against its own DB's directory at registration time.
- **FAULT-003** (major, fault-finder, reproduced under forced contention): no
  `busy_timeout`/WAL — concurrent writers got raw `SQLITE_BUSY`. Fixed: `WqDb::init` sets
  `journal_mode=WAL` + `busy_timeout=5000`.
- **FAULT-004** (major, fault-finder, reproduced): `wq query`/`wq_query` silently executed
  only the first statement of semicolon-separated SQL, discarding the rest with no error —
  dangerous for a tool documented as "full read/write access, no guardrails." Fixed:
  `query_json` now rejects multi-statement SQL with a typed `Error::MultiStatementSql`.
- **ARCH-003 / UX-006** (major, architectural-skeptic + api-ux-adversary, independently):
  `project_root` grew the MCP surface without the ADR amendment the project's own process
  requires (the same rule that gated `wq register`). Fixed: ADR-038 Amendment (3), written
  retroactively.

## Open — logged as follow-ups, not fixed this session

- **FU-wq-7-1** (critical-adjacent, api-ux-adversary UX-001, compounds the fixed path bugs):
  no response field on any mutating tool confirms which DB a write actually resolved to.
  An agent that forgets `project_root` in a multi-project MCP session gets a fully valid
  success response with no way to detect it landed in the wrong project.
- **FU-wq-7-2** (major, fault-finder FAULT-002, **needs Phil's design call, not a bugfix**):
  `resolve_project_db_path`'s ancestor walk-up silently shares a DB across unrelated
  sibling/nested projects if a shared ancestor `.agents/wq.db` already exists — reproduced,
  and live-relevant on this exact machine (`/Users/develop/.agents/` already exists as a
  common ancestor of many distinct project directories in this workspace). Changing this
  is a behavior change for any DB already created this way — do not silently alter it.
- **FU-wq-7-3** (major, architectural-skeptic ARCH-001 + performance-pessimist PERF-001):
  `rollup()` pays a full schema-init write transaction per child, per call (WAL mitigates
  the fsync cost but not the redundant transaction itself). Needs a `WqDb::open_readonly`
  path or a "skip init if schema already present" check.
- **FU-wq-7-4** (major, architectural-skeptic ARCH-002 + performance-pessimist PERF-002):
  `rollup()` has no filter/pagination params, unlike every sibling query method
  (`list_nodes`, `search`, `traverse` all take narrowing params) — will force full-project-
  dump behavior at the ADR's own stated scale (10,000s of nodes × ~10 registered projects).
- **FU-wq-7-5** (major, performance-pessimist PERF-003): no bulk-create path; `mq-embed`
  already exposes `embed_batch` and it's never called — a 500-ticket bulk import today pays
  500 separate ONNX forward passes.
- **FU-wq-7-6** (major, api-ux-adversary UX-002): MCP `k`/`depth` silently coerce invalid
  (e.g. negative) values to defaults instead of erroring — `as_u64()` returning `None` is
  indistinguishable from "caller omitted this argument."
- **FU-wq-7-7** (major, api-ux-adversary UX-003/UX-004): `wq query`/`search`/`traverse` have
  no `--global` equivalent (can't raw-SQL-inspect the global DB); `--global` has zero help
  text anywhere and means "write with no project association" on mutating commands vs.
  "read across the registry tree" on `rollup` — same flag name, different semantics,
  undocumented either way.
- **FU-wq-7-8** (major, api-ux-adversary UX-005): edge `kind` is unvalidated free text — a
  typo (`block` vs `blocks`) silently returns an empty traversal result, indistinguishable
  from "no edges yet."
- Minor/nitpick items (MAINT-001 through 007, PERF-004/005/006/007, UX-007 through 012,
  ARCH-004/005/006): logged in the reviewer transcripts, not repeated here — none block
  anything, several are one-line comment additions whenever a file is next touched for
  another reason.

## Reviewer transcripts

Full YAML findings from all 5 reviewers are in this session's conversation history
(not re-persisted as separate files — this summary is the durable record).
