/// CLI integration tests for Kotlin: `aq` infers the language from `.kt` / `.kts`.
use std::io::Write;
use std::process::Command;

const KOTLIN: &str = r#"
class Greeter {
    fun greet(): String = "hi"
}

fun main() {}
"#;

fn function_names(suffix: &str) -> Vec<String> {
    let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
    f.write_all(KOTLIN.as_bytes()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aq"))
        .args(["--format", "compact", "desc:function_declaration | .name | @text"])
        .arg(f.path())
        .output()
        .expect("failed to run aq");
    assert!(
        output.status.success(),
        "aq failed on {suffix}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| l.trim_matches('"').to_string())
        .collect()
}

#[test]
fn aq_infers_kotlin_from_kt_extension() {
    assert_eq!(function_names(".kt"), ["greet", "main"]);
}

#[test]
fn aq_infers_kotlin_from_kts_extension() {
    assert_eq!(function_names(".kts"), ["greet", "main"]);
}
