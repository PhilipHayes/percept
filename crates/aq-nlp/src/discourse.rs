// ── Discourse relation types ─────────────────────────────────────────────────

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DiscourseRelation {
    Elaboration,
    Cause,
    Contrast,
    Concession,
    Evidence,
    Condition,
    Sequence,
    Background,
}

impl fmt::Display for DiscourseRelation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DiscourseRelation::*;
        let s = match self {
            Elaboration => "elaboration",
            Cause => "cause",
            Contrast => "contrast",
            Concession => "concession",
            Evidence => "evidence",
            Condition => "condition",
            Sequence => "sequence",
            Background => "background",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ConnectivePosition {
    SentenceInitial,
    ClauseInitial,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ConnectiveMatch {
    pub connective: String,
    pub relation: DiscourseRelation,
    pub confidence: f32,
    pub position: ConnectivePosition,
}

// ── Static connective lexicons ───────────────────────────────────────────────

use DiscourseRelation::*;

/// Multi-word connectives sorted by length descending (longest match first).
static MULTI_WORD_CONNECTIVES: &[(&str, DiscourseRelation, f32)] = &[
    // Contrast
    ("on the other hand", Contrast, 0.95),
    ("in contrast to this", Contrast, 0.90),
    ("at the same time", Contrast, 0.80),
    ("in spite of this", Concession, 0.90),
    ("in contrast", Contrast, 0.90),
    ("that said", Contrast, 0.80),
    // Cause
    ("as a result of", Cause, 0.95),
    ("due to the fact that", Cause, 0.95),
    ("for this reason", Cause, 0.90),
    ("as a result", Cause, 0.95),
    ("because of this", Cause, 0.90),
    ("as a consequence", Cause, 0.90),
    ("for that reason", Cause, 0.90),
    ("owing to this", Cause, 0.85),
    // Concession
    ("even though", Concession, 0.95),
    ("even so", Concession, 0.85),
    ("despite this", Concession, 0.90),
    ("in any case", Concession, 0.75),
    // Elaboration
    ("in particular", Elaboration, 0.95),
    ("for example", Elaboration, 0.95),
    ("for instance", Elaboration, 0.95),
    ("in addition to", Elaboration, 0.90),
    ("as well as", Elaboration, 0.85),
    ("in addition", Elaboration, 0.90),
    ("more specifically", Elaboration, 0.95),
    ("that is to say", Elaboration, 0.90),
    ("in other words", Elaboration, 0.90),
    ("to be specific", Elaboration, 0.90),
    ("to illustrate", Elaboration, 0.85),
    ("that is", Elaboration, 0.85),
    ("namely", Elaboration, 0.95),
    // Evidence
    ("studies show", Evidence, 0.95),
    ("research shows", Evidence, 0.95),
    ("evidence suggests", Evidence, 0.95),
    ("data shows", Evidence, 0.90),
    ("according to", Evidence, 0.85),
    ("as shown by", Evidence, 0.90),
    ("it follows that", Evidence, 0.85),
    // Condition
    ("provided that", Condition, 0.95),
    ("on the condition that", Condition, 0.95),
    ("in the event that", Condition, 0.90),
    ("as long as", Condition, 0.90),
    ("only if", Condition, 0.90),
    ("given that", Condition, 0.85),
    ("in case", Condition, 0.80),
    // Sequence
    ("following this", Sequence, 0.85),
    ("after that", Sequence, 0.90),
    ("prior to this", Sequence, 0.85),
    ("in the end", Sequence, 0.80),
    ("to begin with", Sequence, 0.85),
    ("first of all", Sequence, 0.90),
    ("last but not least", Sequence, 0.85),
    ("to start with", Sequence, 0.85),
    ("as a final step", Sequence, 0.90),
    ("in turn", Sequence, 0.85),
    ("next up", Sequence, 0.85),
    // Background
    ("in the past", Background, 0.90),
    ("historically speaking", Background, 0.90),
    ("at that time", Background, 0.85),
    ("up until now", Background, 0.85),
    ("until recently", Background, 0.90),
    ("in recent years", Background, 0.90),
    ("over the years", Background, 0.85),
    ("for many years", Background, 0.85),
    ("in the context of", Background, 0.80),
    ("traditionally", Background, 0.80),
    ("previously known as", Background, 0.90),
    // Elaboration / summary
    ("in summary", Elaboration, 0.90),
    ("to sum up", Elaboration, 0.90),
    ("as noted above", Elaboration, 0.85),
    ("in the meantime", Sequence, 0.85),
    ("at this point", Sequence, 0.80),
    ("as previously mentioned", Background, 0.85),
];

/// Single-word connectives.
static SINGLE_WORD_CONNECTIVES: &[(&str, DiscourseRelation, f32)] = &[
    // Elaboration
    ("specifically", Elaboration, 0.90),
    ("furthermore", Elaboration, 0.90),
    ("additionally", Elaboration, 0.90),
    ("moreover", Elaboration, 0.90),
    ("besides", Elaboration, 0.80),
    ("also", Elaboration, 0.75),
    ("similarly", Elaboration, 0.80),
    // Cause
    ("therefore", Cause, 0.90),
    ("thus", Cause, 0.85),
    ("hence", Cause, 0.85),
    ("because", Cause, 0.90),
    ("consequently", Cause, 0.90),
    ("accordingly", Cause, 0.85),
    // Contrast
    ("however", Contrast, 0.95),
    ("nevertheless", Contrast, 0.90),
    ("nonetheless", Contrast, 0.90),
    ("conversely", Contrast, 0.90),
    ("instead", Contrast, 0.80),
    ("alternatively", Contrast, 0.80),
    ("otherwise", Contrast, 0.80),
    // Concession
    ("although", Concession, 0.90),
    ("though", Concession, 0.85),
    ("admittedly", Concession, 0.85),
    ("granted", Concession, 0.80),
    // Evidence
    ("notably", Evidence, 0.80),
    ("clearly", Evidence, 0.75),
    ("indeed", Evidence, 0.80),
    ("evidently", Evidence, 0.85),
    ("obviously", Evidence, 0.75),
    // Condition
    ("if", Condition, 0.85),
    ("unless", Condition, 0.90),
    ("provided", Condition, 0.85),
    ("supposing", Condition, 0.85),
    // Sequence
    ("first", Sequence, 0.85),
    ("second", Sequence, 0.85),
    ("third", Sequence, 0.85),
    ("finally", Sequence, 0.90),
    ("subsequently", Sequence, 0.90),
    ("afterward", Sequence, 0.85),
    ("afterwards", Sequence, 0.85),
    ("next", Sequence, 0.80),
    ("then", Sequence, 0.75),
    ("lastly", Sequence, 0.90),
    // Background
    ("previously", Background, 0.85),
    ("originally", Background, 0.85),
    ("initially", Background, 0.80),
    ("formerly", Background, 0.85),
    ("historically", Background, 0.85),
    // Ambiguous / lower confidence
    ("since", Cause, 0.70),
    ("while", Contrast, 0.65),
    ("so", Cause, 0.75),
    ("still", Contrast, 0.65),
    ("yet", Contrast, 0.80),
    ("once", Sequence, 0.70),
    // Additional
    ("ultimately", Sequence, 0.85),
    ("naturally", Evidence, 0.75),
    ("regardless", Concession, 0.80),
    ("meanwhile", Sequence, 0.85),
    ("likewise", Elaboration, 0.85),
    ("notwithstanding", Concession, 0.85),
];

// ── Lookup functions ─────────────────────────────────────────────────────────

#[allow(dead_code)]
pub(crate) fn connective_to_relation(text: &str) -> Option<(DiscourseRelation, f32)> {
    let lower = text.to_lowercase();
    for &(connective, ref relation, confidence) in MULTI_WORD_CONNECTIVES {
        if connective == lower.as_str() {
            return Some((relation.clone(), confidence));
        }
    }
    for &(connective, ref relation, confidence) in SINGLE_WORD_CONNECTIVES {
        if connective == lower.as_str() {
            return Some((relation.clone(), confidence));
        }
    }
    None
}

#[allow(dead_code)]
pub(crate) fn lexicon_size() -> usize {
    MULTI_WORD_CONNECTIVES.len() + SINGLE_WORD_CONNECTIVES.len()
}

// ── Sentence scanning ────────────────────────────────────────────────────────

/// Returns true if the character at `pos` in `text` is a word boundary
/// (end of string, whitespace, or punctuation).
fn is_word_boundary_after(text: &str, pos: usize) -> bool {
    if pos >= text.len() {
        return true;
    }
    let ch = text[pos..].chars().next().unwrap_or(' ');
    ch.is_ascii_whitespace() || ch.is_ascii_punctuation()
}

/// Scan the start of `sentence_text` for a known connective.
/// Tries multi-word connectives first (longest match), then single-word.
pub(crate) fn scan_sentence_connective(sentence_text: &str) -> Option<ConnectiveMatch> {
    if sentence_text.is_empty() {
        return None;
    }

    let lower = sentence_text.to_lowercase();
    let lower = lower.trim_start();

    // Multi-word pass (already sorted longest-first)
    for &(connective, ref relation, confidence) in MULTI_WORD_CONNECTIVES {
        if lower.starts_with(connective) {
            let end = connective.len();
            if is_word_boundary_after(lower, end) {
                return Some(ConnectiveMatch {
                    connective: connective.to_string(),
                    relation: relation.clone(),
                    confidence,
                    position: ConnectivePosition::SentenceInitial,
                });
            }
        }
    }

    // Single-word pass
    for &(connective, ref relation, confidence) in SINGLE_WORD_CONNECTIVES {
        if lower.starts_with(connective) {
            let end = connective.len();
            if is_word_boundary_after(lower, end) {
                return Some(ConnectiveMatch {
                    connective: connective.to_string(),
                    relation: relation.clone(),
                    confidence,
                    position: ConnectivePosition::SentenceInitial,
                });
            }
        }
    }

    None
}

// ── Discourse relation detection ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct SentenceInfo {
    pub text: String,
    pub para_idx: usize,
    pub line: usize, // 1-based line number
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct DiscourseRelationData {
    pub relation: DiscourseRelation,
    pub connective: Option<String>,
    pub confidence: f32,
    pub nucleus_sentence_idx: usize,
    pub satellite_sentence_idx: usize,
    pub nucleus_text: String,
    pub satellite_text: String,
    pub nucleus_line: usize,   // 1-based
    pub satellite_line: usize, // 1-based
    pub nucleus_para_idx: usize,
    pub satellite_para_idx: usize,
}

pub(crate) fn detect_discourse_relations(sentences: &[SentenceInfo]) -> Vec<DiscourseRelationData> {
    if sentences.len() < 2 {
        return Vec::new();
    }
    let mut results = Vec::new();
    for i in 1..sentences.len() {
        if let Some(cm) = scan_sentence_connective(&sentences[i].text) {
            let nucleus = &sentences[i - 1];
            let satellite = &sentences[i];
            results.push(DiscourseRelationData {
                relation: cm.relation,
                connective: Some(cm.connective),
                confidence: cm.confidence,
                nucleus_sentence_idx: i - 1,
                satellite_sentence_idx: i,
                nucleus_text: nucleus.text.clone(),
                satellite_text: satellite.text.clone(),
                nucleus_line: nucleus.line,
                satellite_line: satellite.line,
                nucleus_para_idx: nucleus.para_idx,
                satellite_para_idx: satellite.para_idx,
            });
        }
    }
    results
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Display tests
    #[test]
    fn display_elaboration() {
        assert_eq!(DiscourseRelation::Elaboration.to_string(), "elaboration");
    }
    #[test]
    fn display_cause() {
        assert_eq!(DiscourseRelation::Cause.to_string(), "cause");
    }
    #[test]
    fn display_contrast() {
        assert_eq!(DiscourseRelation::Contrast.to_string(), "contrast");
    }
    #[test]
    fn display_concession() {
        assert_eq!(DiscourseRelation::Concession.to_string(), "concession");
    }
    #[test]
    fn display_evidence() {
        assert_eq!(DiscourseRelation::Evidence.to_string(), "evidence");
    }
    #[test]
    fn display_condition() {
        assert_eq!(DiscourseRelation::Condition.to_string(), "condition");
    }
    #[test]
    fn display_sequence() {
        assert_eq!(DiscourseRelation::Sequence.to_string(), "sequence");
    }
    #[test]
    fn display_background() {
        assert_eq!(DiscourseRelation::Background.to_string(), "background");
    }

    // Lookup tests
    #[test]
    fn lookup_however() {
        let result = connective_to_relation("however");
        assert!(result.is_some());
        let (rel, conf) = result.unwrap();
        assert_eq!(rel, DiscourseRelation::Contrast);
        assert!((conf - 0.95).abs() < 0.01);
    }
    #[test]
    fn lookup_because() {
        let result = connective_to_relation("because");
        assert!(result.is_some());
        let (rel, _) = result.unwrap();
        assert_eq!(rel, DiscourseRelation::Cause);
    }
    #[test]
    fn lookup_therefore() {
        let result = connective_to_relation("therefore");
        assert!(result.is_some());
        let (rel, _) = result.unwrap();
        assert_eq!(rel, DiscourseRelation::Cause);
    }
    #[test]
    fn lookup_ambiguous_since() {
        let result = connective_to_relation("since");
        assert!(result.is_some());
        let (rel, conf) = result.unwrap();
        assert_eq!(rel, DiscourseRelation::Cause);
        assert!((conf - 0.70).abs() < 0.01);
    }
    #[test]
    fn lookup_on_the_other_hand() {
        let result = connective_to_relation("on the other hand");
        assert!(result.is_some());
        let (rel, conf) = result.unwrap();
        assert_eq!(rel, DiscourseRelation::Contrast);
        assert!((conf - 0.95).abs() < 0.01);
    }
    #[test]
    fn lookup_as_a_result() {
        let result = connective_to_relation("as a result");
        assert!(result.is_some());
        let (rel, _) = result.unwrap();
        assert_eq!(rel, DiscourseRelation::Cause);
    }
    #[test]
    fn lookup_unknown_returns_none() {
        assert!(connective_to_relation("xyz_unknown_word").is_none());
    }

    // Scan tests
    #[test]
    fn scan_however_sentence() {
        let m = scan_sentence_connective("However, the results were different.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "however");
        assert_eq!(m.relation, DiscourseRelation::Contrast);
    }
    #[test]
    fn scan_no_connective() {
        let m = scan_sentence_connective("The sky is blue.");
        assert!(m.is_none());
    }
    #[test]
    fn scan_multi_word_priority_on_the_other_hand() {
        let m = scan_sentence_connective("On the other hand, costs increased.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "on the other hand");
        assert_eq!(m.relation, DiscourseRelation::Contrast);
    }
    #[test]
    fn scan_in_particular() {
        let m = scan_sentence_connective("In particular, the following items stand out.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "in particular");
        assert_eq!(m.relation, DiscourseRelation::Elaboration);
    }
    #[test]
    fn scan_studies_show() {
        let m = scan_sentence_connective("Studies show that exercise improves mood.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "studies show");
        assert_eq!(m.relation, DiscourseRelation::Evidence);
    }
    #[test]
    fn scan_as_a_result() {
        let m = scan_sentence_connective("As a result, the project was delayed.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "as a result");
        assert_eq!(m.relation, DiscourseRelation::Cause);
    }
    #[test]
    fn scan_ambiguous_confidence() {
        let m = scan_sentence_connective("Since the beginning, things changed.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "since");
        assert!((m.confidence - 0.70).abs() < 0.01);
    }
    #[test]
    fn scan_empty_string() {
        assert!(scan_sentence_connective("").is_none());
    }
    #[test]
    fn scan_furthermore() {
        let m = scan_sentence_connective("Furthermore, the data confirms the hypothesis.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "furthermore");
        assert_eq!(m.relation, DiscourseRelation::Elaboration);
    }
    #[test]
    fn scan_if() {
        let m = scan_sentence_connective("If the condition holds, proceed.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "if");
        assert_eq!(m.relation, DiscourseRelation::Condition);
    }
    #[test]
    fn scan_first() {
        let m = scan_sentence_connective("First, we need to gather requirements.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "first");
        assert_eq!(m.relation, DiscourseRelation::Sequence);
    }
    #[test]
    fn scan_previously() {
        let m = scan_sentence_connective("Previously, this method was used.");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.connective, "previously");
        assert_eq!(m.relation, DiscourseRelation::Background);
    }

    // Lexicon count test
    #[test]
    fn lexicon_count_at_least_130() {
        assert!(lexicon_size() >= 130, "lexicon_size() = {}", lexicon_size());
    }

    // detect_discourse_relations tests

    fn make_sentence_info(text: &str, para_idx: usize, line: usize) -> SentenceInfo {
        SentenceInfo {
            text: text.to_string(),
            para_idx,
            line,
        }
    }

    #[test]
    fn detect_therefore_cause() {
        let sentences = vec![
            make_sentence_info("The server crashed.", 0, 1),
            make_sentence_info("Therefore, users lost their sessions.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Cause);
        assert_eq!(rels[0].nucleus_sentence_idx, 0);
        assert_eq!(rels[0].satellite_sentence_idx, 1);
    }

    #[test]
    fn detect_however_contrast() {
        let sentences = vec![
            make_sentence_info("The system is fast.", 0, 1),
            make_sentence_info("However, the cost is high.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Contrast);
    }

    #[test]
    fn detect_specifically_elaboration() {
        let sentences = vec![
            make_sentence_info("Testing revealed three bugs.", 0, 1),
            make_sentence_info("Specifically, the login failed.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Elaboration);
    }

    #[test]
    fn detect_no_connective() {
        let sentences = vec![
            make_sentence_info("The server crashed.", 0, 1),
            make_sentence_info("Users lost their sessions.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert!(rels.is_empty());
    }

    #[test]
    fn detect_three_sentences_middle() {
        let sentences = vec![
            make_sentence_info("The team deployed.", 0, 1),
            make_sentence_info("The error rate dropped.", 0, 2),
            make_sentence_info("Consequently, uptime improved.", 0, 3),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Cause);
        assert_eq!(rels[0].nucleus_sentence_idx, 1);
        assert_eq!(rels[0].satellite_sentence_idx, 2);
    }

    #[test]
    fn detect_multiple_relations() {
        let sentences = vec![
            make_sentence_info("The server crashed.", 0, 1),
            make_sentence_info("Therefore, users lost data.", 0, 2),
            make_sentence_info("However, the backup recovered.", 0, 3),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].relation, DiscourseRelation::Cause);
        assert_eq!(rels[1].relation, DiscourseRelation::Contrast);
    }

    #[test]
    fn detect_evidence() {
        let sentences = vec![
            make_sentence_info("The algorithm is faster.", 0, 1),
            make_sentence_info("Studies show latency dropped 50%.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Evidence);
    }

    #[test]
    fn detect_condition() {
        let sentences = vec![
            make_sentence_info("Check the logs.", 0, 1),
            make_sentence_info("If errors exist, restart the service.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Condition);
    }

    #[test]
    fn detect_sequence() {
        let sentences = vec![
            make_sentence_info("First, gather requirements.", 0, 1),
            make_sentence_info("Then, design.", 0, 2),
            make_sentence_info("Finally, implement.", 0, 3),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].relation, DiscourseRelation::Sequence);
        assert_eq!(rels[1].relation, DiscourseRelation::Sequence);
    }

    #[test]
    fn detect_concession() {
        let sentences = vec![
            make_sentence_info("The approach is expensive.", 0, 1),
            make_sentence_info("Although it works, we cannot afford it.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Concession);
    }

    #[test]
    fn detect_background() {
        let sentences = vec![
            make_sentence_info("The project launched in 2024.", 0, 1),
            make_sentence_info("Previously, research took two years.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Background);
    }

    #[test]
    fn detect_single_sentence() {
        let sentences = vec![make_sentence_info("The server is running.", 0, 1)];
        let rels = detect_discourse_relations(&sentences);
        assert!(rels.is_empty());
    }

    #[test]
    fn detect_ambiguous_since_confidence() {
        let sentences = vec![
            make_sentence_info("The server crashed.", 0, 1),
            make_sentence_info("Since then, we monitored.", 0, 2),
        ];
        let rels = detect_discourse_relations(&sentences);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation, DiscourseRelation::Cause);
        assert!((rels[0].confidence - 0.70).abs() < 0.01);
    }
}
