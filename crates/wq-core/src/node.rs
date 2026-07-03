use serde::{Deserialize, Serialize};

use crate::db::WqDb;
use crate::error::Result;

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
    pub fn create_node(&self, _new: NewNode) -> Result<Node> {
        todo!()
    }

    pub fn get_node(&self, _id: &str) -> Result<Node> {
        todo!()
    }

    pub fn update_node(&self, _id: &str, _update: UpdateNode) -> Result<Node> {
        todo!()
    }

    pub fn list_nodes(&self, _node_type: Option<&str>, _status: Option<&str>) -> Result<Vec<Node>> {
        todo!()
    }
}
