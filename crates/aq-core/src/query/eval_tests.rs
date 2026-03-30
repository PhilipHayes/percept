#[cfg(test)]
mod tests {
    use crate::node::OwnedNode;
    use crate::query::lexer::lex;
    use crate::query::parser::parse;
    use crate::query::eval::{eval, result_to_json};
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Evaluate a query against a node and return JSON values
    fn run(query: &str, node: &OwnedNode) -> Vec<serde_json::Value> {
        let tokens = lex(query).unwrap();
        let expr = parse(&tokens).unwrap();
        let results = eval(&expr, node).unwrap();
        results.iter().map(result_to_json).collect()
    }

    /// Build a simple function_item node for testing
    fn make_function(name: &str, start: usize, end: usize) -> OwnedNode {
        let name_node = OwnedNode::leaf("identifier", name, start);
        let body_node = OwnedNode {
            node_type: "block".into(),
            text: None,
            subtree_text: Some("{ ... }".into()),
            field_indices: HashMap::new(),
            children: vec![],
            start_line: start,
            end_line: end,
            source_file: None,
        };
        let mut field_indices = HashMap::new();
        field_indices.insert("name".into(), vec![0usize]); // index of name_node in children
        field_indices.insert("body".into(), vec![1usize]); // index of body_node in children
        OwnedNode {
            node_type: "function_item".into(),
            text: None,
            subtree_text: Some(format!("fn {}() {{ ... }}", name)),
            field_indices,
            children: vec![name_node, body_node],
            start_line: start,
            end_line: end,
            source_file: Some("test.rs".into()),
        }
    }

    /// Build a root (source_file) node containing some functions
    fn make_root() -> OwnedNode {
        let fn1 = make_function("foo", 1, 10);
        let fn2 = make_function("bar", 12, 30);
        let fn3 = make_function("_private", 32, 35);
        OwnedNode {
            node_type: "source_file".into(),
            text: None,
            subtree_text: None,
            field_indices: HashMap::new(),
            children: vec![fn1, fn2, fn3],
            start_line: 1,
            end_line: 35,
            source_file: Some("test.rs".into()),
        }
    }

    // -----------------------------------------------------------------------
    // Identity
    // -----------------------------------------------------------------------

    #[test]
    fn identity_returns_self() {
        let root = make_root();
        let results = run(".", &root);
        assert_eq!(results.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Meta accessors
    // -----------------------------------------------------------------------

    #[test]
    fn meta_type() {
        let root = make_root();
        let results = run("@type", &root);
        assert_eq!(results, vec![serde_json::json!("source_file")]);
    }

    #[test]
    fn meta_line() {
        let root = make_root();
        let results = run("@line", &root);
        assert_eq!(results, vec![serde_json::json!(1)]);
    }

    #[test]
    fn meta_start_end() {
        let root = make_root();
        let results = run("@start", &root);
        assert_eq!(results, vec![serde_json::json!(1)]);
        let results = run("@end", &root);
        assert_eq!(results, vec![serde_json::json!(35)]);
    }

    #[test]
    fn meta_file() {
        let root = make_root();
        let results = run("@file", &root);
        assert_eq!(results, vec![serde_json::json!("test.rs")]);
    }

    #[test]
    fn meta_text_leaf() {
        let leaf = OwnedNode::leaf("identifier", "hello", 1);
        let results = run("@text", &leaf);
        assert_eq!(results, vec![serde_json::json!("hello")]);
    }

    // -----------------------------------------------------------------------
    // Children navigation
    // -----------------------------------------------------------------------

    #[test]
    fn children_all() {
        let root = make_root();
        let results = run("children", &root);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn children_first() {
        let root = make_root();
        let results = run("children[0] | @type", &root);
        assert_eq!(results, vec![serde_json::json!("function_item")]);
    }

    #[test]
    fn children_last() {
        let root = make_root();
        let results = run("children[-1] | .name | @text", &root);
        assert_eq!(results, vec![serde_json::json!("_private")]);
    }

    // -----------------------------------------------------------------------
    // Type filter
    // -----------------------------------------------------------------------

    #[test]
    fn desc_type_filter() {
        let root = make_root();
        let results = run("desc:function_item | .name | @text", &root);
        assert_eq!(results, vec![
            serde_json::json!("foo"),
            serde_json::json!("bar"),
            serde_json::json!("_private"),
        ]);
    }

    #[test]
    fn desc_type_filter_identifier() {
        let root = make_root();
        let results = run("desc:identifier | @text", &root);
        // Each function has 1 identifier child in "name" field
        assert_eq!(results.len(), 3);
    }

    // -----------------------------------------------------------------------
    // Field access
    // -----------------------------------------------------------------------

    #[test]
    fn field_access() {
        let fn_node = make_function("my_func", 1, 10);
        let results = run(".name | @text", &fn_node);
        assert_eq!(results, vec![serde_json::json!("my_func")]);
    }

    #[test]
    fn field_missing() {
        let fn_node = make_function("my_func", 1, 10);
        let results = run(".nonexistent", &fn_node);
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // Select
    // -----------------------------------------------------------------------

    #[test]
    fn select_filter() {
        let root = make_root();
        let results = run(
            "desc:function_item | select(.name | @text | startswith(\"_\") | not) | .name | @text",
            &root,
        );
        assert_eq!(results, vec![
            serde_json::json!("foo"),
            serde_json::json!("bar"),
        ]);
    }

    #[test]
    fn select_arithmetic() {
        let root = make_root();
        let results = run(
            "desc:function_item | select(@end - @start > 10) | .name | @text",
            &root,
        );
        assert_eq!(results, vec![serde_json::json!("bar")]);
    }

    // -----------------------------------------------------------------------
    // Object construction
    // -----------------------------------------------------------------------

    #[test]
    fn object_construction() {
        let fn_node = make_function("my_func", 5, 15);
        let results = run("{name: .name | @text, line: @line}", &fn_node);
        assert_eq!(results, vec![serde_json::json!({"name": "my_func", "line": 5})]);
    }

    // -----------------------------------------------------------------------
    // Array construction
    // -----------------------------------------------------------------------

    #[test]
    fn array_collect() {
        let root = make_root();
        let results = run("[desc:function_item | .name | @text]", &root);
        assert_eq!(results, vec![serde_json::json!(["foo", "bar", "_private"])]);
    }

    #[test]
    fn array_literal() {
        let root = make_root();
        let results = run("[1, 2, 3]", &root);
        assert_eq!(results, vec![serde_json::json!([1, 2, 3])]);
    }

    // -----------------------------------------------------------------------
    // Arithmetic
    // -----------------------------------------------------------------------

    #[test]
    fn arithmetic_on_meta() {
        let fn_node = make_function("test", 5, 15);
        let results = run("@end - @start", &fn_node);
        assert_eq!(results, vec![serde_json::json!(10)]);
    }

    #[test]
    fn arithmetic_add_values() {
        let leaf = OwnedNode::leaf("num", "0", 1);
        let results = run("2 + 3", &leaf);
        assert_eq!(results, vec![serde_json::json!(5)]);
    }

    #[test]
    fn arithmetic_mul_div() {
        let leaf = OwnedNode::leaf("num", "0", 1);
        assert_eq!(run("6 * 7", &leaf), vec![serde_json::json!(42)]);
        assert_eq!(run("10 / 2", &leaf), vec![serde_json::json!(5)]);
    }

    // -----------------------------------------------------------------------
    // Comparison
    // -----------------------------------------------------------------------

    #[test]
    fn comparison_eq_true() {
        let leaf = OwnedNode::leaf("identifier", "test", 1);
        let results = run("@text == \"test\"", &leaf);
        assert_eq!(results, vec![serde_json::json!(true)]);
    }

    #[test]
    fn comparison_eq_false() {
        let leaf = OwnedNode::leaf("identifier", "test", 1);
        let results = run("@text == \"other\"", &leaf);
        assert_eq!(results, vec![serde_json::json!(false)]);
    }

    #[test]
    fn comparison_regex_match() {
        let leaf = OwnedNode::leaf("identifier", "test_foo", 1);
        let results = run("@text =~ \"^test_\"", &leaf);
        assert_eq!(results, vec![serde_json::json!(true)]);
    }

    // -----------------------------------------------------------------------
    // Alternative operator
    // -----------------------------------------------------------------------

    #[test]
    fn alternative_uses_first() {
        let leaf = OwnedNode::leaf("identifier", "hello", 1);
        let results = run("@text // \"default\"", &leaf);
        assert_eq!(results, vec![serde_json::json!("hello")]);
    }

    #[test]
    fn alternative_uses_fallback() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        // null // "default" should produce "default"
        let results = run("null // \"default\"", &leaf);
        assert_eq!(results, vec![serde_json::json!("default")]);
    }

    // -----------------------------------------------------------------------
    // String interpolation
    // -----------------------------------------------------------------------

    #[test]
    fn string_interpolation() {
        let fn_node = make_function("my_func", 5, 15);
        let results = run(r#""\(.name | @text) at line \(@line)""#, &fn_node);
        assert_eq!(results, vec![serde_json::json!("my_func at line 5")]);
    }

    // -----------------------------------------------------------------------
    // Builtins
    // -----------------------------------------------------------------------

    #[test]
    fn builtin_length_array() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 3] | length", &leaf);
        assert_eq!(results, vec![serde_json::json!(3)]);
    }

    #[test]
    fn builtin_length_string() {
        let leaf = OwnedNode::leaf("identifier", "hello", 1);
        let results = run("@text | length", &leaf);
        assert_eq!(results, vec![serde_json::json!(5)]);
    }

    #[test]
    fn builtin_length_children() {
        let root = make_root();
        let results = run("length", &root);
        assert_eq!(results, vec![serde_json::json!(3)]);
    }

    #[test]
    fn builtin_keys() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"{"a": 1, "b": 2} | keys"#, &leaf);
        let result = &results[0];
        let arr = result.as_array().unwrap();
        assert!(arr.contains(&serde_json::json!("a")));
        assert!(arr.contains(&serde_json::json!("b")));
    }

    #[test]
    fn builtin_first_last() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("[1, 2, 3] | first", &leaf), vec![serde_json::json!(1)]);
        assert_eq!(run("[1, 2, 3] | last", &leaf), vec![serde_json::json!(3)]);
    }

    #[test]
    fn builtin_startswith() {
        let leaf = OwnedNode::leaf("identifier", "test_foo", 1);
        let results = run("@text | startswith(\"test\")", &leaf);
        assert_eq!(results, vec![serde_json::json!(true)]);
    }

    #[test]
    fn builtin_endswith() {
        let leaf = OwnedNode::leaf("identifier", "test_foo", 1);
        let results = run("@text | endswith(\"foo\")", &leaf);
        assert_eq!(results, vec![serde_json::json!(true)]);
    }

    #[test]
    fn builtin_contains() {
        let leaf = OwnedNode::leaf("identifier", "test_foo", 1);
        let results = run("@text | contains(\"_fo\")", &leaf);
        assert_eq!(results, vec![serde_json::json!(true)]);
    }

    #[test]
    fn builtin_type() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("\"hello\" | type", &leaf), vec![serde_json::json!("string")]);
        assert_eq!(run("42 | type", &leaf), vec![serde_json::json!("number")]);
        assert_eq!(run("true | type", &leaf), vec![serde_json::json!("boolean")]);
        assert_eq!(run("null | type", &leaf), vec![serde_json::json!("null")]);
        assert_eq!(run("[1] | type", &leaf), vec![serde_json::json!("array")]);
    }

    #[test]
    fn builtin_not() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("true | not", &leaf), vec![serde_json::json!(false)]);
        assert_eq!(run("false | not", &leaf), vec![serde_json::json!(true)]);
    }

    #[test]
    fn builtin_map() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 3] | map(. + 10)", &leaf);
        assert_eq!(results, vec![serde_json::json!([11, 12, 13])]);
    }

    #[test]
    fn builtin_sort_by() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"[{"a": 3}, {"a": 1}, {"a": 2}] | sort_by(.a)"#, &leaf);
        assert_eq!(results, vec![serde_json::json!([{"a": 1}, {"a": 2}, {"a": 3}])]);
    }

    #[test]
    fn builtin_group_by() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"[{"t": "a"}, {"t": "b"}, {"t": "a"}] | group_by(.t)"#, &leaf);
        let groups = results[0].as_array().unwrap();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn builtin_unique_by() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 1, 3, 2] | unique_by(.)", &leaf);
        assert_eq!(results, vec![serde_json::json!([1, 2, 3])]);
    }

    #[test]
    fn builtin_limit() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 3, 4, 5] | limit(3)", &leaf);
        assert_eq!(results, vec![serde_json::json!([1, 2, 3])]);
    }

    #[test]
    fn builtin_flatten() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[[1, 2], [3, 4]] | flatten", &leaf);
        assert_eq!(results, vec![serde_json::json!([1, 2, 3, 4])]);
    }

    #[test]
    fn builtin_add_numbers() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 3] | add", &leaf);
        assert_eq!(results, vec![serde_json::json!(6)]);
    }

    #[test]
    fn builtin_add_strings() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"["a", "b", "c"] | add"#, &leaf);
        assert_eq!(results, vec![serde_json::json!("abc")]);
    }

    #[test]
    fn builtin_any_all() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("[true, false, true] | any", &leaf), vec![serde_json::json!(true)]);
        assert_eq!(run("[true, false, true] | all", &leaf), vec![serde_json::json!(false)]);
        assert_eq!(run("[true, true, true] | all", &leaf), vec![serde_json::json!(true)]);
    }

    #[test]
    fn builtin_reverse() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 3] | reverse", &leaf);
        assert_eq!(results, vec![serde_json::json!([3, 2, 1])]);
    }

    #[test]
    fn builtin_join() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"["a", "b", "c"] | join(", ")"#, &leaf);
        assert_eq!(results, vec![serde_json::json!("a, b, c")]);
    }

    #[test]
    fn builtin_split() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#""a,b,c" | split(",")"#, &leaf);
        assert_eq!(results, vec![serde_json::json!(["a", "b", "c"])]);
    }

    #[test]
    fn builtin_test() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run(r#""hello123" | test("\\d+")"#, &leaf), vec![serde_json::json!(true)]);
        assert_eq!(run(r#""hello" | test("\\d+")"#, &leaf), vec![serde_json::json!(false)]);
    }

    #[test]
    fn builtin_to_number() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run(r#""42" | to_number"#, &leaf), vec![serde_json::json!(42)]);
        assert_eq!(run("42 | tonumber", &leaf), vec![serde_json::json!(42)]);
    }

    #[test]
    fn builtin_to_string() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("42 | to_string", &leaf), vec![serde_json::json!("42")]);
        assert_eq!(run("true | tostring", &leaf), vec![serde_json::json!("true")]);
    }

    #[test]
    fn builtin_ascii_case() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run(r#""Hello" | ascii_downcase"#, &leaf), vec![serde_json::json!("hello")]);
        assert_eq!(run(r#""Hello" | ascii_upcase"#, &leaf), vec![serde_json::json!("HELLO")]);
    }

    #[test]
    fn builtin_has() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(
            run(r#"{"a": 1, "b": 2} | has("a")"#, &leaf),
            vec![serde_json::json!(true)]
        );
        assert_eq!(
            run(r#"{"a": 1} | has("z")"#, &leaf),
            vec![serde_json::json!(false)]
        );
    }

    // -----------------------------------------------------------------------
    // Iterate and index
    // -----------------------------------------------------------------------

    #[test]
    fn iterate_array() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("[1, 2, 3] | .[]", &leaf);
        assert_eq!(results, vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ]);
    }

    #[test]
    fn index_array() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("[10, 20, 30] | .[0]", &leaf), vec![serde_json::json!(10)]);
        assert_eq!(run("[10, 20, 30] | .[-1]", &leaf), vec![serde_json::json!(30)]);
    }

    // -----------------------------------------------------------------------
    // If-then-else
    // -----------------------------------------------------------------------

    #[test]
    fn if_then_else_true() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("if true then 1 else 2 end", &leaf);
        assert_eq!(results, vec![serde_json::json!(1)]);
    }

    #[test]
    fn if_then_else_false() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run("if false then 1 else 2 end", &leaf);
        assert_eq!(results, vec![serde_json::json!(2)]);
    }

    // -----------------------------------------------------------------------
    // Format meta fields
    // -----------------------------------------------------------------------

    #[test]
    fn format_csv() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"["a", "b", "c"] | @csv"#, &leaf);
        assert_eq!(results, vec![serde_json::json!("a,b,c")]);
    }

    #[test]
    fn format_tsv() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"["a", "b", "c"] | @tsv"#, &leaf);
        assert_eq!(results, vec![serde_json::json!("a\tb\tc")]);
    }

    #[test]
    fn format_json() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        let results = run(r#"{"a": 1} | @json"#, &leaf);
        let s = results[0].as_str().unwrap();
        // Should be valid compact JSON
        let parsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(parsed, serde_json::json!({"a": 1}));
    }

    // -----------------------------------------------------------------------
    // Logic operators
    // -----------------------------------------------------------------------

    #[test]
    fn logic_and() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("true and true", &leaf), vec![serde_json::json!(true)]);
        assert_eq!(run("true and false", &leaf), vec![serde_json::json!(false)]);
    }

    #[test]
    fn logic_or() {
        let leaf = OwnedNode::leaf("x", "0", 1);
        assert_eq!(run("false or true", &leaf), vec![serde_json::json!(true)]);
        assert_eq!(run("false or false", &leaf), vec![serde_json::json!(false)]);
    }

    // -----------------------------------------------------------------------
    // count_desc builtin
    // -----------------------------------------------------------------------

    #[test]
    fn builtin_count_desc() {
        let root = make_root();
        let results = run("count_desc(\"function_item\")", &root);
        assert_eq!(results, vec![serde_json::json!(3)]);
    }

    #[test]
    fn builtin_count_desc_zero() {
        let root = make_root();
        let results = run("count_desc(\"class_declaration\")", &root);
        assert_eq!(results, vec![serde_json::json!(0)]);
    }

    // -----------------------------------------------------------------------
    // Complex queries
    // -----------------------------------------------------------------------

    #[test]
    fn complex_pipeline() {
        let root = make_root();
        let results = run(
            "desc:function_item | {name: .name | @text, lines: @end - @start}",
            &root,
        );
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], serde_json::json!({"name": "foo", "lines": 9}));
        assert_eq!(results[1], serde_json::json!({"name": "bar", "lines": 18}));
    }

    #[test]
    fn collect_sort_limit() {
        let root = make_root();
        let results = run(
            "[desc:function_item | {name: .name | @text, lines: @end - @start}] | sort_by(.lines) | reverse | limit(2)",
            &root,
        );
        let arr = results[0].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "bar"); // longest first
    }

    #[test]
    fn string_interp_in_pipeline() {
        let root = make_root();
        let results = run(
            r#"desc:function_item | "\(.name | @text): \(@end - @start) lines""#,
            &root,
        );
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], serde_json::json!("foo: 9 lines"));
    }

    // -----------------------------------------------------------------------
    // Match patterns
    // -----------------------------------------------------------------------

    #[test]
    fn match_single_type() {
        let root = make_root();
        let results = run("match(function_item) | .name | @text", &root);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn match_descendant_pattern() {
        let root = make_root();
        let results = run("match(source_file function_item) | .name | @text", &root);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn match_with_text_predicate() {
        let root = make_root();
        // Match identifier nodes that have text "foo" (descendant of source_file)
        let results = run(r#"match(identifier[@text == "foo"]) | @text"#, &root);
        // Should find identifier nodes with text "foo" (inside function_item children)
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], serde_json::json!("foo"));
    }

    // -----------------------------------------------------------------------
    // Parent navigation
    // -----------------------------------------------------------------------

    #[test]
    fn parent_of_child() {
        let root = make_root();
        // A function_item's parent is source_file
        let results = run("children[0] | parent | @type", &root);
        assert_eq!(results, vec![serde_json::json!("source_file")]);
    }

    #[test]
    fn parent_of_root_is_empty() {
        let root = make_root();
        // Root node has no parent
        let results = run("parent", &root);
        assert!(results.is_empty());
    }

    #[test]
    fn parent_of_deep_node() {
        let root = make_root();
        // identifier (name field of function_item) → parent is function_item
        let results = run(
            r#"desc:identifier | select(@text == "foo") | parent | @type"#,
            &root,
        );
        assert_eq!(results, vec![serde_json::json!("function_item")]);
    }

    #[test]
    fn parent_chain() {
        let root = make_root();
        // identifier → parent (function_item) → parent (source_file)
        let results = run(
            r#"desc:identifier | select(@text == "foo") | parent | parent | @type"#,
            &root,
        );
        assert_eq!(results, vec![serde_json::json!("source_file")]);
    }

    // -----------------------------------------------------------------------
    // Ancestors
    // -----------------------------------------------------------------------

    #[test]
    fn ancestors_from_leaf() {
        let root = make_root();
        // identifier → ancestors should be [function_item, source_file]
        let results = run(
            r#"desc:identifier | select(@text == "foo") | ancestors | @type"#,
            &root,
        );
        assert_eq!(
            results,
            vec![
                serde_json::json!("function_item"),
                serde_json::json!("source_file"),
            ]
        );
    }

    #[test]
    fn ancestors_of_root_is_empty() {
        let root = make_root();
        let results = run("ancestors", &root);
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // Siblings
    // -----------------------------------------------------------------------

    #[test]
    fn siblings_all() {
        let root = make_root();
        // First function_item's siblings = the other two function_items
        let results = run(
            r#"children:function_item | select(.name | @text == "foo") | siblings | .name | @text"#,
            &root,
        );
        assert_eq!(
            results,
            vec![serde_json::json!("bar"), serde_json::json!("_private")]
        );
    }

    #[test]
    fn prev_sibling() {
        let root = make_root();
        // "bar" is the second function — prev_sibling is "foo"
        let results = run(
            r#"children:function_item | select(.name | @text == "bar") | prev_sibling | .name | @text"#,
            &root,
        );
        assert_eq!(results, vec![serde_json::json!("foo")]);
    }

    #[test]
    fn prev_sibling_of_first_is_empty() {
        let root = make_root();
        let results = run(
            r#"children:function_item | select(.name | @text == "foo") | prev_sibling"#,
            &root,
        );
        assert!(results.is_empty());
    }

    #[test]
    fn next_sibling() {
        let root = make_root();
        let results = run(
            r#"children:function_item | select(.name | @text == "bar") | next_sibling | .name | @text"#,
            &root,
        );
        assert_eq!(results, vec![serde_json::json!("_private")]);
    }

    #[test]
    fn next_sibling_of_last_is_empty() {
        let root = make_root();
        let results = run(
            r#"children:function_item | select(.name | @text == "_private") | next_sibling"#,
            &root,
        );
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // Depth
    // -----------------------------------------------------------------------

    #[test]
    fn depth_of_root() {
        let root = make_root();
        let results = run("@depth", &root);
        assert_eq!(results, vec![serde_json::json!(0)]);
    }

    #[test]
    fn depth_of_child() {
        let root = make_root();
        let results = run("children[0] | @depth", &root);
        assert_eq!(results, vec![serde_json::json!(1)]);
    }

    #[test]
    fn depth_of_leaf() {
        let root = make_root();
        // identifier is 2 levels deep: source_file → function_item → identifier
        let results = run(
            r#"desc:identifier | select(@text == "foo") | @depth"#,
            &root,
        );
        assert_eq!(results, vec![serde_json::json!(2)]);
    }

    // -----------------------------------------------------------------------
    // Path
    // -----------------------------------------------------------------------

    #[test]
    fn path_of_root() {
        let root = make_root();
        let results = run("@path", &root);
        assert_eq!(
            results,
            vec![serde_json::json!(["source_file"])]
        );
    }

    #[test]
    fn path_of_leaf() {
        let root = make_root();
        let results = run(
            r#"desc:identifier | select(@text == "foo") | @path"#,
            &root,
        );
        assert_eq!(
            results,
            vec![serde_json::json!(["source_file", "function_item", "identifier"])]
        );
    }

    // -----------------------------------------------------------------------
    // Field access returns same node as children (parent map correctness)
    // -----------------------------------------------------------------------

    #[test]
    fn field_child_has_parent() {
        let root = make_root();
        // Accessing .name via field should still have a parent (function_item)
        let results = run(
            "children[0] | .name | parent | @type",
            &root,
        );
        assert_eq!(results, vec![serde_json::json!("function_item")]);
    }
}
