use std::collections::HashMap;

use crate::langs::Language;

/// Confidence metrics from tree-sitter parsing.
///
/// Agents use this to calibrate trust in parse results — if `error_nodes > 0`,
/// some declarations may be missing from skeleton/signature output.
#[derive(Debug, Clone)]
pub struct ParseMetrics {
    /// Total bytes of source code parsed.
    pub source_bytes: usize,
    /// Total named nodes in the tree.
    pub total_nodes: usize,
    /// Number of ERROR nodes (tree-sitter couldn't parse this region).
    pub error_nodes: usize,
    /// Number of MISSING nodes (tree-sitter inferred a missing token).
    pub missing_nodes: usize,
    /// Parse confidence: 1.0 means no errors, lower means partial parse.
    pub confidence: f64,
}

/// Parse source code into a tree-sitter tree.
///
/// The returned `ParsedTree` owns the source and tree, and provides
/// access to nodes that implement `AqNode`.
pub struct ParsedTree {
    pub source: String,
    pub tree: tree_sitter::Tree,
    pub language: Language,
    pub file_path: Option<String>,
}

impl ParsedTree {
    /// Parse source code in the given language.
    pub fn parse(
        source: String,
        language: Language,
        file_path: Option<String>,
    ) -> Result<Self, ParseTreeError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.ts_language())
            .map_err(|e| ParseTreeError {
                message: format!("Failed to set language: {e}"),
            })?;

        let tree = parser.parse(&source, None).ok_or_else(|| ParseTreeError {
            message: "tree-sitter parse returned None".into(),
        })?;

        Ok(Self {
            source,
            tree,
            language,
            file_path,
        })
    }

    /// Get the root node of the parsed tree.
    pub fn root_node(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }

    /// Get the source text for a given node.
    pub fn node_text(&self, node: &tree_sitter::Node<'_>) -> &str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// Compute parse confidence metrics by walking the tree-sitter tree
    /// and counting ERROR/MISSING nodes.
    pub fn metrics(&self) -> ParseMetrics {
        let mut total_nodes: usize = 0;
        let mut error_nodes: usize = 0;
        let mut missing_nodes: usize = 0;

        // Walk the full tree using tree-sitter's cursor (no allocation per node)
        let mut cursor = self.tree.walk();
        let mut did_enter = true;
        loop {
            if did_enter {
                let node = cursor.node();
                if node.is_named() {
                    total_nodes += 1;
                }
                if node.is_error() {
                    error_nodes += 1;
                }
                if node.is_missing() {
                    missing_nodes += 1;
                }
            }

            // Depth-first traversal
            if (did_enter && cursor.goto_first_child()) || cursor.goto_next_sibling() {
                did_enter = true;
            } else if cursor.goto_parent() {
                did_enter = false;
            } else {
                break;
            }
        }

        let problematic = error_nodes + missing_nodes;
        let confidence = if total_nodes == 0 {
            1.0
        } else {
            1.0 - (problematic as f64 / total_nodes as f64)
        };

        ParseMetrics {
            source_bytes: self.source.len(),
            total_nodes,
            error_nodes,
            missing_nodes,
            confidence,
        }
    }

    /// Convert the entire tree-sitter tree into an OwnedNode tree.
    ///
    /// This copies the tree structure into owned data, decoupling it from
    /// tree-sitter lifetimes so the query engine can operate freely.
    /// Named nodes only (skips anonymous/punctuation nodes).
    pub fn to_owned_node(&self) -> aq_core::OwnedNode {
        self.convert_node(&self.root_node(), false)
    }

    /// Convert to OwnedNode tree including all nodes (anonymous/punctuation).
    pub fn to_owned_node_all(&self) -> aq_core::OwnedNode {
        self.convert_node(&self.root_node(), true)
    }

    fn convert_node(
        &self,
        ts_node: &tree_sitter::Node<'_>,
        include_all: bool,
    ) -> aq_core::OwnedNode {
        let mut children = Vec::new();
        let mut field_indices: HashMap<String, Vec<usize>> = HashMap::new();

        let mut cursor = ts_node.walk();
        if cursor.goto_first_child() {
            loop {
                let child_node = cursor.node();
                if include_all || child_node.is_named() {
                    let owned_child = self.convert_node(&child_node, include_all);
                    let idx = children.len();
                    children.push(owned_child);

                    if let Some(field_name) = cursor.field_name() {
                        field_indices
                            .entry(field_name.to_string())
                            .or_default()
                            .push(idx);
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        // Leaf nodes store text directly; branch nodes only store subtree_text
        let is_leaf = children.is_empty();
        let text = if is_leaf {
            Some(self.node_text(ts_node).to_string())
        } else {
            None
        };
        let subtree_text = Some(self.node_text(ts_node).to_string());

        aq_core::OwnedNode {
            node_type: ts_node.kind().to_string(),
            text,
            subtree_text,
            field_indices,
            children,
            start_line: ts_node.start_position().row + 1, // tree-sitter is 0-indexed
            end_line: ts_node.end_position().row + 1,
            source_file: self.file_path.clone(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Parse error: {message}")]
pub struct ParseTreeError {
    pub message: String,
}
