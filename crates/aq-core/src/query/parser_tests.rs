#[cfg(test)]
mod tests {
    use crate::query::lexer::lex;
    use crate::query::parser::*;

    /// Helper: parse a query string into an Expr
    fn parse_query(q: &str) -> Expr {
        let tokens = lex(q).unwrap();
        parse(&tokens).unwrap()
    }

    /// Helper: parse and expect an error
    fn parse_err(q: &str) -> ParseError {
        let tokens = lex(q).unwrap();
        parse(&tokens).unwrap_err()
    }

    // -----------------------------------------------------------------------
    // Identity and field access
    // -----------------------------------------------------------------------

    #[test]
    fn identity() {
        assert!(matches!(parse_query("."), Expr::Identity));
    }

    #[test]
    fn field_access() {
        match parse_query(".name") {
            Expr::Field(name) => assert_eq!(name, "name"),
            other => panic!("Expected Field, got {:?}", other),
        }
    }

    #[test]
    fn nested_field_access() {
        match parse_query(".body | .name") {
            Expr::Pipe(left, right) => {
                assert!(matches!(*left, Expr::Field(ref s) if s == "body"));
                assert!(matches!(*right, Expr::Field(ref s) if s == "name"));
            }
            other => panic!("Expected Pipe, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Meta accessors
    // -----------------------------------------------------------------------

    #[test]
    fn meta_type() {
        assert!(matches!(parse_query("@type"), Expr::Meta(MetaField::Type)));
    }

    #[test]
    fn meta_text() {
        assert!(matches!(parse_query("@text"), Expr::Meta(MetaField::Text)));
    }

    #[test]
    fn meta_line() {
        assert!(matches!(parse_query("@line"), Expr::Meta(MetaField::Line)));
    }

    #[test]
    fn meta_start_end() {
        assert!(matches!(parse_query("@start"), Expr::Meta(MetaField::Start)));
        assert!(matches!(parse_query("@end"), Expr::Meta(MetaField::End)));
    }

    #[test]
    fn meta_file() {
        assert!(matches!(parse_query("@file"), Expr::Meta(MetaField::File)));
    }

    #[test]
    fn meta_subtree_text() {
        assert!(matches!(parse_query("@subtree_text"), Expr::Meta(MetaField::SubtreeText)));
    }

    #[test]
    fn meta_csv_tsv_json() {
        assert!(matches!(parse_query("@csv"), Expr::Meta(MetaField::Csv)));
        assert!(matches!(parse_query("@tsv"), Expr::Meta(MetaField::Tsv)));
        assert!(matches!(parse_query("@json"), Expr::Meta(MetaField::Json)));
    }

    #[test]
    fn unknown_meta_field() {
        let err = parse_err("@bogus");
        assert!(err.message.contains("Unknown meta field"));
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    #[test]
    fn children_no_index() {
        assert!(matches!(parse_query("children"), Expr::Children(None)));
    }

    #[test]
    fn children_with_index() {
        assert!(matches!(parse_query("children[0]"), Expr::Children(Some(0))));
        assert!(matches!(parse_query("children[-1]"), Expr::Children(Some(-1))));
    }

    #[test]
    fn descendants() {
        assert!(matches!(parse_query("desc"), Expr::Descendants(None)));
    }

    #[test]
    fn descendants_with_depth() {
        assert!(matches!(parse_query("desc(3)"), Expr::Descendants(Some(3))));
    }

    #[test]
    fn parent_ancestors() {
        assert!(matches!(parse_query("parent"), Expr::Parent));
        assert!(matches!(parse_query("ancestors"), Expr::Ancestors));
    }

    #[test]
    fn siblings() {
        assert!(matches!(parse_query("siblings"), Expr::Sibling(SiblingKind::All)));
        assert!(matches!(parse_query("prev_sibling"), Expr::Sibling(SiblingKind::Prev)));
        assert!(matches!(parse_query("next_sibling"), Expr::Sibling(SiblingKind::Next)));
    }

    // -----------------------------------------------------------------------
    // Type filters
    // -----------------------------------------------------------------------

    #[test]
    fn desc_type_filter() {
        match parse_query("desc:function_item") {
            Expr::TypeFilter { axis, types } => {
                assert!(matches!(axis, Axis::Descendants(None)));
                assert_eq!(types, vec!["function_item"]);
            }
            other => panic!("Expected TypeFilter, got {:?}", other),
        }
    }

    #[test]
    fn children_type_filter() {
        match parse_query("children:identifier") {
            Expr::TypeFilter { axis, types } => {
                assert!(matches!(axis, Axis::Children));
                assert_eq!(types, vec!["identifier"]);
            }
            other => panic!("Expected TypeFilter, got {:?}", other),
        }
    }

    #[test]
    fn self_type_filter() {
        match parse_query(".:class_declaration") {
            Expr::TypeFilter { axis, types } => {
                assert!(matches!(axis, Axis::Self_));
                assert_eq!(types, vec!["class_declaration"]);
            }
            other => panic!("Expected TypeFilter, got {:?}", other),
        }
    }

    #[test]
    fn multi_type_filter() {
        match parse_query("desc:(function_item | struct_item)") {
            Expr::TypeFilter { axis, types } => {
                assert!(matches!(axis, Axis::Descendants(None)));
                assert_eq!(types, vec!["function_item", "struct_item"]);
            }
            other => panic!("Expected TypeFilter, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Select
    // -----------------------------------------------------------------------

    #[test]
    fn select_expression() {
        match parse_query("select(@type == \"foo\")") {
            Expr::Select(inner) => {
                assert!(matches!(*inner, Expr::Compare(_, CmpOp::Eq, _)));
            }
            other => panic!("Expected Select, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Pipe
    // -----------------------------------------------------------------------

    #[test]
    fn pipe_chain() {
        match parse_query("desc | .name | @text") {
            Expr::Pipe(_, _) => {} // success
            other => panic!("Expected Pipe, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Comparison operators
    // -----------------------------------------------------------------------

    #[test]
    fn comparison_eq() {
        match parse_query("@type == \"foo\"") {
            Expr::Compare(_, CmpOp::Eq, _) => {}
            other => panic!("Expected Compare Eq, got {:?}", other),
        }
    }

    #[test]
    fn comparison_regex() {
        match parse_query("@text =~ \"^test_\"") {
            Expr::Compare(_, CmpOp::RegexMatch, _) => {}
            other => panic!("Expected Compare RegexMatch, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Arithmetic
    // -----------------------------------------------------------------------

    #[test]
    fn arithmetic_add_sub() {
        match parse_query("@end - @start + 1") {
            Expr::Arithmetic(_, ArithOp::Add, _) => {} // + is outer
            other => panic!("Expected Arithmetic, got {:?}", other),
        }
    }

    #[test]
    fn arithmetic_mul_precedence() {
        // `2 + 3 * 4` should be `2 + (3 * 4)`
        match parse_query("2 + 3 * 4") {
            Expr::Arithmetic(_, ArithOp::Add, right) => {
                assert!(matches!(*right, Expr::Arithmetic(_, ArithOp::Mul, _)));
            }
            other => panic!("Expected Add(_, Mul), got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Logic
    // -----------------------------------------------------------------------

    #[test]
    fn logic_and_or() {
        match parse_query("true and false or true") {
            Expr::Logic(_, LogicOp::Or, _) => {} // `or` is outer (lower precedence)
            other => panic!("Expected Logic Or, got {:?}", other),
        }
    }

    #[test]
    fn logic_not() {
        match parse_query("not true") {
            Expr::LogicNot(_) => {}
            other => panic!("Expected LogicNot, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Alternative operator
    // -----------------------------------------------------------------------

    #[test]
    fn alternative() {
        match parse_query(".name // \"default\"") {
            Expr::Alternative(_, _) => {}
            other => panic!("Expected Alternative, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Object construction
    // -----------------------------------------------------------------------

    #[test]
    fn object_construction() {
        match parse_query("{name: .name, line: @line}") {
            Expr::Object(pairs) => {
                assert_eq!(pairs.len(), 2);
                assert_eq!(pairs[0].0, "name");
                assert_eq!(pairs[1].0, "line");
            }
            other => panic!("Expected Object, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Array construction
    // -----------------------------------------------------------------------

    #[test]
    fn array_collect() {
        match parse_query("[desc:function_item]") {
            Expr::Array(_) => {}
            other => panic!("Expected Array, got {:?}", other),
        }
    }

    #[test]
    fn array_literal() {
        match parse_query("[1, 2, 3]") {
            Expr::Array(inner) => {
                assert!(matches!(*inner, Expr::Concat(_)));
            }
            other => panic!("Expected Array(Concat), got {:?}", other),
        }
    }

    #[test]
    fn empty_array() {
        match parse_query("[]") {
            Expr::Literal(Value::Array(a)) => assert!(a.is_empty()),
            other => panic!("Expected empty array, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Iterate and index
    // -----------------------------------------------------------------------

    #[test]
    fn iterate() {
        assert!(matches!(parse_query(".[]"), Expr::Iterate));
    }

    #[test]
    fn index() {
        assert!(matches!(parse_query(".[0]"), Expr::Index(0)));
        assert!(matches!(parse_query(".[-1]"), Expr::Index(-1)));
    }

    // -----------------------------------------------------------------------
    // Literals
    // -----------------------------------------------------------------------

    #[test]
    fn literal_string() {
        match parse_query("\"hello\"") {
            Expr::Literal(Value::String(s)) => assert_eq!(s, "hello"),
            other => panic!("Expected String literal, got {:?}", other),
        }
    }

    #[test]
    fn literal_number() {
        match parse_query("42") {
            Expr::Literal(Value::Number(n)) => assert_eq!(n, 42.0),
            other => panic!("Expected Number literal, got {:?}", other),
        }
    }

    #[test]
    fn literal_bool() {
        assert!(matches!(parse_query("true"), Expr::Literal(Value::Bool(true))));
        assert!(matches!(parse_query("false"), Expr::Literal(Value::Bool(false))));
    }

    #[test]
    fn literal_null() {
        assert!(matches!(parse_query("null"), Expr::Literal(Value::Null)));
    }

    // -----------------------------------------------------------------------
    // String interpolation
    // -----------------------------------------------------------------------

    #[test]
    fn string_interpolation() {
        match parse_query(r#""\(.name | @text) at line \(@line)""#) {
            Expr::StringInterp(parts) => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(parts[0], StringPart::Interpolation(_)));
                assert!(matches!(parts[1], StringPart::Literal(ref s) if s == " at line "));
                assert!(matches!(parts[2], StringPart::Interpolation(_)));
            }
            other => panic!("Expected StringInterp, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Builtins
    // -----------------------------------------------------------------------

    #[test]
    fn zero_arg_builtin() {
        match parse_query("length") {
            Expr::Builtin(name, args) => {
                assert_eq!(name, "length");
                assert!(args.is_empty());
            }
            other => panic!("Expected Builtin, got {:?}", other),
        }
    }

    #[test]
    fn builtin_with_args() {
        match parse_query("startswith(\"test\")") {
            Expr::Builtin(name, args) => {
                assert_eq!(name, "startswith");
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected Builtin, got {:?}", other),
        }
    }

    #[test]
    fn builtin_multiple_args() {
        match parse_query("limit(3)") {
            Expr::Builtin(name, args) => {
                assert_eq!(name, "limit");
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected Builtin, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // If-then-else
    // -----------------------------------------------------------------------

    #[test]
    fn if_then_else() {
        match parse_query("if @type == \"foo\" then .name else null end") {
            Expr::IfThenElse { else_branch, .. } => {
                assert!(else_branch.is_some());
            }
            other => panic!("Expected IfThenElse, got {:?}", other),
        }
    }

    #[test]
    fn if_then_no_else() {
        match parse_query("if true then 1 end") {
            Expr::IfThenElse { else_branch, .. } => {
                assert!(else_branch.is_none());
            }
            other => panic!("Expected IfThenElse, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Match patterns
    // -----------------------------------------------------------------------

    #[test]
    fn match_single_type() {
        match parse_query("match(function_item)") {
            Expr::Match(pattern) => {
                assert_eq!(pattern.steps.len(), 1);
                assert_eq!(pattern.steps[0].node_type, "function_item");
                assert!(matches!(pattern.steps[0].combinator, Combinator::Root));
            }
            other => panic!("Expected Match, got {:?}", other),
        }
    }

    #[test]
    fn match_child_combinator() {
        match parse_query("match(impl_item > function_item)") {
            Expr::Match(pattern) => {
                assert_eq!(pattern.steps.len(), 2);
                assert!(matches!(pattern.steps[0].combinator, Combinator::Root));
                assert!(matches!(pattern.steps[1].combinator, Combinator::Child));
            }
            other => panic!("Expected Match, got {:?}", other),
        }
    }

    #[test]
    fn match_descendant_combinator() {
        match parse_query("match(impl_item function_item)") {
            Expr::Match(pattern) => {
                assert_eq!(pattern.steps.len(), 2);
                assert!(matches!(pattern.steps[1].combinator, Combinator::Descendant));
            }
            other => panic!("Expected Match, got {:?}", other),
        }
    }

    #[test]
    fn match_with_predicate() {
        match parse_query(r#"match(identifier[@text == "test"])"#) {
            Expr::Match(pattern) => {
                assert_eq!(pattern.steps[0].predicates.len(), 1);
                assert_eq!(pattern.steps[0].predicates[0].field, "text");
                assert_eq!(pattern.steps[0].predicates[0].value, "test");
            }
            other => panic!("Expected Match with predicate, got {:?}", other),
        }
    }

    #[test]
    fn match_with_capture() {
        match parse_query("match(function_item @fn)") {
            Expr::Match(pattern) => {
                assert_eq!(pattern.steps[0].capture_name, Some("fn".into()));
            }
            other => panic!("Expected Match with capture, got {:?}", other),
        }
    }

    #[test]
    fn match_with_field_constraint() {
        match parse_query("match(function_item name:(identifier))") {
            Expr::Match(pattern) => {
                assert_eq!(pattern.steps[0].field_constraint, Some(("name".into(), "identifier".into())));
            }
            other => panic!("Expected Match with field constraint, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Grouping
    // -----------------------------------------------------------------------

    #[test]
    fn parenthesized_grouping() {
        // (1 + 2) * 3 — grouping changes precedence
        match parse_query("(1 + 2) * 3") {
            Expr::Arithmetic(left, ArithOp::Mul, _) => {
                assert!(matches!(*left, Expr::Arithmetic(_, ArithOp::Add, _)));
            }
            other => panic!("Expected Mul(Add, _), got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Complex real-world queries
    // -----------------------------------------------------------------------

    #[test]
    fn complex_pipeline() {
        // Should parse without errors
        let _ = parse_query(
            "desc:function_item | select(@end - @start > 50) | {name: .name | @text, lines: @end - @start}"
        );
    }

    #[test]
    fn complex_with_alternative() {
        let _ = parse_query(".return_type | @text // \"void\"");
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[test]
    fn trailing_token_error() {
        let err = parse_err(". )");
        assert!(err.message.contains("Unexpected token"));
    }

    #[test]
    fn missing_closing_paren() {
        let err = parse_err("select(");
        // Should error on missing RParen or unexpected EOF
        assert!(
            err.message.contains("Expected") || err.message.contains("Unexpected"),
            "Unexpected error message: {}",
            err.message,
        );
    }

    // --- Bracket-filter syntax ---

    #[test]
    fn bracket_filter_desc_type() {
        // desc:function_item[pred] → Pipe(TypeFilter, Select(pred))
        let expr = parse_query("desc:function_item[.name | @text == \"main\"]");
        match &expr {
            Expr::Pipe(left, right) => {
                assert!(matches!(left.as_ref(), Expr::TypeFilter { .. }));
                assert!(matches!(right.as_ref(), Expr::Select(_)));
            }
            _ => panic!("Expected Pipe(TypeFilter, Select), got {:?}", expr),
        }
    }

    #[test]
    fn bracket_filter_children_type() {
        let expr = parse_query("children:identifier[@text == \"foo\"]");
        match &expr {
            Expr::Pipe(left, right) => {
                assert!(matches!(left.as_ref(), Expr::TypeFilter { .. }));
                assert!(matches!(right.as_ref(), Expr::Select(_)));
            }
            _ => panic!("Expected Pipe(TypeFilter, Select), got {:?}", expr),
        }
    }

    #[test]
    fn bracket_filter_multi_type() {
        let expr = parse_query("desc:(function_item | struct_item)[@end - @start > 10]");
        match &expr {
            Expr::Pipe(left, right) => {
                if let Expr::TypeFilter { types, .. } = left.as_ref() {
                    assert_eq!(types.len(), 2);
                } else {
                    panic!("Expected TypeFilter with 2 types");
                }
                assert!(matches!(right.as_ref(), Expr::Select(_)));
            }
            _ => panic!("Expected Pipe(TypeFilter, Select), got {:?}", expr),
        }
    }

    #[test]
    fn bracket_filter_in_pipeline() {
        // Bracket-filter followed by more pipe stages
        let expr = parse_query("desc:function_item[.name | @text | startswith(\"test\")] | .name | @text");
        // Should parse as a pipeline
        assert!(matches!(expr, Expr::Pipe(_, _)));
    }
}
