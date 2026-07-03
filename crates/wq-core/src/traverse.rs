use crate::db::WqDb;
use crate::error::{Error, Result};
use crate::node::Node;

const TRAVERSE_SQL: &str = "
WITH RECURSIVE walk(id, depth) AS (
    SELECT e.to_id AS id, 1 AS depth
    FROM edges e
    WHERE e.from_id = :start_id AND e.kind = :kind

    UNION ALL

    SELECT e.to_id, w.depth + 1
    FROM edges e
    JOIN walk w ON e.from_id = w.id
    WHERE e.kind = :kind AND w.depth < :max_depth
)
SELECT n.id, n.type, n.title, n.body, n.status, n.harness_origin, n.created_at, n.updated_at,
       n.metadata_json, MIN(w.depth) AS depth
FROM walk w
JOIN nodes n ON n.id = w.id
GROUP BY n.id
ORDER BY depth
";

impl WqDb {
    pub fn traverse(&self, start_id: &str, kind: &str, max_depth: u32) -> Result<Vec<(Node, u32)>> {
        self.assert_node_exists(start_id)?;

        let mut stmt = self.conn.prepare(TRAVERSE_SQL)?;
        let rows = stmt.query_map(
            rusqlite::named_params! {
                ":start_id": start_id,
                ":kind": kind,
                ":max_depth": max_depth,
            },
            row_to_node_with_depth,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }
}

fn row_to_node_with_depth(
    row: &rusqlite::Row,
) -> std::result::Result<(Node, u32), rusqlite::Error> {
    let metadata_json: Option<String> = row.get(8)?;
    let metadata = metadata_json
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let node = Node {
        id: row.get(0)?,
        node_type: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        status: row.get(4)?,
        harness_origin: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        metadata,
    };
    let depth: u32 = row.get(9)?;
    Ok((node, depth))
}
