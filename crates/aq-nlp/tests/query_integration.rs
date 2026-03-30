use aq_core::backend::Backend;
use aq_core::{eval, lex, parse, EvalResult};
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
    results
        .into_iter()
        .map(|r| match r {
            EvalResult::Node(n) => n.text().unwrap_or(n.node_type()).to_string(),
            EvalResult::Value(v) => v.as_str().unwrap_or(&v.to_string()).to_string(),
        })
        .collect()
}

fn run_query_node_types(text: &str, query: &str) -> Vec<String> {
    let backend = NlpBackend;
    let tree = backend.parse(text, "english", None).expect("parse failed");
    let tokens = lex(query).expect("lex failed");
    let ast = parse(&tokens).expect("parse failed");
    let results = eval(&ast, &tree).expect("eval failed");
    results
        .into_iter()
        .map(|r| match r {
            EvalResult::Node(n) => n.node_type().to_string(),
            EvalResult::Value(v) => v.to_string(),
        })
        .collect()
}

#[test]
fn query_all_entities() {
    if !spacy_available() {
        return;
    }
    let results = run_query_node_types("Sarah went to Paris. Bob stayed in London.", "desc:entity");
    assert_eq!(results.len(), 4, "Expected 4 entities, got: {:?}", results);
    assert!(
        results.iter().all(|t| t == "entity"),
        "All results should be entities: {:?}",
        results
    );
}

#[test]
fn query_person_entities() {
    if !spacy_available() {
        return;
    }
    let results = run_query(
        "Sarah went to Paris. Bob stayed in London.",
        r#"desc:entity | select(.type | @text == "PERSON")"#,
    );
    assert_eq!(
        results.len(),
        2,
        "Expected 2 PERSON entities, got: {:?}",
        results
    );
}

#[test]
fn query_gpe_entities() {
    if !spacy_available() {
        return;
    }
    let results = run_query(
        "Sarah went to Paris. Bob stayed in London.",
        r#"desc:entity | select(.type | @text == "GPE")"#,
    );
    assert_eq!(
        results.len(),
        2,
        "Expected 2 GPE entities, got: {:?}",
        results
    );
}

#[test]
fn query_sentences() {
    if !spacy_available() {
        return;
    }
    let results = run_query_node_types("I am happy. She is sad.", "desc:sentence");
    assert_eq!(results.len(), 2, "Expected 2 sentences, got: {:?}", results);
    assert!(
        results.iter().all(|t| t == "sentence"),
        "All results should be sentences: {:?}",
        results
    );
}

#[test]
fn query_noun_tokens() {
    if !spacy_available() {
        return;
    }
    let results = run_query(
        "The big cat sat on the mat.",
        r#"desc:token | select(.pos | @text == "NOUN")"#,
    );
    assert_eq!(
        results.len(),
        2,
        "Expected 2 NOUN tokens (cat, mat), got: {:?}",
        results
    );
}

#[test]
fn query_entity_text() {
    if !spacy_available() {
        return;
    }
    let results = run_query("Sarah went to Paris.", "desc:entity | @text");
    assert!(
        results.contains(&"Sarah".to_string()),
        "Expected 'Sarah' in {:?}",
        results
    );
    assert!(
        results.contains(&"Paris".to_string()),
        "Expected 'Paris' in {:?}",
        results
    );
}

#[test]
fn query_on_empty_document() {
    if !spacy_available() {
        return;
    }
    let results = run_query_node_types("", "desc:entity");
    assert!(
        results.is_empty(),
        "Expected no entities for empty doc, got: {:?}",
        results
    );
}

#[test]
fn query_verb_tokens() {
    if !spacy_available() {
        return;
    }
    let results = run_query(
        "Sarah chased the cat.",
        r#"desc:token | select(.pos | @text == "VERB")"#,
    );
    assert_eq!(
        results.len(),
        1,
        "Expected 1 VERB token (chased), got: {:?}",
        results
    );
    assert!(
        results.contains(&"chased".to_string()),
        "Expected 'chased' in {:?}",
        results
    );
}
