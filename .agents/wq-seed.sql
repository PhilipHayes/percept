-- percept backlog, wq-shaped (ADR-038).
-- GENERATED — regenerate rather than hand-editing if you re-seed.
-- Schema comes from crates/wq-core/src/schema.sql; the vec0 embeddings
-- table is omitted here and created by wq on first open.

CREATE TABLE IF NOT EXISTS nodes (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT,
  status TEXT,
  harness_origin TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS edges (
  id TEXT PRIMARY KEY,
  from_id TEXT NOT NULL REFERENCES nodes(id),
  to_id TEXT NOT NULL REFERENCES nodes(id),
  kind TEXT NOT NULL,
  weight REAL DEFAULT 1.0,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id, kind);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id, kind);
CREATE INDEX IF NOT EXISTS idx_nodes_type_status ON nodes(type, status);

CREATE TABLE IF NOT EXISTS registry (
  project_name TEXT PRIMARY KEY,
  db_path TEXT NOT NULL,
  parent_registry_id TEXT,
  last_seen TEXT
);

INSERT INTO nodes VALUES('64b94133-6629-5773-890a-b3598b5edb14','project','percept','Agent Perception Layer — aq, gq, oq, lq, tq, mq, nq, cq, wq.','in_progress','claude-code','2026-08-11T00:00:00Z','2026-08-11T00:00:00Z','{"slug": "project"}');
INSERT INTO nodes VALUES('85309bc1-a9aa-5e06-8e43-b85264c95f39','ticket','CI does not check the wq crates at all',replace('ci.yml excludes wq-core, wq-cli and wq-mcp from build, test AND clippy, because\nthe runner cannot fetch ONNX Runtime for mq-embed. So the newest crates in the\nworkspace are the only ones nothing verifies.\n\nThere is a known way through: the `onnxruntime` wheel on pypi ships\nlibonnxruntime.so, and pypi is reachable where cdn.pyke.io is not. Fetch the\nversion ort-sys wants, point ORT_LIB_LOCATION at it and set\nORT_PREFER_DYNAMIC_LINK=1. That is exactly how wq was built and tested in a\nsandbox for ADR-038''s amendments, so it is proven, not speculative.','\n',char(10)),'open','claude-code','2026-08-11T00:00:00Z','2026-08-11T00:00:00Z','{"priority": "P1", "slug": "tk-ci-covers-wq"}');
INSERT INTO nodes VALUES('be4ac167-f847-5e47-a724-1c34d6823934','ticket','Pin the Rust toolchain so local clippy matches CI',replace('The 2026-08-11 clippy breakage was invisible locally: the sandbox had 1.94.1,\nCI resolves `stable` which is now 1.97, and `unnecessary_sort_by` did not exist\nin between. Local runs reported zero problems while CI had been red for weeks.\n\nA rust-toolchain.toml makes "clean here" mean "clean in CI". The cost is\nexplicitly bumping it, which is the point — a lint sweep becomes a deliberate\ncommit instead of a surprise on someone else''s push.','\n',char(10)),'open','claude-code','2026-08-11T00:00:00Z','2026-08-11T00:00:00Z','{"priority": "P1", "slug": "tk-pin-toolchain"}');
INSERT INTO nodes VALUES('a8d626e8-82d7-590c-bbef-62c097637058','ticket','wq doctor detects drift but nothing prevents it',replace('Residual risk recorded in ADR-038 Amendment 2026-08-11 (2). `wq reindex`\nrepairs, `wq doctor` detects, but a raw `wq query` INSERT still creates an\nunsearchable node with no warning at write time.\n\nOptions: warn on write when query_json touches `nodes`, or a trigger. Neither\nis obviously right — the escape hatch exists on purpose — which is why it was\nleft open rather than guessed at.','\n',char(10)),'open','claude-code','2026-08-11T00:00:00Z','2026-08-11T00:00:00Z','{"priority": "P3", "slug": "tk-wq-prevent-drift"}');
INSERT INTO nodes VALUES('4c3809af-51b4-5836-9548-6a46e30a57d7','ticket','aq-cli declares main.rs in two build targets',replace('Cargo warns on every build: `crates/aq-cli/src/main.rs` is present in both the\n`aq` and `nq` bin targets. Harmless, but it is noise on every single command\nand it trains people to ignore cargo warnings.','\n',char(10)),'open','claude-code','2026-08-11T00:00:00Z','2026-08-11T00:00:00Z','{"priority": "P4", "slug": "tk-aq-cli-dup-target"}');

INSERT INTO edges VALUES('56fe3717-bec0-5085-bb01-cddb79b06803','85309bc1-a9aa-5e06-8e43-b85264c95f39','64b94133-6629-5773-890a-b3598b5edb14','part_of',1.0,'2026-08-11T00:00:00Z');
INSERT INTO edges VALUES('8185371d-0267-5496-9890-bd5a64175bee','be4ac167-f847-5e47-a724-1c34d6823934','64b94133-6629-5773-890a-b3598b5edb14','part_of',1.0,'2026-08-11T00:00:00Z');
INSERT INTO edges VALUES('03bd605c-f915-5c3d-b50d-4d08198436cf','be4ac167-f847-5e47-a724-1c34d6823934','85309bc1-a9aa-5e06-8e43-b85264c95f39','relates_to',1.0,'2026-08-11T00:00:00Z');
INSERT INTO edges VALUES('6260ffb1-18b8-5915-b60f-b1b5d9a9bd5f','a8d626e8-82d7-590c-bbef-62c097637058','64b94133-6629-5773-890a-b3598b5edb14','part_of',1.0,'2026-08-11T00:00:00Z');
INSERT INTO edges VALUES('2356f4b9-5d73-51ad-a39e-56bfd8c97c33','4c3809af-51b4-5836-9548-6a46e30a57d7','64b94133-6629-5773-890a-b3598b5edb14','part_of',1.0,'2026-08-11T00:00:00Z');


