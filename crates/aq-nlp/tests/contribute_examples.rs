//! End-to-end integration tests using the original contribute project
//! (Swift, 2019) example sentences. These are the sentences that motivated
//! the entire nq tool — passive voice parsing that NSLinguisticTagger
//! couldn't handle.

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

fn parse_text(text: &str) -> aq_core::OwnedNode {
    let backend = NlpBackend;
    backend.parse(text, "english", None).expect("parse failed")
}

fn query_interaction_texts(text: &str) -> Vec<String> {
    let tree = parse_text(text);
    let tokens = lex("desc:interaction").expect("lex");
    let ast = parse(&tokens).expect("parse");
    let results = eval(&ast, &tree).expect("eval");
    results.iter().map(|r| match r {
        EvalResult::Node(n) => n.text().unwrap_or("").to_string(),
        EvalResult::Value(v) => v.to_string(),
    }).collect()
}

fn query_agents(text: &str) -> Vec<String> {
    let tree = parse_text(text);
    let tokens = lex("desc:interaction | .agent").expect("lex");
    let ast = parse(&tokens).expect("parse");
    let results = eval(&ast, &tree).expect("eval");
    results.iter().map(|r| match r {
        EvalResult::Node(n) => n.text().unwrap_or("").to_string(),
        EvalResult::Value(v) => v.as_str().unwrap_or("").to_string(),
    }).collect()
}

fn query_voice(text: &str) -> Vec<String> {
    let tree = parse_text(text);
    let tokens = lex("desc:interaction | .voice").expect("lex");
    let ast = parse(&tokens).expect("parse");
    let results = eval(&ast, &tree).expect("eval");
    results.iter().map(|r| match r {
        EvalResult::Node(n) => n.text().unwrap_or("").to_string(),
        EvalResult::Value(v) => v.as_str().unwrap_or("").to_string(),
    }).collect()
}

/// THE canonical test — this is the exact sentence that broke the contribute project.
/// NSLinguisticTagger saw David=Noun, punched=Verb, Jane=Noun — no way to
/// distinguish who punched whom. spaCy gives us nsubjpass/agent deps to resolve it.
#[test]
fn contribute_david_punched_by_jane() {
    if !spacy_available() { return; }
    let text = "David, punched by Jane, with so much force that it hurt him.";
    let interactions = query_interaction_texts(text);
    assert!(!interactions.is_empty(), "Should find interactions: {:?}", interactions);
    
    // The "punched" interaction should have Jane as agent
    let agents = query_agents(text);
    let voices = query_voice(text);
    
    // Find the passive interaction (punched)
    assert!(
        agents.iter().any(|a| a.contains("Jane")),
        "Jane should be the agent of 'punched': agents={:?}", agents
    );
    assert!(
        voices.contains(&"passive".to_string()),
        "Should detect passive voice: {:?}", voices
    );
}

#[test]
fn contribute_came_back_to_life() {
    if !spacy_available() { return; }
    let text = "Jane came back to life with the help of Bob Markey.";
    let interactions = query_interaction_texts(text);
    assert!(!interactions.is_empty(), "Should find at least one interaction");
    
    let agents = query_agents(text);
    assert!(
        agents.iter().any(|a| a.contains("Jane")),
        "Jane should be the agent: {:?}", agents
    );
}

#[test]
fn contribute_ran_away() {
    if !spacy_available() { return; }
    let text = "After Jane came back to life she ran away to her home in Azure.";
    let interactions = query_interaction_texts(text);
    assert!(
        interactions.len() >= 2,
        "Should find at least 2 interactions (came, ran): {:?}", interactions
    );
}

#[test]
fn contribute_joey_battled() {
    if !spacy_available() { return; }
    let text = "While in and around Lavender Town, Joey battled Kristy and won.";
    let interactions = query_interaction_texts(text);
    assert!(!interactions.is_empty(), "Should find interactions");
    
    let agents = query_agents(text);
    assert!(
        agents.iter().any(|a| a.contains("Joey")),
        "Joey should be an agent: {:?}", agents
    );
}

#[test]
fn contribute_multi_sentence_narrative() {
    if !spacy_available() { return; }
    let text = "Jane came back to life with the help of Bob Markey. \
                After Jane came back to life she ran away to her home in Azure. \
                While in and around Lavender Town, Joey battled Kristy and won.";
    let interactions = query_interaction_texts(text);
    assert!(
        interactions.len() >= 3,
        "Should find at least 3 interactions in narrative: {:?}", interactions
    );
}

#[test]
fn compound_subject_enters() {
    if !spacy_available() { return; }
    let text = "Sarah and Tom entered the cave.";
    let agents = query_agents(text);
    let joined = agents.join(" ");
    assert!(
        joined.contains("Sarah") && joined.contains("Tom"),
        "Both Sarah and Tom should appear as agents: {:?}", agents
    );
}

#[test]
fn copula_no_panic() {
    if !spacy_available() { return; }
    // Copula sentences should not panic — may produce 0 or 1 interactions
    let text = "The sky was blue.";
    let _interactions = query_interaction_texts(text);
    // Just assert no panic
}

#[test]
fn long_narrative_no_panic() {
    if !spacy_available() { return; }
    let text = "David, punched by Jane, with so much force that it hurt him. \
                He used The Grand Ghost Soul to finish her off. \
                Jane came back to life with the help of Bob Markey. \
                After Jane came back to life she ran away to her home in Azure. \
                While in and around Lavender Town, Joey battled Kristy and won. \
                Sarah opened the ancient door with a golden key. \
                The treasure was discovered by the entire team. \
                Bob gave Sarah the map. \
                They celebrated their victory. \
                The sun set over the mountains.";
    let interactions = query_interaction_texts(text);
    assert!(
        interactions.len() >= 5,
        "Long narrative should produce 5+ interactions: got {} — {:?}",
        interactions.len(), interactions
    );
}
