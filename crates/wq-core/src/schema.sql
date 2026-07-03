CREATE TABLE IF NOT EXISTS nodes (
  id TEXT PRIMARY KEY,               -- uuid
  type TEXT NOT NULL,                -- project | epic | story | ticket | decision | note
  title TEXT NOT NULL,
  body TEXT,
  status TEXT,                       -- open | in_progress | blocked | done | ...
  harness_origin TEXT,               -- which agent harness created/last touched this
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  metadata_json TEXT                 -- free-form JSON for domain-specific fields
);

CREATE TABLE IF NOT EXISTS edges (
  id TEXT PRIMARY KEY,
  from_id TEXT NOT NULL REFERENCES nodes(id),
  to_id TEXT NOT NULL REFERENCES nodes(id),
  kind TEXT NOT NULL,                -- blocks | depends_on | part_of | relates_to | supersedes | discussed_in
  weight REAL DEFAULT 1.0,
  created_at TEXT NOT NULL
);

-- Required at confirmed scale (10,000s of nodes per project) to keep
-- traversal and lookups sub-millisecond; not optional past prototype size.
CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id, kind);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id, kind);
CREATE INDEX IF NOT EXISTS idx_nodes_type_status ON nodes(type, status);

-- Semantic search (wq-2). Requires the sqlite-vec extension, registered
-- process-wide in db.rs before any connection opens. Dimension 384 is
-- coupled to mq-embed's BgeSmall default — create_node asserts
-- engine.dims() == 384 at write time and fails loudly on mismatch
-- (an existing FLOAT[384] table cannot absorb differently-sized vectors).
CREATE VIRTUAL TABLE IF NOT EXISTS node_embeddings USING vec0(
  node_id TEXT PRIMARY KEY,
  embedding FLOAT[384]
);
