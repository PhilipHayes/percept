use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewNode {
    pub node_type: String,
    pub title: String,
    pub body: Option<String>,
    pub status: Option<String>,
    pub harness_origin: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub node_type: String,
    pub title: String,
    pub body: Option<String>,
    pub status: Option<String>,
    pub harness_origin: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpdateNode {
    pub title: Option<String>,
    pub body: Option<String>,
    pub status: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl WqDb {
    pub fn create_node(&self, new: NewNode) -> Result<Node> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let metadata_json = new
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        self.conn.execute(
            "INSERT INTO nodes (id, type, title, body, status, harness_origin, created_at, updated_at, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
            rusqlite::params![
                id,
                new.node_type,
                new.title,
                new.body,
                new.status,
                new.harness_origin,
                now,
                metadata_json,
            ],
        )?;

        self.get_node(&id)
    }

    pub fn get_node(&self, id: &str) -> Result<Node> {
        let result = self.conn.query_row(
            "SELECT id, type, title, body, status, harness_origin, created_at, updated_at, metadata_json
             FROM nodes WHERE id = ?1",
            [id],
            row_to_node,
        );

        match result {
            Ok(node) => Ok(node),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(Error::NodeNotFound(id.to_string())),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_node(&self, id: &str, update: UpdateNode) -> Result<Node> {
        let existing = self.get_node(id)?;

        let title = update.title.unwrap_or(existing.title);
        let body = update.body.or(existing.body);
        let status = update.status.or(existing.status);
        let metadata = update.metadata.or(existing.metadata);
        let metadata_json = metadata.as_ref().map(serde_json::to_string).transpose()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

        self.conn.execute(
            "UPDATE nodes SET title = ?1, body = ?2, status = ?3, metadata_json = ?4, updated_at = ?5
             WHERE id = ?6",
            rusqlite::params![title, body, status, metadata_json, now, id],
        )?;

        self.get_node(id)
    }

    pub fn list_nodes(&self, node_type: Option<&str>, status: Option<&str>) -> Result<Vec<Node>> {
        let mut sql = String::from(
            "SELECT id, type, title, body, status, harness_origin, created_at, updated_at, metadata_json
             FROM nodes",
        );
        let mut clauses = Vec::new();
        if node_type.is_some() {
            clauses.push("type = ?");
        }
        if status.is_some() {
            clauses.push("status = ?");
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at");

        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
        if let Some(t) = &node_type {
            params.push(t);
        }
        if let Some(s) = &status {
            params.push(s);
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), row_to_node)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }
}

fn row_to_node(row: &rusqlite::Row) -> std::result::Result<Node, rusqlite::Error> {
    let metadata_json: Option<String> = row.get(8)?;
    let metadata = metadata_json
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?;

    Ok(Node {
        id: row.get(0)?,
        node_type: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        status: row.get(4)?,
        harness_origin: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        metadata,
    })
}
