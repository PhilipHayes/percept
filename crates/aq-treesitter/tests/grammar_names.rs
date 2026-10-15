//! AQ-2: `Language` describes its grammar to the query checker — the real
//! tree-sitter kinds and fields, not a fake.
use aq_core::query::check::{check, Scope};
use aq_core::{lex, parse};
use aq_treesitter::langs::Language;

const ALL: [Language; 16] = [
    Language::Rust,
    Language::JavaScript,
    Language::TypeScript,
    Language::TypeScriptTsx,
    Language::Python,
    Language::Json,
    Language::Go,
    Language::Java,
    Language::C,
    Language::Cpp,
    Language::Dart,
    Language::Swift,
    Language::Kotlin,
    Language::CSharp,
    Language::Toml,
    Language::Yaml,
];

fn verdict(
    lang: Language,
    q: &str,
    scope: Scope,
) -> Result<(), Box<aq_core::query::check::QueryError>> {
    check(&parse(&lex(q).unwrap()).unwrap(), &lang, scope)
}

#[test]
fn every_language_lists_sorted_unique_kinds_and_fields() {
    for lang in ALL {
        let kinds = lang.node_kinds();
        assert!(kinds.len() > 5, "{lang:?}");
        assert!(
            kinds.windows(2).all(|w| w[0] < w[1]),
            "{lang:?} kinds sorted and unique"
        );
        assert!(!kinds.iter().any(|k| k.is_empty()));
        let tokens = lang.anonymous_kinds();
        assert!(
            tokens.windows(2).all(|w| w[0] < w[1]),
            "{lang:?} tokens sorted and unique"
        );
        let fields = lang.field_names();
        assert!(
            fields.windows(2).all(|w| w[0] < w[1]),
            "{lang:?} fields sorted and unique"
        );
    }
}

#[test]
fn kinds_are_the_named_visible_ones_of_the_grammar() {
    let dart = Language::Dart.node_kinds();
    assert!(dart.contains(&"class_declaration".to_string()));
    assert!(
        !dart.contains(&"class_definition".to_string()),
        "that is Python's"
    );
    assert!(Language::Python
        .node_kinds()
        .contains(&"class_definition".to_string()));
    assert!(Language::Rust
        .node_kinds()
        .contains(&"function_item".to_string()));
    // `class` / `(` are tokens, not nodes
    assert!(!dart.contains(&"class".to_string()));
    assert!(Language::Dart
        .anonymous_kinds()
        .contains(&"class".to_string()));
    assert!(Language::Rust.anonymous_kinds().contains(&"(".to_string()));
    // hidden supertypes (`_expression`) never appear in a tree
    assert!(!Language::Rust
        .node_kinds()
        .iter()
        .any(|k| k.starts_with('_')));
}

#[test]
fn fields_are_the_grammars_field_table() {
    let rust = Language::Rust.field_names();
    for f in ["name", "body", "parameters", "return_type"] {
        assert!(rust.contains(&f.to_string()), "{f}");
    }
    assert!(!rust.contains(&"nme".to_string()));
    assert!(Language::Dart.field_names().contains(&"name".to_string()));
}

#[test]
fn the_checker_uses_the_real_grammar() {
    assert!(verdict(
        Language::Python,
        "desc:class_definition | .name | @text",
        Scope::Named
    )
    .is_ok());
    let e = verdict(Language::Dart, "desc:class_definition", Scope::Named).unwrap_err();
    assert_eq!(e.kind, "unknown-node-kind");
    assert_eq!(e.language.as_deref(), Some("Dart"));
    assert!(
        e.suggestions.contains(&"class_declaration".to_string()),
        "{:?}",
        e.suggestions
    );

    let e = verdict(Language::Rust, "desc:function_item | .nme", Scope::Named).unwrap_err();
    assert_eq!(e.kind, "unknown-field");
    assert_eq!(e.suggestions, ["name"]);

    assert!(verdict(Language::Dart, "desc:class", Scope::Named).is_err());
    assert!(verdict(Language::Dart, "desc:class", Scope::All).is_ok());
    assert!(verdict(Language::Dart, "desc:*", Scope::Named).is_ok());
}

#[test]
fn language_display_name_appears_in_the_message() {
    let e = verdict(Language::TypeScriptTsx, "desc:nope", Scope::Named).unwrap_err();
    assert!(e.message.contains("for TypeScript (TSX)"), "{}", e.message);
}

/// The kinds a Python/Java-shaped guess names are refused with the C# spelling.
#[test]
fn csharp_kinds_are_known_and_near_misses_suggest_them() {
    assert!(verdict(
        Language::CSharp,
        "desc:class_declaration | desc:method_declaration | .name",
        Scope::Named
    )
    .is_ok());
    for (guess, want) in [
        ("class_definition", "class_declaration"),
        ("interface_definition", "interface_declaration"),
        ("method_definition", "method_declaration"),
        ("namespace_definition", "namespace_declaration"),
    ] {
        let e = verdict(Language::CSharp, &format!("desc:{guess}"), Scope::Named).unwrap_err();
        assert_eq!(e.kind, "unknown-node-kind");
        assert_eq!(e.language.as_deref(), Some("C#"));
        assert!(
            e.suggestions.contains(&want.to_string()),
            "{guess}: {:?}",
            e.suggestions
        );
    }
}
