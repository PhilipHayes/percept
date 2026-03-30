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
fn query_all_interactions() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "Sarah chased the cat. Bob opened the door.",
        "desc:interaction",
    );
    assert!(types.len() >= 2, "Expected at least 2 interactions, got: {:?}", types);
    assert!(types.iter().all(|t| t == "interaction"));
}

#[test]
fn query_all_verb_phrases() {
    if !spacy_available() { return; }
    let types = run_query_node_types(
        "Sarah ran. The cat slept.",
        "desc:verb_phrase",
    );
    assert!(types.len() >= 2, "Expected at least 2 verb_phrases, got: {:?}", types);
    assert!(types.iter().all(|t| t == "verb_phrase"));
}

#[test]
fn query_passive_interaction_has_correct_agent() {
    if !spacy_available() { return; }
    // The critical test: passive voice -> agent should be resolved to Sarah, not "The cat"
    let interactions = run_query_node_types("The cat was chased by Sarah.", "desc:interaction");
    assert!(!interactions.is_empty(), "Should find at least one interaction");
    let agent_texts = run_query("The cat was chased by Sarah.", "desc:interaction | .agent");
    assert!(
        agent_texts.iter().any(|t| t.contains("Sarah")),
        "Passive voice agent should be Sarah, got: {:?}", agent_texts
    );
}

#[test]
fn query_interaction_pipe_agent() {
    if !spacy_available() { return; }
    let texts = run_query(
        "Sarah chased the cat. Bob opened the door.",
        "desc:interaction | .agent",
    );
    assert!(texts.len() >= 2, "Expected at least 2 agents, got: {:?}", texts);
    let joined = texts.join(" ");
    assert!(joined.contains("Sarah") || joined.contains("sarah"), "Should find Sarah: {:?}", texts);
    assert!(joined.contains("Bob") || joined.contains("bob"), "Should find Bob: {:?}", texts);
}

#[test]
fn query_interaction_pipe_verb() {
    if !spacy_available() { return; }
    let texts = run_query(
        "Sarah chased the cat. Bob opened the door.",
        "desc:interaction | .verb",
    );
    let joined = texts.join(" ");
    assert!(joined.contains("chased"), "Should find chased: {:?}", texts);
    assert!(joined.contains("opened"), "Should find opened: {:?}", texts);
}

#[test]
fn query_verb_phrase_voice() {
    if !spacy_available() { return; }
    let texts = run_query(
        "The cat was chased by Sarah.",
        "desc:verb_phrase | .voice",
    );
    assert!(texts.contains(&"passive".to_string()), "Should find passive voice: {:?}", texts);
}

#[test]
fn tier1_entity_queries_still_work() {
    if !spacy_available() { return; }
    // Regression test: entity queries from Tier 1 must still work
    let types = run_query_node_types("Sarah went to Paris.", "desc:entity");
    assert!(!types.is_empty(), "Entity queries should still work");
    assert!(types.iter().all(|t| t == "entity"));
}

#[test]
fn tier1_token_queries_still_work() {
    if !spacy_available() { return; }
    // Regression test: token queries from Tier 1 must still work
    let types = run_query_node_types("Hello world.", "desc:token");
    assert!(!types.is_empty(), "Token queries should still work");
    assert!(types.iter().all(|t| t == "token"), "Got: {:?}", types);
}
