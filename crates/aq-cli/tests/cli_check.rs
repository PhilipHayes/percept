/// AQ-2: `aq` refuses a query that can only select nothing (a node kind or field
/// the grammar lacks) with an error on stderr and suggestions, and accepts
/// `desc:*`.
use std::io::Write;
use std::process::{Command, Output};

const DART: &str = "class Foo { void bar() {} }\n";
const RUST: &str = "fn alpha() {}\nstruct S;\n";

fn file(suffix: &str, src: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    f
}

fn aq(args: &[&str], files: &[&tempfile::NamedTempFile]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aq"))
        .args(args)
        .args(files.iter().map(|f| f.path()))
        .output()
        .expect("failed to run aq")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn an_unknown_kind_errors_on_stderr_with_suggestions_and_prints_nothing() {
    let d = file(".dart", DART);
    let o = aq(&["--format", "compact", "desc:class_definition"], &[&d]);
    assert!(!o.status.success());
    assert_eq!(out(&o), "");
    let e = err(&o);
    assert!(
        e.contains("unknown node kind `class_definition` for Dart"),
        "{e}"
    );
    assert!(e.contains("did you mean `class_declaration`"), "{e}");
}

#[test]
fn a_valid_kind_still_works() {
    let d = file(".dart", DART);
    let o = aq(
        &[
            "--format",
            "compact",
            "desc:class_declaration | .name | @text",
        ],
        &[&d],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o).trim(), "\"Foo\"");
}

#[test]
fn desc_star_is_accepted_and_selects_every_descendant() {
    let d = file(".dart", DART);
    let star = aq(&["--format", "compact", "desc:* | @type"], &[&d]);
    let plain = aq(&["--format", "compact", "desc | @type"], &[&d]);
    assert!(star.status.success(), "{}", err(&star));
    assert!(!out(&star).is_empty());
    assert_eq!(out(&star), out(&plain));
    let kids = aq(&["--format", "compact", "children:* | @type"], &[&d]);
    assert!(kids.status.success(), "{}", err(&kids));
}

#[test]
fn an_unknown_field_errors_with_a_suggestion() {
    let r = file(".rs", RUST);
    let o = aq(
        &["--format", "compact", "desc:function_item | .nme | @text"],
        &[&r],
    );
    assert!(!o.status.success());
    assert_eq!(out(&o), "");
    let e = err(&o);
    assert!(
        e.contains("unknown field `nme` for Rust") && e.contains("did you mean `name`"),
        "{e}"
    );
    let ok = aq(
        &["--format", "compact", "desc:function_item | .name | @text"],
        &[&r],
    );
    assert_eq!(out(&ok).trim(), "\"alpha\"");
}

#[test]
fn an_anonymous_token_is_refused_unless_all_nodes_is_given() {
    let r = file(".rs", RUST);
    let o = aq(&["--format", "compact", "desc:fn | @text"], &[&r]);
    assert!(!o.status.success());
    assert!(err(&o).contains("anonymous token"), "{}", err(&o));
    let all = aq(
        &["--all-nodes", "--format", "compact", "desc:fn | @text"],
        &[&r],
    );
    assert!(all.status.success(), "{}", err(&all));
    assert_eq!(out(&all).trim(), "\"fn\"");
}

#[test]
fn a_mixed_language_run_is_fine_when_the_query_matches_in_one_of_them() {
    let d = file(".dart", DART);
    let r = file(".rs", RUST);
    // function_item exists in Rust only
    let o = aq(
        &["--format", "compact", "desc:function_item | .name | @text"],
        &[&d, &r],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o).trim(), "\"alpha\"");
}

#[test]
fn a_mixed_language_run_errors_when_the_query_matches_in_none_and_says_each() {
    let d = file(".dart", DART);
    let r = file(".rs", RUST);
    let o = aq(&["--format", "compact", "desc:nonexistent_kind"], &[&d, &r]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(
        e.contains("any of the languages") && e.contains("for Dart") && e.contains("for Rust"),
        "{e}"
    );
}

#[test]
fn stdin_with_lang_is_checked_too() {
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_aq"))
        .args(["--lang", "dart", "desc:class_definition"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(DART.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(!o.status.success());
    assert!(err(&o).contains("class_declaration"), "{}", err(&o));
}

#[test]
fn explain_does_not_need_a_grammar() {
    let o = Command::new(env!("CARGO_BIN_EXE_aq"))
        .args(["--explain", "desc:class_definition"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
}
