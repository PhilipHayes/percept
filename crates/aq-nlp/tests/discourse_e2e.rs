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
fn e2e_technical_report() {
    if !spacy_available() {
        return;
    }
    let text = "The team deployed version 2.0 on Monday. Testing revealed three critical bugs. Specifically, the authentication module failed, the cache layer overflowed, and the API gateway timed out.\n\nTherefore, the rollback was initiated within two hours. However, some data loss occurred during the transition. Studies show that approximately 3% of user sessions were affected.\n\nPreviously, version 1.9 had been running stable for six months. If similar issues occur in the future, the team will implement automated rollback. Unless the root cause is identified, version 2.0 will not be redeployed.";
    let types = run_query_node_types(text, "desc:discourse");
    // Should find multiple discourse relations (at least 5)
    assert!(
        types.len() >= 5,
        "Expected at least 5 discourse relations in technical report, got {}: {:?}",
        types.len(),
        types
    );
    assert!(types.iter().all(|t| t == "discourse"));
}

#[test]
fn e2e_argument_essay() {
    if !spacy_available() {
        return;
    }
    let text = "Remote work increases productivity. For instance, studies show that developers complete 20% more tasks at home. However, collaboration suffers without in-person interaction. Although video calls help, they cannot replace spontaneous hallway conversations.\n\nConsequently, hybrid models are gaining popularity. First, employees work from home three days per week. Then, they come to the office for collaborative sessions. Finally, team retrospectives happen monthly in person.";
    let types = run_query_node_types(text, "desc:discourse");
    assert!(
        types.len() >= 5,
        "Expected at least 5 discourse relations in essay, got {}: {:?}",
        types.len(),
        types
    );
}

#[test]
fn e2e_no_connectives() {
    if !spacy_available() {
        return;
    }
    let types = run_query_node_types(
        "The cat sat on the mat. The dog slept in the sun. Birds sang in the trees.",
        "desc:discourse",
    );
    assert!(
        types.is_empty(),
        "Expected no discourse for connective-free text, got: {:?}",
        types
    );
}

#[test]
fn e2e_dense_connectives() {
    if !spacy_available() {
        return;
    }
    let text = "The project started. Then, requirements were gathered. Subsequently, design began. However, delays occurred. Therefore, the deadline was extended. Finally, the product launched.";
    let types = run_query_node_types(text, "desc:discourse");
    assert_eq!(
        types.len(),
        5,
        "Expected 5 discourse relations in dense connective text, got {}: {:?}",
        types.len(),
        types
    );
}

#[test]
fn e2e_discourse_plus_entities_plus_roles() {
    if !spacy_available() {
        return;
    }
    let text = "Sarah heard a noise. Therefore, she investigated. However, she found nothing.";
    let discourse = run_query_node_types(text, "desc:discourse");
    assert_eq!(
        discourse.len(),
        2,
        "Expected 2 discourse relations, got: {:?}",
        discourse
    );
    let entities = run_query_node_types(text, "desc:entity");
    assert!(!entities.is_empty(), "Expected entities");
    let interactions = run_query_node_types(text, "desc:interaction");
    assert!(!interactions.is_empty(), "Expected interactions");
}

#[test]
fn e2e_cross_paragraph_multi() {
    if !spacy_available() {
        return;
    }
    let text = "The initial deployment succeeded. Performance metrics looked good.\n\nHowever, after 48 hours, memory leaks appeared. Consequently, the team rolled back.\n\nPreviously, similar issues had been reported in staging.";
    let types = run_query_node_types(text, "desc:discourse");
    assert!(
        types.len() >= 3,
        "Expected at least 3 discourse relations, got: {:?}",
        types
    );
    // Check cross-paragraph scope exists
    let scopes = run_query(text, "desc:discourse | .scope");
    assert!(
        scopes.iter().any(|s| s == "cross_paragraph"),
        "Expected at least one cross_paragraph relation, got: {:?}",
        scopes
    );
}
