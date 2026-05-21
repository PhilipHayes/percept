/// Integration tests: parse real source code with tree-sitter and evaluate aq queries.
use aq_core::{eval, lex, parse, result_to_json};
use aq_treesitter::langs::Language;
use aq_treesitter::parse::ParsedTree;

/// Helper: parse source, run query, return JSON values
fn query_source(source: &str, lang: Language, query: &str) -> Vec<serde_json::Value> {
    let tree = ParsedTree::parse(source.to_string(), lang, Some("test.rs".into())).unwrap();
    let root = tree.to_owned_node();
    let tokens = lex(query).unwrap();
    let expr = parse(&tokens).unwrap();
    let results = eval(&expr, &root).unwrap();
    results.iter().map(result_to_json).collect()
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

const RUST_SOURCE: &str = r#"
use std::io;
use std::fmt;

fn hello(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    fn distance(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}
"#;

#[test]
fn rust_root_type() {
    let results = query_source(RUST_SOURCE, Language::Rust, "@type");
    assert_eq!(results, vec![serde_json::json!("source_file")]);
}

#[test]
fn rust_find_functions() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:function_item | .name | @text",
    );
    assert_eq!(
        results,
        vec![
            serde_json::json!("hello"),
            serde_json::json!("add"),
            serde_json::json!("new"),
            serde_json::json!("distance"),
        ]
    );
}

#[test]
fn rust_function_line_counts() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:function_item | {name: .name | @text, lines: @end - @start}",
    );
    assert_eq!(results.len(), 4);
    for r in &results {
        assert!(r["name"].is_string());
        assert!(r["lines"].is_number());
    }
}

#[test]
fn rust_find_structs() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:struct_item | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Point")]);
}

#[test]
fn rust_use_declarations() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:use_declaration | @subtree_text",
    );
    assert_eq!(results.len(), 2);
}

#[test]
fn rust_count_functions() {
    let results = query_source(RUST_SOURCE, Language::Rust, "[desc:function_item] | length");
    assert_eq!(results, vec![serde_json::json!(4)]);
}

#[test]
fn rust_collect_and_sort() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "[desc:function_item | .name | @text] | sort_by(.) | reverse",
    );
    let arr = results[0].as_array().unwrap();
    // Alphabetical reverse order
    assert_eq!(arr[0], "new");
    assert_eq!(arr[1], "hello");
    assert_eq!(arr[2], "distance");
    assert_eq!(arr[3], "add");
}

#[test]
fn rust_select_filter() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:function_item | select(.name | @text | startswith("h")) | .name | @text"#,
    );
    assert_eq!(results, vec![serde_json::json!("hello")]);
}

#[test]
fn rust_impl_functions() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:impl_item | desc:function_item | .name | @text",
    );
    assert_eq!(
        results,
        vec![serde_json::json!("new"), serde_json::json!("distance"),]
    );
}

#[test]
fn rust_object_construction() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:struct_item | {name: .name | @text, line: @line, file: @file}"#,
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "Point");
    assert_eq!(results[0]["file"], "test.rs");
}

#[test]
fn rust_string_interpolation() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:function_item | "\(.name | @text):\(@line)""#,
    );
    assert_eq!(results.len(), 4);
    // Each should be "name:line" format
    for r in &results {
        let s = r.as_str().unwrap();
        assert!(s.contains(':'), "Expected colon in '{}'", s);
    }
}

#[test]
fn rust_join_names() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"[desc:function_item | .name | @text] | join(", ")"#,
    );
    assert_eq!(
        results,
        vec![serde_json::json!("hello, add, new, distance")]
    );
}

#[test]
fn rust_count_desc() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"count_desc("function_item")"#,
    );
    assert_eq!(results, vec![serde_json::json!(4)]);
}

#[test]
fn rust_if_then_else() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:function_item | if @end - @start > 2 then {name: .name | @text, big: true} else {name: .name | @text, big: false} end"#,
    );
    assert_eq!(results.len(), 4);
}

#[test]
fn rust_match_pattern() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "match(function_item) | .name | @text",
    );
    assert_eq!(results.len(), 4);
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

const PYTHON_SOURCE: &str = r#"
import os
from pathlib import Path

def greet(name):
    return f"Hello, {name}!"

def add(a, b):
    return a + b

class Calculator:
    def __init__(self, value=0):
        self.value = value

    def add(self, n):
        self.value += n
        return self

    def result(self):
        return self.value
"#;

#[test]
fn python_find_functions() {
    let results = query_source(
        PYTHON_SOURCE,
        Language::Python,
        "desc:function_definition | .name | @text",
    );
    // Top-level: greet, add; Class methods: __init__, add, result
    assert!(
        results.len() >= 5,
        "Expected >= 5 functions, got {}",
        results.len()
    );
}

#[test]
fn python_find_classes() {
    let results = query_source(
        PYTHON_SOURCE,
        Language::Python,
        "desc:class_definition | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Calculator")]);
}

#[test]
fn python_import_statements() {
    let results = query_source(
        PYTHON_SOURCE,
        Language::Python,
        "desc:(import_statement | import_from_statement) | @subtree_text",
    );
    assert_eq!(results.len(), 2);
}

// ---------------------------------------------------------------------------
// JavaScript
// ---------------------------------------------------------------------------

const JS_SOURCE: &str = r#"
const greet = (name) => `Hello, ${name}!`;

function add(a, b) {
    return a + b;
}

class Calculator {
    constructor(value = 0) {
        this.value = value;
    }

    add(n) {
        this.value += n;
        return this;
    }
}

export { Calculator };
"#;

#[test]
fn js_find_functions() {
    let results = query_source(
        JS_SOURCE,
        Language::JavaScript,
        "desc:function_declaration | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("add")]);
}

#[test]
fn js_find_classes() {
    let results = query_source(
        JS_SOURCE,
        Language::JavaScript,
        "desc:class_declaration | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Calculator")]);
}

#[test]
fn js_children_count() {
    let results = query_source(JS_SOURCE, Language::JavaScript, "[children] | length");
    // Should have several top-level declarations
    assert!(results[0].as_u64().unwrap() >= 3);
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

const JSON_SOURCE: &str = r#"{
    "name": "aq",
    "version": "0.1.0",
    "dependencies": {
        "serde": "1.0",
        "clap": "4.0"
    }
}"#;

#[test]
fn json_root_type() {
    let results = query_source(JSON_SOURCE, Language::Json, "@type");
    assert_eq!(results, vec![serde_json::json!("document")]);
}

#[test]
fn json_find_pairs() {
    let results = query_source(JSON_SOURCE, Language::Json, "desc:pair | .key | @text");
    assert!(
        results.len() >= 4,
        "Expected >= 4 pairs, got {}",
        results.len()
    );
}

// ---------------------------------------------------------------------------
// Complex end-to-end queries
// ---------------------------------------------------------------------------

#[test]
fn e2e_tech_debt_big_functions() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:function_item | select(@end - @start > 2) | {name: .name | @text, lines: @end - @start}",
    );
    for r in &results {
        assert!(r["lines"].as_i64().unwrap() > 2);
    }
}

#[test]
fn e2e_array_pipeline() {
    // Collect → sort → reverse → limit → iterate → string interpolation
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"[desc:function_item | {name: .name | @text, lines: @end - @start}] | sort_by(.lines) | reverse | limit(2) | .[] | "\(.name): \(.lines) lines""#,
    );
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.as_str().unwrap().contains("lines"));
    }
}

#[test]
fn e2e_unique_types() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "[children | @type] | unique_by(.)",
    );
    let arr = results[0].as_array().unwrap();
    // Should have unique types (use_declaration, function_item, struct_item, impl_item)
    assert!(arr.len() >= 3);
}

#[test]
fn e2e_csv_format() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "desc:function_item | [.name | @text, @line, @end] | @csv",
    );
    for r in &results {
        let s = r.as_str().unwrap();
        assert!(s.contains(','), "CSV should contain commas: {}", s);
    }
}

#[test]
fn e2e_group_by() {
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        "[desc:function_item | {name: .name | @text, big: @end - @start > 2}] | group_by(.big)",
    );
    let groups = results[0].as_array().unwrap();
    assert!(groups.len() >= 1);
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

const GO_SOURCE: &str = r#"
package main

import "fmt"

func greet(name string) string {
    return fmt.Sprintf("Hello, %s!", name)
}

func add(a, b int) int {
    return a + b
}

type Point struct {
    X float64
    Y float64
}

func (p Point) Distance() float64 {
    return p.X*p.X + p.Y*p.Y
}
"#;

#[test]
fn go_find_functions() {
    let results = query_source(
        GO_SOURCE,
        Language::Go,
        "desc:function_declaration | .name | @text",
    );
    assert_eq!(
        results,
        vec![serde_json::json!("greet"), serde_json::json!("add"),]
    );
}

#[test]
fn go_find_methods() {
    let results = query_source(
        GO_SOURCE,
        Language::Go,
        "desc:method_declaration | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Distance")]);
}

#[test]
fn go_find_structs() {
    let results = query_source(
        GO_SOURCE,
        Language::Go,
        "desc:type_declaration | desc:type_spec | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Point")]);
}

#[test]
fn go_import_statements() {
    let results = query_source(
        GO_SOURCE,
        Language::Go,
        "desc:import_declaration | @subtree_text",
    );
    assert_eq!(results.len(), 1);
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

const JAVA_SOURCE: &str = r#"
import java.util.List;

public class Calculator {
    private int value;

    public Calculator(int v) {
        this.value = v;
    }

    public int add(int n) {
        return value + n;
    }

    public int result() {
        return value;
    }

    public static void main(String[] args) {
        Calculator c = new Calculator(0);
    }
}
"#;

#[test]
fn java_find_classes() {
    let results = query_source(
        JAVA_SOURCE,
        Language::Java,
        "desc:class_declaration | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Calculator")]);
}

#[test]
fn java_find_methods() {
    let results = query_source(
        JAVA_SOURCE,
        Language::Java,
        "desc:method_declaration | .name | @text",
    );
    assert_eq!(
        results,
        vec![
            serde_json::json!("add"),
            serde_json::json!("result"),
            serde_json::json!("main"),
        ]
    );
}

#[test]
fn java_import_statements() {
    let results = query_source(
        JAVA_SOURCE,
        Language::Java,
        "desc:import_declaration | @subtree_text",
    );
    assert_eq!(results.len(), 1);
}

#[test]
fn java_method_count() {
    let results = query_source(
        JAVA_SOURCE,
        Language::Java,
        "[desc:method_declaration] | length",
    );
    assert_eq!(results, vec![serde_json::json!(3)]);
}

// ---------------------------------------------------------------------------
// C
// ---------------------------------------------------------------------------

const C_SOURCE: &str = r#"
#include <stdio.h>

int add(int a, int b) {
    return a + b;
}

void greet(const char* name) {
    printf("Hello, %s!\n", name);
}

struct Point {
    double x;
    double y;
};
"#;

#[test]
fn c_find_functions() {
    let results = query_source(
        C_SOURCE,
        Language::C,
        "desc:function_definition | .declarator | .declarator | @text",
    );
    assert_eq!(
        results,
        vec![serde_json::json!("add"), serde_json::json!("greet"),]
    );
}

#[test]
fn c_find_structs() {
    let results = query_source(
        C_SOURCE,
        Language::C,
        "desc:struct_specifier | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Point")]);
}

#[test]
fn c_top_level_types() {
    let results = query_source(C_SOURCE, Language::C, "children | @type");
    let types: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(types.contains(&"preproc_include"));
    assert!(types.contains(&"function_definition"));
}

// ---------------------------------------------------------------------------
// C++
// ---------------------------------------------------------------------------

const CPP_SOURCE: &str = r#"
#include <string>

class Point {
public:
    double x, y;

    Point(double x, double y) : x(x), y(y) {}

    double distance() const {
        return x * x + y * y;
    }
};

int add(int a, int b) {
    return a + b;
}
"#;

#[test]
fn cpp_find_classes() {
    let results = query_source(
        CPP_SOURCE,
        Language::Cpp,
        "desc:class_specifier | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Point")]);
}

#[test]
fn cpp_find_functions() {
    let results = query_source(
        CPP_SOURCE,
        Language::Cpp,
        "desc:function_definition | .declarator | @text",
    );
    assert!(
        results.len() >= 2,
        "Expected >= 2 functions, got {}",
        results.len()
    );
}

#[test]
fn cpp_top_level_types() {
    let results = query_source(CPP_SOURCE, Language::Cpp, "children | @type");
    let types: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(types.contains(&"class_specifier") || types.contains(&"declaration"));
}

// ---------------------------------------------------------------------------
// Dart
// ---------------------------------------------------------------------------

const DART_SOURCE: &str = r#"
import 'dart:math';

class Calculator {
  int value = 0;

  int add(int n) {
    value += n;
    return value;
  }

  int result() {
    return value;
  }
}

String greet(String name) {
  return "Hello, $name!";
}
"#;

#[test]
fn dart_find_classes() {
    let results = query_source(
        DART_SOURCE,
        Language::Dart,
        "desc:class_declaration | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Calculator")]);
}

#[test]
fn dart_find_function_signatures() {
    let results = query_source(
        DART_SOURCE,
        Language::Dart,
        "desc:function_signature | .name | @text",
    );
    // Should find class methods + top-level functions
    assert!(
        results.len() >= 2,
        "Expected >= 2 function signatures, got {}",
        results.len()
    );
}

#[test]
fn dart_import_statements() {
    let results = query_source(
        DART_SOURCE,
        Language::Dart,
        "desc:import_or_export | @subtree_text",
    );
    assert_eq!(results.len(), 1);
}

#[test]
fn dart_top_level_types() {
    let results = query_source(DART_SOURCE, Language::Dart, "children | @type");
    let types: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(types.contains(&"class_declaration"));
    assert!(types.contains(&"import_or_export"));
}

// ---------------------------------------------------------------------------
// Dart 3.x (RED tests — require tree-sitter-dart 0.1.0+)
// ---------------------------------------------------------------------------

const DART3_SOURCE: &str = r#"
sealed class Shape {
  double get area;
}

final class Circle extends Shape {
  final double radius;
  Circle(this.radius);

  @override
  double get area => 3.14159 * radius * radius;
}

base class Animal {
  final String name;
  Animal(this.name);
}

enum Color {
  red,
  green,
  blue;

  String get hex => switch (this) {
    Color.red => '#FF0000',
    Color.green => '#00FF00',
    Color.blue => '#0000FF',
  };
}

mixin Printable {
  void printSelf() => print(toString());
}

extension StringExt on String {
  bool get isBlank => trim().isEmpty;
}
"#;

/// sealed/final/base class bodies are parsed as class_definition nodes with 0.1.0+.
/// With 0.0.4 they produce ERROR nodes, so "Shape", "Circle", and "Animal" are missing.
#[test]
fn dart3_sealed_class_parses() {
    let results = query_source(
        DART3_SOURCE,
        Language::Dart,
        "desc:class_declaration | .name | @text",
    );
    let names: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        names.contains(&"Shape"),
        "expected 'Shape' in class names, got {:?}",
        names
    );
    assert!(
        names.contains(&"Circle"),
        "expected 'Circle' in class names, got {:?}",
        names
    );
    assert!(
        names.contains(&"Animal"),
        "expected 'Animal' in class names, got {:?}",
        names
    );
}

/// With 0.1.0+ the tree should be error-free. With 0.0.4 there are ERROR nodes
/// wherever sealed/final/base class modifiers appear.
#[test]
fn dart3_no_error_nodes() {
    let tree = ParsedTree::parse(
        DART3_SOURCE.to_string(),
        Language::Dart,
        Some("test.dart".into()),
    )
    .unwrap();
    let m = tree.metrics();
    assert_eq!(
        m.error_nodes, 0,
        "Dart 3.x source should have zero ERROR nodes (got {}); upgrade to tree-sitter-dart 0.1.0+",
        m.error_nodes
    );
}

/// Enhanced enum (with method body) requires 0.1.0+.
/// With 0.0.4 the `Color` enum may also produce ERROR nodes due to the `hex` getter.
#[test]
fn dart3_enhanced_enum() {
    let results = query_source(
        DART3_SOURCE,
        Language::Dart,
        "desc:enum_declaration | .name | @text",
    );
    let names: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        names.contains(&"Color"),
        "expected 'Color' in enum names, got {:?}",
        names
    );
}

/// mixin declaration — supported since Dart 2.1, present in tree-sitter-dart 0.0.4.
/// Expected to PASS even on 0.0.4.
#[test]
fn dart3_mixin_declaration() {
    let results = query_source(
        DART3_SOURCE,
        Language::Dart,
        "desc:mixin_declaration | .name | @text",
    );
    let names: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        names.contains(&"Printable"),
        "expected 'Printable' in mixin names, got {:?}",
        names
    );
}

/// extension declaration — supported since Dart 2.7, present in tree-sitter-dart 0.0.4.
/// Expected to PASS even on 0.0.4.
#[test]
fn dart3_extension_declaration() {
    let results = query_source(
        DART3_SOURCE,
        Language::Dart,
        "desc:extension_declaration | .name | @text",
    );
    let names: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        names.contains(&"StringExt"),
        "expected 'StringExt' in extension names, got {:?}",
        names
    );
}

// ---------------------------------------------------------------------------
// Swift
// ---------------------------------------------------------------------------

const SWIFT_SOURCE: &str = r#"
import Foundation

func greet(name: String) -> String {
    return "Hello, \(name)!"
}

func add(a: Int, b: Int) -> Int {
    return a + b
}

class Point {
    var x: Double
    var y: Double

    init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }

    func distance() -> Double {
        return x * x + y * y
    }
}
"#;

#[test]
fn swift_find_functions() {
    let results = query_source(
        SWIFT_SOURCE,
        Language::Swift,
        "desc:function_declaration | .name | @text",
    );
    assert_eq!(
        results,
        vec![
            serde_json::json!("greet"),
            serde_json::json!("add"),
            serde_json::json!("distance"),
        ]
    );
}

#[test]
fn swift_find_classes() {
    let results = query_source(
        SWIFT_SOURCE,
        Language::Swift,
        "desc:class_declaration | .name | @text",
    );
    assert_eq!(results, vec![serde_json::json!("Point")]);
}

#[test]
fn swift_function_count() {
    let results = query_source(
        SWIFT_SOURCE,
        Language::Swift,
        "[desc:function_declaration] | length",
    );
    assert_eq!(results, vec![serde_json::json!(3)]);
}

#[test]
fn swift_top_level_types() {
    let results = query_source(SWIFT_SOURCE, Language::Swift, "children | @type");
    let types: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(types.contains(&"import_declaration"));
    assert!(types.contains(&"function_declaration"));
    assert!(types.contains(&"class_declaration"));
}

// ---------------------------------------------------------------------------
// Parent / Ancestors / Siblings (cross-language)
// ---------------------------------------------------------------------------

#[test]
fn rust_parent_navigation() {
    // Identifier inside function → parent is function_item
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:identifier | select(@text == "hello") | parent | @type"#,
    );
    assert_eq!(results, vec![serde_json::json!("function_item")]);
}

#[test]
fn rust_ancestors() {
    // Identifier "hello" → ancestors chain to source_file
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:identifier | select(@text == "hello") | ancestors | @type"#,
    );
    let types: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(types.contains(&"function_item"));
    assert!(types.contains(&"source_file"));
}

#[test]
fn rust_siblings() {
    // "hello" function's siblings should include "add"
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:function_item | select(.name | @text == "hello") | siblings | select(@type == "function_item") | .name | @text"#,
    );
    let names: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"add"));
}

#[test]
fn rust_depth() {
    // Identifier is nested: source_file → function_item → identifier = depth 2
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:identifier | select(@text == "hello") | @depth"#,
    );
    assert_eq!(results.len(), 1);
    let depth = results[0].as_i64().unwrap();
    assert!(
        depth >= 2,
        "identifier should be at depth >= 2, got {}",
        depth
    );
}

#[test]
fn rust_path() {
    // Path from root to identifier "hello"
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:identifier | select(@text == "hello") | @path"#,
    );
    assert_eq!(results.len(), 1);
    let path = results[0].as_array().unwrap();
    assert_eq!(path.first().unwrap(), "source_file");
    assert_eq!(path.last().unwrap(), "identifier");
}

#[test]
fn rust_field_child_parent() {
    // .name field access should return a node with a working parent
    let results = query_source(
        RUST_SOURCE,
        Language::Rust,
        r#"desc:function_item | select(.name | @text == "hello") | .name | parent | @type"#,
    );
    assert_eq!(results, vec![serde_json::json!("function_item")]);
}

#[test]
fn python_parent_navigation() {
    let results = query_source(
        PYTHON_SOURCE,
        Language::Python,
        r#"desc:identifier | select(@text == "greet") | parent | @type"#,
    );
    assert_eq!(results.len(), 1);
    // Parent of the function name identifier is function_definition
    assert_eq!(results[0], serde_json::json!("function_definition"));
}

// ---------------------------------------------------------------------------
// Parse confidence / metrics
// ---------------------------------------------------------------------------

#[test]
fn metrics_valid_rust_has_full_confidence() {
    let tree = ParsedTree::parse(
        RUST_SOURCE.to_string(),
        Language::Rust,
        Some("test.rs".into()),
    )
    .unwrap();
    let m = tree.metrics();
    assert_eq!(m.error_nodes, 0);
    assert_eq!(m.missing_nodes, 0);
    assert_eq!(m.confidence, 1.0);
    assert!(m.total_nodes > 0);
    assert_eq!(m.source_bytes, RUST_SOURCE.len());
}

#[test]
fn metrics_broken_rust_detects_errors() {
    let broken = "fn foo() { let x = ; } fn bar(a: i32 b: i32) {}";
    let tree =
        ParsedTree::parse(broken.to_string(), Language::Rust, Some("broken.rs".into())).unwrap();
    let m = tree.metrics();
    assert!(
        m.error_nodes > 0,
        "should detect ERROR nodes in broken code"
    );
    assert!(
        m.confidence < 1.0,
        "confidence should be < 1.0 for broken code"
    );
    assert!(
        m.confidence > 0.0,
        "confidence should still be > 0.0 for partial parse"
    );
}

#[test]
fn metrics_valid_python_has_full_confidence() {
    let tree = ParsedTree::parse(
        PYTHON_SOURCE.to_string(),
        Language::Python,
        Some("test.py".into()),
    )
    .unwrap();
    let m = tree.metrics();
    assert_eq!(m.error_nodes, 0);
    assert_eq!(m.confidence, 1.0);
}

#[test]
fn metrics_empty_source() {
    let tree = ParsedTree::parse("".to_string(), Language::Rust, Some("empty.rs".into())).unwrap();
    let m = tree.metrics();
    assert_eq!(m.error_nodes, 0);
    // tree-sitter always creates a root "source_file" node even for empty input
    assert!(m.total_nodes <= 1);
    assert_eq!(m.confidence, 1.0);
    assert_eq!(m.source_bytes, 0);
}

// ---------------------------------------------------------------------------
// TOML (S9.A.-1.1 RED — Language::Toml does not exist yet; these tests
// fail at compile time, which is the correct RED state for TDD.
// S9.A.-1.2 GREEN will wire the 5-touchpoint implementation.)
// ---------------------------------------------------------------------------

const TOML_FIXTURE: &str = include_str!("fixtures/toml/Cargo.sample.toml");

/// Parse the synthetic TOML fixture and assert zero tree-sitter ERROR nodes.
#[test]
fn toml_parses_without_error_nodes() {
    let tree = ParsedTree::parse(
        TOML_FIXTURE.to_string(),
        Language::Toml,
        Some("Cargo.sample.toml".into()),
    )
    .unwrap();
    let m = tree.metrics();
    assert_eq!(
        m.error_nodes, 0,
        "TOML fixture should parse with zero ERROR nodes"
    );
}

/// The four top-level TOML table section headers must appear as bare_key nodes.
#[test]
fn toml_find_top_level_keys() {
    let results = query_source(TOML_FIXTURE, Language::Toml, "desc:bare_key | @text");
    let keys: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        keys.contains(&"package"),
        "expected section 'package', got {:?}",
        keys
    );
    assert!(
        keys.contains(&"dependencies"),
        "expected section 'dependencies', got {:?}",
        keys
    );
    assert!(
        keys.contains(&"features"),
        "expected section 'features', got {:?}",
        keys
    );
    assert!(
        keys.contains(&"dev-dependencies"),
        "expected section 'dev-dependencies', got {:?}",
        keys
    );
}

/// The [features] table must surface its three keys: default, foo, bar.
#[test]
fn toml_find_specific_feature_key() {
    let results = query_source(TOML_FIXTURE, Language::Toml, "desc:bare_key | @text");
    let keys: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        keys.contains(&"default"),
        "expected 'default' in keys, got {:?}",
        keys
    );
    assert!(
        keys.contains(&"foo"),
        "expected 'foo' in keys, got {:?}",
        keys
    );
    assert!(
        keys.contains(&"bar"),
        "expected 'bar' in keys, got {:?}",
        keys
    );
}

/// The [dependencies] table must expose 'serde' and 'tree-sitter' as bare_key nodes.
#[test]
fn toml_find_dep_by_name() {
    let results = query_source(TOML_FIXTURE, Language::Toml, "desc:bare_key | @text");
    let keys: Vec<&str> = results.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        keys.contains(&"serde"),
        "expected 'serde' in keys, got {:?}",
        keys
    );
    assert!(
        keys.contains(&"tree-sitter"),
        "expected 'tree-sitter' in keys, got {:?}",
        keys
    );
}

/// Language::from_extension("toml") must return Some(Language::Toml).
#[test]
fn toml_from_extension_returns_toml() {
    assert_eq!(Language::from_extension("toml"), Some(Language::Toml));
}

/// TreeSitterBackend::supported_languages() must contain the string "toml".
#[test]
fn toml_in_supported_languages() {
    use aq_core::backend::Backend;
    use aq_treesitter::TreeSitterBackend;
    let backend = TreeSitterBackend;
    let langs = backend.supported_languages();
    assert!(
        langs.contains(&"toml"),
        "expected \"toml\" in supported_languages, got {:?}",
        langs
    );
}
