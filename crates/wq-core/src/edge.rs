use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEdge {
    pub from_id: String,
    pub to_id: String,
    pub kind: String,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub kind: String,
    pub weight: f64,
    pub created_at: String,
}

impl WqDb {
    pub fn create_edge(&self, new: NewEdge) -> Result<Edge> {
        self.assert_node_exists(&new.from_id)?;
        self.assert_node_exists(&new.to_id)?;

        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let weight = new.weight.unwrap_or(1.0);

        self.conn.execute(
            "INSERT INTO edges (id, from_id, to_id, kind, weight, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, new.from_id, new.to_id, new.kind, weight, now],
        )?;

        let edge = self.conn.query_row(
            "SELECT id, from_id, to_id, kind, weight, created_at FROM edges WHERE id = ?1",
            [&id],
            row_to_edge,
        )?;
        Ok(edge)
    }

    pub fn list_edges(
        &self,
        from_id: Option<&str>,
        to_id: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Vec<Edge>> {
        let mut sql =
            String::from("SELECT id, from_id, to_id, kind, weight, created_at FROM edges");
        let mut clauses = Vec::new();
        if from_id.is_some() {
            clauses.push("from_id = ?");
        }
        if to_id.is_some() {
            clauses.push("to_id = ?");
        }
        if kind.is_some() {
            clauses.push("kind = ?");
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at");

        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
        if let Some(f) = &from_id {
            params.push(f);
        }
        if let Some(t) = &to_id {
            params.push(t);
        }
        if let Some(k) = &kind {
            params.push(k);
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), row_to_edge)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub(crate) fn assert_node_exists(&self, id: &str) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        if exists {
            Ok(())
        } else {
            Err(Error::NodeNotFound(id.to_string()))
        }
    }
}

fn row_to_edge(row: &rusqlite::Row) -> std::result::Result<Edge, rusqlite::Error> {
    Ok(Edge {
        id: row.get(0)?,
        from_id: row.get(1)?,
        to_id: row.get(2)?,
        kind: row.get(3)?,
        weight: row.get(4)?,
        created_at: row.get(5)?,
    })
}
