use crate::db::WqDb;
use crate::error::Result;
use crate::node::Node;

impl WqDb {
    pub fn traverse(&self, start_id: &str, kind: &str, max_depth: u32) -> Result<Vec<(Node, u32)>> {
        todo!()
    }
}
