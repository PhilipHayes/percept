//! AQ-2: `desc:*` is any kind, and a query naming a kind or field the grammar
//! lacks is refused (`aq_core::query::check`). A fake grammar keeps these tests
//! independent of tree-sitter; `aq-treesitter` has the real-grammar ones.
use aq_core::query::check::{check, Grammar, QueryError, Scope};
use aq_core::query::parser::{Expr, ANY_KIND};
use aq_core::{eval, lex, parse, result_to_json, OwnedNode};

struct Toy;

impl Grammar for Toy {
    fn name(&self) -> &str {
        "Toy"
    }
    fn node_kinds(&self) -> Vec<String> {
        [
            "class_declaration",
            "function_declaration",
            "identifier",
            "string",
            "block",
        ]
        .map(String::from)
        .to_vec()
    }
    fn anonymous_kinds(&self) -> Vec<String> {
        ["class", "(", ")", "string"].map(String::from).to_vec()
    }
    fn field_names(&self) -> Vec<String> {
        ["name", "body", "parameters"].map(String::from).to_vec()
    }
}

fn expr(q: &str) -> Expr {
    parse(&lex(q).unwrap()).unwrap_or_else(|e| panic!("{q}: {e}"))
}

fn verdict(q: &str) -> Result<(), Box<QueryError>> {
    check(&expr(q), &Toy, Scope::Named)
}

fn err(q: &str) -> QueryError {
    *verdict(q).expect_err(q)
}

// ── desc:* ───────────────────────────────────────────────────────────────────

#[test]
fn star_filters_parse_as_any_kind() {
    for q in ["desc:*", "children:*", "desc(2):*", ".:*"] {
        match expr(q) {
            Expr::TypeFilter { types, .. } => assert_eq!(types, vec![ANY_KIND.to_string()], "{q}"),
            other => panic!("{q}: {other:?}"),
        }
    }
    // with a bracket filter, and as one stage of a pipe
    assert!(matches!(expr("desc:*[@text == \"x\"]"), Expr::Pipe(..)));
    assert!(matches!(
        expr("desc:class_declaration | children:* | @type"),
        Expr::Pipe(..)
    ));
}

#[test]
fn star_is_not_allowed_inside_a_kind_list_and_multiplication_is_untouched() {
    assert!(parse(&lex("desc:(identifier | *)").unwrap()).is_err());
    assert!(matches!(expr("(desc | length) * 2"), Expr::Arithmetic(..)));
    assert!(matches!(
        expr("desc:identifier | length * 2"),
        Expr::Pipe(..)
    ));
}

fn tree() -> OwnedNode {
    let mut leaf = OwnedNode::leaf("identifier", "Foo", 1);
    leaf.subtree_text = Some("Foo".into());
    let mut class = OwnedNode::leaf("class_declaration", "", 1);
    class.text = None;
    class.children = vec![leaf, OwnedNode::leaf("block", "{}", 1)];
    let mut root = OwnedNode::leaf("program", "", 1);
    root.text = None;
    root.children = vec![class];
    root
}

fn types(q: &str) -> Vec<String> {
    let root = tree();
    eval(&expr(q), &root)
        .unwrap()
        .iter()
        .map(|r| result_to_json(r)["@type"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn desc_star_selects_every_descendant_and_children_star_every_child() {
    assert_eq!(
        types("desc:*"),
        ["class_declaration", "identifier", "block"]
    );
    assert_eq!(types("desc"), types("desc:*"));
    assert_eq!(types("children:*"), ["class_declaration"]);
    assert_eq!(
        types("desc:class_declaration | children:*"),
        ["identifier", "block"]
    );
    assert_eq!(types("desc(1):*"), ["class_declaration"]);
}

#[test]
fn star_composes_with_a_predicate() {
    assert_eq!(types("desc:*[@text == \"Foo\"]"), ["identifier"]);
}

#[test]
fn star_is_valid_for_the_checker() {
    assert!(verdict("desc:*").is_ok());
    assert!(verdict("children:* | desc:*").is_ok());
    assert!(verdict("desc:*[@type == \"identifier\"]").is_ok());
}

// ── node kinds ───────────────────────────────────────────────────────────────

#[test]
fn a_known_kind_passes_and_an_unknown_one_is_refused_with_suggestions() {
    assert!(verdict("desc:class_declaration | .name | @text").is_ok());
    let e = err("desc:class_definition");
    assert_eq!(e.kind, "unknown-node-kind");
    assert_eq!(e.node_kind.as_deref(), Some("class_definition"));
    assert_eq!(e.language.as_deref(), Some("Toy"));
    assert_eq!(
        e.suggestions.first().map(String::as_str),
        Some("class_declaration")
    );
    assert!(e.field.is_none());
    assert!(
        e.message
            .contains("unknown node kind `class_definition` for Toy (in `desc:class_definition`)"),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("did you mean `class_declaration`"),
        "{}",
        e.message
    );
    assert!(e.message.contains("nothing was searched"), "{}", e.message);
    assert_eq!(e.to_string(), e.message);
}

#[test]
fn every_place_a_kind_can_be_named_is_checked() {
    for q in [
        "desc:(class_declaration | nope)",
        "children:nope",
        "[desc:nope] | length",
        "desc | select(@type == \"nope\")",
        "desc | select(\"nope\" != @type)",
        "match(nope)",
        "match(class_declaration > nope)",
        "match(class_declaration name:(nope))",
        "match(identifier[@type == \"nope\"])",
        "desc:identifier | if @text == \"a\" then desc:nope else . end",
        "{k: desc:nope}",
    ] {
        let e = err(q);
        assert_eq!(e.node_kind.as_deref(), Some("nope"), "{q}: {e}");
    }
}

#[test]
fn an_anonymous_token_gets_its_own_message_and_all_nodes_accepts_it() {
    let e = err("desc:class");
    assert_eq!(e.kind, "unknown-node-kind");
    assert!(e.message.contains("anonymous token"), "{}", e.message);
    assert!(check(&expr("desc:class"), &Toy, Scope::All).is_ok());
    assert!(check(&expr("desc | select(@type == \"(\")"), &Toy, Scope::All).is_ok());
    // still refused under --all-nodes when it is no token either
    assert!(check(&expr("desc:nope"), &Toy, Scope::All).is_err());
}

#[test]
fn a_name_that_is_both_node_and_token_is_a_node() {
    assert!(verdict("desc:string").is_ok());
}

#[test]
fn error_is_always_a_kind() {
    assert!(verdict("desc:ERROR").is_ok());
}

#[test]
fn a_type_regex_must_match_some_kind() {
    assert!(verdict("desc | select(@type =~ \"^class\")").is_ok());
    let e = err("desc | select(@type =~ \"^zzz\")");
    assert!(e.message.contains("matches no node kind"), "{}", e.message);
    assert_eq!(e.kind, "unknown-node-kind");
    // anonymous kinds only count under --all-nodes
    assert!(verdict("desc | select(@type =~ \"^[(]$\")").is_err());
    assert!(check(&expr("desc | select(@type =~ \"^[(]$\")"), &Toy, Scope::All).is_ok());
}

#[test]
fn a_query_naming_no_kind_or_field_is_never_refused() {
    for q in [
        "desc | length",
        ".",
        "[desc:*] | length",
        "@text",
        "desc:* | @text",
    ] {
        assert!(verdict(q).is_ok(), "{q}");
    }
}

#[test]
fn every_unknown_kind_is_named_in_one_error() {
    let e = err("desc:aaa | desc:bbb");
    assert!(
        e.message.contains("`aaa`") && e.message.contains("`bbb`"),
        "{}",
        e.message
    );
    assert_eq!(e.node_kind.as_deref(), Some("aaa"));
}

// ── fields ───────────────────────────────────────────────────────────────────

#[test]
fn an_unknown_field_is_refused_with_suggestions() {
    assert!(verdict("desc:class_declaration | .name | @text").is_ok());
    let e = err("desc:class_declaration | .nme");
    assert_eq!(e.kind, "unknown-field");
    assert_eq!(e.field.as_deref(), Some("nme"));
    assert!(e.node_kind.is_none());
    assert_eq!(e.language.as_deref(), Some("Toy"));
    assert_eq!(e.suggestions, ["name"]);
    assert!(
        e.message
            .contains("unknown field `nme` for Toy (in `.nme`); did you mean `name`?"),
        "{}",
        e.message
    );
}

#[test]
fn fields_are_checked_where_a_query_certainly_reads_them_off_a_node() {
    for q in [
        ".nme",
        "desc:class_declaration | .body | .nme",
        "desc | select(.nme)",
        "[desc:class_declaration | .nme]",
        "{n: .nme}",
        "desc:class_declaration | children:* | .nme",
        "desc:class_declaration | if .name then .nme else . end",
        "match(class_declaration nme:(identifier))",
    ] {
        assert_eq!(err(q).field.as_deref(), Some("nme"), "{q}");
    }
}

#[test]
fn a_dot_name_that_may_be_an_object_key_is_left_alone() {
    for q in [
        "desc:class_declaration | {n: @text} | .n",
        "desc:class_declaration | {n: @text, m: 1} | .m",
        "[desc:class_declaration | @text] | .[0] | .x",
        "desc:class_declaration | @text | .x",
        "desc:class_declaration | group_by(@type) | .[] | .key",
        "desc:class_declaration | first | .x",
        "desc:class_declaration | map(.x)",
    ] {
        assert!(verdict(q).is_ok(), "{q}: {:?}", verdict(q));
    }
}

#[test]
fn a_real_field_is_not_refused_even_on_a_kind_that_does_not_carry_it() {
    // Documented limit: the field table is global, not per kind.
    assert!(verdict("desc:identifier | .body").is_ok());
}

#[test]
fn a_bad_kind_and_a_bad_field_are_reported_together_kind_first() {
    let e = err("desc:class_definition | .nme");
    assert_eq!(e.kind, "unknown-node-kind");
    assert!(
        e.message.contains("class_definition") && e.message.contains("`nme`"),
        "{}",
        e.message
    );
    assert!(e.field.is_none());
}

#[test]
fn query_error_serialises_additively() {
    let e = err("desc:class_declaration | .nme");
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["kind"], "unknown-field");
    assert_eq!(v["field"], "nme");
    assert!(v.get("node_kind").is_none(), "absent, not null");
    let k = serde_json::to_value(err("desc:nope")).unwrap();
    assert!(k.get("field").is_none());
    let back: QueryError = serde_json::from_value(v).unwrap();
    assert_eq!(back, e);
}
