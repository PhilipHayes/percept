use std::io::Write;
/// CLI integration tests for --signatures output.
///
/// Validates that:
/// - The `name` field is ALWAYS present in every signature entry
/// - Dart-specific constructs (operators, constructors, getters, setters) produce correct names
use std::process::Command;

fn aq_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aq"))
}

fn write_temp_file(name: &str, content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(name).tempfile().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f
}

fn run_signatures(file: &tempfile::NamedTempFile) -> serde_json::Value {
    let output = aq_bin()
        .arg("--signatures")
        .arg(file.path())
        .output()
        .expect("failed to run aq");
    assert!(
        output.status.success(),
        "aq failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("invalid JSON output")
}

const DART_FULL: &str = r#"
class Calculator {
  int value = 0;
  int add(int n) { value += n; return value; }
  int result() => value;
}

String greet(String name) => "Hello, $name!";
void main() {}

class Person {
  final String name;
  Person(this.name);
  Person.fromJson(Map<String, dynamic> json) : name = json['name'];
  String get displayName => name;
  set nickname(String v) {}
  bool operator ==(Object other) => other is Person && other.name == name;
  int get hashCode => name.hashCode;
}
"#;

const RUST_SAMPLE: &str = r#"
fn hello(name: &str) -> String { format!("Hello, {}!", name) }
fn add(a: i32, b: i32) -> i32 { a + b }
struct Foo { x: i32 }
impl Foo {
    fn new(x: i32) -> Self { Foo { x } }
    fn value(&self) -> i32 { self.x }
}
"#;

#[test]
fn signatures_always_has_name_field_dart() {
    let f = write_temp_file(".dart", DART_FULL);
    let json = run_signatures(&f);
    let sigs = json["signatures"].as_array().expect("signatures array");
    assert!(!sigs.is_empty(), "Expected at least one signature");
    for sig in sigs {
        assert!(
            sig.get("name").is_some(),
            "Signature missing 'name' field: {}",
            sig
        );
        let name = sig["name"].as_str().unwrap();
        assert!(!name.is_empty(), "Signature has empty name: {}", sig);
    }
}

#[test]
fn signatures_always_has_name_field_rust() {
    let f = write_temp_file(".rs", RUST_SAMPLE);
    let json = run_signatures(&f);
    let sigs = json["signatures"].as_array().expect("signatures array");
    assert!(!sigs.is_empty(), "Expected at least one signature");
    for sig in sigs {
        assert!(
            sig.get("name").is_some(),
            "Signature missing 'name' field: {}",
            sig
        );
    }
}

#[test]
fn signatures_name_values_are_correct_dart() {
    let f = write_temp_file(".dart", DART_FULL);
    let json = run_signatures(&f);
    let sigs = json["signatures"].as_array().unwrap();
    let names: Vec<&str> = sigs.iter().map(|s| s["name"].as_str().unwrap()).collect();

    // Regular functions
    assert!(names.contains(&"add"), "missing 'add': {:?}", names);
    assert!(names.contains(&"result"), "missing 'result': {:?}", names);
    assert!(names.contains(&"greet"), "missing 'greet': {:?}", names);
    assert!(names.contains(&"main"), "missing 'main': {:?}", names);

    // Constructors
    assert!(
        names.contains(&"Person"),
        "missing constructor 'Person': {:?}",
        names
    );

    // Getters & setters
    assert!(
        names.contains(&"displayName"),
        "missing getter 'displayName': {:?}",
        names
    );
    assert!(
        names.contains(&"nickname"),
        "missing setter 'nickname': {:?}",
        names
    );
    assert!(
        names.contains(&"hashCode"),
        "missing getter 'hashCode': {:?}",
        names
    );

    // Operator — should have "operator ==" as name
    assert!(
        names.iter().any(|n| n.starts_with("operator")),
        "missing operator signature: {:?}",
        names
    );
}
