use serde::{Deserialize, Serialize};

/// Universal AST node interface.
///
/// `aq-core` operates entirely through this trait. Different backends
/// (tree-sitter, Dart analyzer, custom parsers) implement it to plug
/// into the query engine.
pub trait AqNode {
    /// The node's type identifier (e.g. "function_declaration", "identifier").
    fn node_type(&self) -> &str;

    /// The source text of this node (leaf nodes have meaningful text;
    /// branch nodes may return the full subtree text).
    fn text(&self) -> Option<&str>;

    /// The full source text of this node's subtree, including all children.
    fn subtree_text(&self) -> Option<&str> {
        self.text()
    }

    /// All named children (skips anonymous/punctuation nodes by default).
    fn named_children(&self) -> Vec<&dyn AqNode>;

    /// Access a child by its field name (language-grammar-specific).
    fn child_by_field(&self, name: &str) -> Option<&dyn AqNode>;

    /// Parent node, if available.
    fn parent(&self) -> Option<&dyn AqNode>;

    /// All sibling nodes (excluding self).
    fn siblings(&self) -> Vec<&dyn AqNode> {
        vec![]
    }

    /// Previous sibling, if any.
    fn prev_sibling(&self) -> Option<&dyn AqNode> {
        None
    }

    /// Next sibling, if any.
    fn next_sibling(&self) -> Option<&dyn AqNode> {
        None
    }

    /// Start line (1-indexed).
    fn start_line(&self) -> usize;

    /// End line (1-indexed).
    fn end_line(&self) -> usize;

    /// Source file path, if known.
    fn source_file(&self) -> Option<&str>;

    /// Depth of this node from root (root = 0).
    fn depth(&self) -> usize {
        match self.parent() {
            Some(p) => p.depth() + 1,
            None => 0,
        }
    }
}

/// A simple owned node for testing and for representing query results
/// without lifetime ties to the original tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedNode {
    pub node_type: String,
    pub text: Option<String>,
    pub subtree_text: Option<String>,
    /// Field name → indices into `children`. No cloning — field access returns
    /// the same node instance as iterating children.
    pub field_indices: std::collections::HashMap<String, Vec<usize>>,
    pub children: Vec<OwnedNode>,
    pub start_line: usize,
    pub end_line: usize,
    pub source_file: Option<String>,
}

impl OwnedNode {
    pub fn leaf(node_type: impl Into<String>, text: impl Into<String>, line: usize) -> Self {
        Self {
            node_type: node_type.into(),
            text: Some(text.into()),
            subtree_text: None,
            field_indices: Default::default(),
            children: vec![],
            start_line: line,
            end_line: line,
            source_file: None,
        }
    }
}

impl AqNode for OwnedNode {
    fn node_type(&self) -> &str {
        &self.node_type
    }

    fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    fn subtree_text(&self) -> Option<&str> {
        self.subtree_text.as_deref().or(self.text.as_deref())
    }

    fn named_children(&self) -> Vec<&dyn AqNode> {
        self.children.iter().map(|c| c as &dyn AqNode).collect()
    }

    fn child_by_field(&self, name: &str) -> Option<&dyn AqNode> {
        self.field_indices
            .get(name)
            .and_then(|v| v.first())
            .and_then(|&idx| self.children.get(idx))
            .map(|c| c as &dyn AqNode)
    }

    fn parent(&self) -> Option<&dyn AqNode> {
        None
    }

    fn start_line(&self) -> usize {
        self.start_line
    }

    fn end_line(&self) -> usize {
        self.end_line
    }

    fn source_file(&self) -> Option<&str> {
        self.source_file.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Serialize → deserialize and assert equality.
    fn roundtrip(node: &OwnedNode) -> OwnedNode {
        let json = serde_json::to_string(node).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn test_owned_node_leaf_roundtrip() {
        let node = OwnedNode::leaf("entity", "Joseph", 5);
        let restored = roundtrip(&node);
        assert_eq!(node, restored);
    }

    #[test]
    fn test_owned_node_with_children_roundtrip() {
        let child_a = OwnedNode::leaf("entity", "Joseph", 1);
        let child_b = OwnedNode::leaf("entity", "Reuben", 2);
        let parent = OwnedNode {
            node_type: "scene".into(),
            text: Some("Scene 1".into()),
            subtree_text: Some("Joseph went to Reuben".into()),
            field_indices: HashMap::from([("actors".into(), vec![0, 1])]),
            children: vec![child_a, child_b],
            start_line: 1,
            end_line: 3,
            source_file: Some("genesis.txt".into()),
        };
        let restored = roundtrip(&parent);
        assert_eq!(parent, restored);
    }

    #[test]
    fn test_owned_node_field_indices_roundtrip() {
        let mut indices = HashMap::new();
        indices.insert("subject".into(), vec![0]);
        indices.insert("object".into(), vec![1, 2]);
        indices.insert("modifier".into(), vec![]);
        let node = OwnedNode {
            node_type: "interaction".into(),
            text: None,
            subtree_text: Some("Joseph told his brothers".into()),
            field_indices: indices,
            children: vec![
                OwnedNode::leaf("entity", "Joseph", 1),
                OwnedNode::leaf("entity", "brothers", 1),
                OwnedNode::leaf("entity", "flock", 2),
            ],
            start_line: 1,
            end_line: 2,
            source_file: None,
        };
        let restored = roundtrip(&node);
        assert_eq!(node, restored);
    }

    #[test]
    fn test_owned_node_optional_fields_roundtrip() {
        let node = OwnedNode {
            node_type: "token".into(),
            text: None,
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![],
            start_line: 0,
            end_line: 0,
            source_file: None,
        };
        let json = serde_json::to_string(&node).expect("serialize");
        // Verify None serializes as null
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["text"], serde_json::Value::Null);
        assert_eq!(v["source_file"], serde_json::Value::Null);
        let restored = roundtrip(&node);
        assert_eq!(node, restored);
    }
}
