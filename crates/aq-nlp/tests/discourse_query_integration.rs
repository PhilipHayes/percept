use aq_core::{lex, parse, eval, EvalResult};
use aq_core::backend::Backend;
use aq_nlp::NlpBackend;

fn spacy_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import spacy; spacy.load('en_core_web_sm')"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_query(text: &str, query: &str) -> Vec<String> {
    let backend = NlpBackend;
    let tree = backend.parse(text, "english", None).expect("parse failed");
    let tokens = lex(query).expect("lex failed");
    let ast = parse(&tokens).expect("parse failed");
    let results = eval(&ast, &tree).expect("eval failed");
    results.into_iter().map(|r| match r {
        EvalResult::Node(n) => n.text().unwrap_or(n.node_type()).to_string(),
        EvalResult::Value(v) => v.as_str().unwrap_or(&v.to_string()).to_string(),
    }).collect()
}

fn run_query_node_types(text: &str, query: &str) -> Vec<String> {
    let backend = NlpBackend;
    let tree = backend.parse(text, "english", None).expect("parse failed");
    let tokens = lex(query).expect("lex failed");
    let ast = parse(&tokens).expect("parse failed");
    let results = eval(&ast, &tree).expect("eval failed");
    results.into_iter().map(|r| match r {
        EvalResult::Node(n) => n.node_type().to_string(),
        EvalResult::Value(v) => v.to_string(),
    }).collect()
}

#[test]
fn adr_evidence_query() {
    if !spacy_available() { return; }
    let results = run_query(
        "The new algorithm is faster. Studies show that latency dropped by 50%. However, memory usage increased.",
        r#"desc:discourse[.type | @text == "evidence"]"#,
    );
    assert_eq!(results.len(), 1, "Expected 1 Evidence relation, got: {:?}", results);
}

#[test]
fn adr_contrast_query() {
    if !spacy_available() { return; }
    let results = run_query_node_types(
        "The system is fast. However, it is expensive. The team approved it. Nevertheless, concerns remain.",
        r#"desc:discourse[.type | @text == "contrast"]"#,
    );
    assert_eq!(results.len(), 2, "Expected 2 Contrast relations, got: {:?}", results);
    assert!(results.iter().all(|t| t == "discourse"));
}

#[test]
fn adr_condition_query() {
    if !spacy_available() { return; }
    let results = run_query_node_types(
        "The system shall run. If authentication fails, access is denied. Unless overridden, this is final.",
        r#"desc:discourse[.type | @text == "condition"]"#,
    );
    assert!(!results.is_empty(), "Expected at least 1 Condition relation, got: {:?}", results);
}

#[test]
fn adr_nucleus_line_query() {
    if !spacy_available() { return; }
    let results = run_query_node_types(
        "The server crashed.\nTherefore, users lost data.",
        r#"desc:discourse[.nucleus_line | @text == "1"]"#,
    );
    assert_eq!(results.len(), 1, "Expected 1 relation with nucleus on line 1, got: {:?}", results);
}

#[test]
fn connective_filter_query() {
    if !spacy_available() { return; }
    let results = run_query(
        "The system works. However, it is slow. Therefore, we need optimization.",
        r#"desc:discourse[.connective | @text == "however"]"#,
    );
    assert_eq!(results.len(), 1, "Expected 1 result for connective=However, got: {:?}", results);
}

#[test]
fn desc_discourse_enumeration() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "The system works. However, it is slow. Therefore, we need optimization.",
        "desc:discourse",
    );
    assert_eq!(types.len(), 2, "Expected 2 discourse nodes, got: {:?}", types);
    assert!(types.iter().all(|t| t == "discourse"));
}

#[test]
fn discourse_type_child() {
    if !spacy_available() { return; }
    let results = run_query(
        "The server crashed. Therefore, users lost data.",
        "desc:discourse | .type",
    );
    assert_eq!(results, vec!["cause"]);
}

#[test]
fn discourse_connective_child() {
    if !spacy_available() { return; }
    let results = run_query(
        "The system works. However, it is slow.",
        "desc:discourse | .connective",
    );
    assert_eq!(results, vec!["however"]);
}

#[test]
fn discourse_direction_child() {
    if !spacy_available() { return; }
    let results = run_query(
        "The system works. However, it is slow.",
        "desc:discourse | .direction",
    );
    assert_eq!(results, vec!["forward"]);
}

#[test]
fn discourse_scope_intra_paragraph() {
    if !spacy_available() { return; }
    let results = run_query(
        "The system works. However, it is slow.",
        "desc:discourse | .scope",
    );
    assert_eq!(results, vec!["intra_paragraph"]);
}

#[test]
fn discourse_scope_cross_paragraph() {
    if !spacy_available() { return; }
    let results = run_query(
        "The system works.\n\nHowever, it is slow.",
        "desc:discourse | .scope",
    );
    assert_eq!(results, vec!["cross_paragraph"]);
}

#[test]
fn discourse_elaboration_query() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "The test failed. Specifically, the login endpoint returned 500.",
        r#"desc:discourse[.type | @text == "elaboration"]"#,
    );
    assert_eq!(types.len(), 1);
}

#[test]
fn discourse_sequence_query() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "First, gather requirements. Then, design the system. Finally, implement.",
        r#"desc:discourse[.type | @text == "sequence"]"#,
    );
    assert_eq!(types.len(), 2, "Expected 2 Sequence relations (Then, Finally), got: {:?}", types);
}

#[test]
fn discourse_background_query() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "The project launched in 2024. Previously, the team spent two years in research.",
        r#"desc:discourse[.type | @text == "background"]"#,
    );
    assert_eq!(types.len(), 1);
}

#[test]
fn discourse_concession_query() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "The approach is expensive. Although it works, we cannot afford it.",
        r#"desc:discourse[.type | @text == "concession"]"#,
    );
    assert_eq!(types.len(), 1);
}

#[test]
fn tier1_regression_entity() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "Sarah arrived. However, Bob left.",
        "desc:entity",
    );
    assert!(types.iter().all(|t| t == "entity"));
    assert!(types.len() >= 2, "Expected at least 2 entities, got: {:?}", types);
}

#[test]
fn tier2_regression_interaction() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "Sarah arrived. Therefore, Bob celebrated.",
        "desc:interaction",
    );
    assert!(!types.is_empty(), "Expected interactions");
    assert!(types.iter().all(|t| t == "interaction"));
}

#[test]
fn no_discourse_single_sentence() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "The server is running.",
        "desc:discourse",
    );
    assert!(types.is_empty(), "Expected no discourse for single sentence, got: {:?}", types);
}

#[test]
fn discourse_and_interaction_coexist() {
    if !spacy_available() { return; }
    let discourse = run_query_node_types(
        "Sarah kicked the ball. However, Bob caught it.",
        "desc:discourse",
    );
    assert!(!discourse.is_empty(), "Expected discourse nodes");
    let interactions = run_query_node_types(
        "Sarah kicked the ball. However, Bob caught it.",
        "desc:interaction",
    );
    assert!(!interactions.is_empty(), "Expected interaction nodes");
}
