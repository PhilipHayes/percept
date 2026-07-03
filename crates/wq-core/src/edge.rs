use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::Result;

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
        todo!()
    }

    pub fn list_edges(
        &self,
        from_id: Option<&str>,
        to_id: Option<&str>,
        kind: Option<&str>,
    ) -> Result<Vec<Edge>> {
        todo!()
    }
}
