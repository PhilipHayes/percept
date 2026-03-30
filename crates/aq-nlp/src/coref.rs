use crate::spacy::{SpacySentence, SpacyToken as SpacyTokenData};
use crate::tree::{collect_span_text, normalize_dep, offset_to_line};
use std::collections::HashMap;

// ── Co-reference types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CorefType {
    Appositive,
    SameSentencePronoun,
    CrossSentencePronoun,
    PossessivePronoun,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CoreferenceData {
    pub referent: String,
    pub canonical: String,
    pub coref_type: CorefType,
    pub confidence: f32,
    pub sentence_idx: usize,
    pub token_idx: usize,
    pub source_line: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CoreferenceChain {
    pub canonical: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub mentions: Vec<CoreferenceData>,
    pub total_mention_count: usize,
}

// ── Appositive extraction ────────────────────────────────────────────────────

pub(crate) fn extract_appositives_from_sentence(
    sentence: &SpacySentence,
    sentence_idx: usize,
    line_starts: &[usize],
) -> Vec<CoreferenceData> {
    let tokens = &sentence.tokens;
    let mut results = Vec::new();

    for (token_index, token) in tokens.iter().enumerate() {
        if normalize_dep(&token.dep) != "appos" {
            continue;
        }

        let head_index = token.head;
        if head_index >= tokens.len() {
            continue;
        }
        let head = &tokens[head_index];

        // Head must be a named entity or proper noun (or appositive token is)
        let head_is_entity = !head.ent_type.is_empty() || head.pos == "PROPN";
        let appos_is_entity = !token.ent_type.is_empty() || token.pos == "PROPN";

        if !head_is_entity && !appos_is_entity {
            continue;
        }

        let appos_span = collect_span_text(token_index, tokens);
        let head_span = collect_span_text(head_index, tokens);

        // Reverse appositive: prefer PROPN / named-entity token as canonical
        let (canonical, referent) = if appos_is_entity && !head_is_entity {
            (appos_span, head_span)
        } else {
            (head_span, appos_span)
        };

        let source_line = offset_to_line(token.idx, line_starts);

        results.push(CoreferenceData {
            referent,
            canonical,
            coref_type: CorefType::Appositive,
            confidence: 0.95,
            sentence_idx,
            token_idx: token_index,
            source_line,
        });
    }

    results
}

// ── Chain aggregation ────────────────────────────────────────────────────────

pub(crate) fn build_coreference_chains(
    all_corefs: &[CoreferenceData],
    entity_type_map: &HashMap<String, String>,
) -> Vec<CoreferenceChain> {
    // Group by lowercase canonical
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&CoreferenceData>> = HashMap::new();
    let mut first_case: HashMap<String, String> = HashMap::new();

    for coref in all_corefs {
        let key = coref.canonical.to_lowercase();
        if !groups.contains_key(&key) {
            order.push(key.clone());
            first_case.insert(key.clone(), coref.canonical.clone());
        }
        groups.entry(key).or_default().push(coref);
    }

    let mut chains: Vec<CoreferenceChain> = order
        .into_iter()
        .map(|key| {
            let mentions_refs = &groups[&key];
            let canonical = first_case[&key].clone();
            let entity_type = entity_type_map.get(&key).cloned().unwrap_or_default();

            let mut aliases: Vec<String> = Vec::new();
            for m in mentions_refs.iter() {
                if !aliases.contains(&m.referent) {
                    aliases.push(m.referent.clone());
                }
            }

            let mentions: Vec<CoreferenceData> =
                mentions_refs.iter().map(|m| (*m).clone()).collect();
            let total_mention_count = mentions.len();

            CoreferenceChain {
                canonical,
                entity_type,
                aliases,
                mentions,
                total_mention_count,
            }
        })
        .collect();

    chains.sort_by(|a, b| a.canonical.cmp(&b.canonical));
    chains
}

// ── Gender classification ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Gender {
    Female,
    Male,
    Neutral,
    Plural,
}

pub(crate) fn pronoun_gender(text: &str) -> Option<Gender> {
    match text.to_lowercase().as_str() {
        "she" | "her" | "hers" | "herself" => Some(Gender::Female),
        "he" | "him" | "his" | "himself" => Some(Gender::Male),
        "it" | "its" | "itself" => Some(Gender::Neutral),
        "they" | "them" | "their" | "theirs" | "themselves" => Some(Gender::Plural),
        _ => None,
    }
}

pub(crate) fn is_pronoun(token: &SpacyTokenData) -> bool {
    token.pos == "PRON"
}

pub(crate) fn is_reflexive(text: &str) -> bool {
    matches!(
        text.to_lowercase().as_str(),
        "herself"
            | "himself"
            | "itself"
            | "themselves"
            | "ourselves"
            | "myself"
            | "yourself"
            | "yourselves"
    )
}

pub(crate) fn is_expletive_it(token: &SpacyTokenData, tokens: &[SpacyTokenData]) -> bool {
    if token.text.to_lowercase() != "it" {
        return false;
    }
    if normalize_dep(&token.dep) == "expl" {
        return true;
    }
    let head_idx = token.head;
    if head_idx < tokens.len() {
        let head = &tokens[head_idx];
        let weather_verbs = ["rain", "snow", "hail", "seem", "appear"];
        if weather_verbs.contains(&head.lemma.as_str()) {
            return true;
        }
    }
    let has_non_pronoun_entity = tokens
        .iter()
        .any(|t| !t.ent_type.is_empty() && t.pos != "PRON");
    !has_non_pronoun_entity
}

pub(crate) fn resolve_same_sentence_pronouns(
    sentence: &SpacySentence,
    sentence_idx: usize,
    line_starts: &[usize],
) -> Vec<CoreferenceData> {
    let tokens = &sentence.tokens;
    let mut results = Vec::new();

    for (token_index, token) in tokens.iter().enumerate() {
        if !is_pronoun(token) {
            continue;
        }
        if is_reflexive(&token.text) {
            continue;
        }
        if is_expletive_it(token, tokens) {
            continue;
        }

        let dep = normalize_dep(&token.dep);
        if dep == "nsubj" || dep == "nsubjpass" {
            let first_entity_idx = tokens
                .iter()
                .enumerate()
                .find(|(_, t)| (!t.ent_type.is_empty() || t.pos == "PROPN") && t.pos != "PRON")
                .map(|(i, _)| i);
            match first_entity_idx {
                Some(first_ent) if token_index <= first_ent => continue,
                None => continue,
                _ => {}
            }
        }

        let gender = match pronoun_gender(&token.text.to_lowercase()) {
            Some(g) => g,
            None => continue,
        };

        let mut candidates: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(i, t)| *i < token_index && (!t.ent_type.is_empty() || t.pos == "PROPN"))
            .map(|(i, _)| i)
            .collect();

        match gender {
            Gender::Female | Gender::Male => {
                candidates.retain(|&i| {
                    let t = &tokens[i];
                    t.ent_type == "PERSON" || t.pos == "PROPN"
                });
            }
            Gender::Neutral => {
                candidates.retain(|&i| tokens[i].ent_type != "PERSON");
                if candidates.is_empty() {
                    if let Some((noun_idx, _)) = tokens
                        .iter()
                        .enumerate()
                        .rfind(|(i, t)| *i < token_index && t.pos == "NOUN")
                    {
                        candidates.push(noun_idx);
                    }
                }
            }
            Gender::Plural => {
                candidates.retain(|&i| {
                    let t = &tokens[i];
                    t.ent_type == "PERSON" || t.pos == "PROPN"
                });
            }
        }

        if candidates.is_empty() {
            continue;
        }

        let (candidate_idx, confidence) = if candidates.len() == 1 {
            (candidates[0], 0.90_f32)
        } else {
            (*candidates.iter().max().unwrap(), 0.65_f32)
        };

        let coref_type = if normalize_dep(&token.dep) == "poss" {
            CorefType::PossessivePronoun
        } else {
            CorefType::SameSentencePronoun
        };

        let canonical = collect_span_text(candidate_idx, tokens);
        let referent = token.text.to_lowercase();
        let source_line = offset_to_line(token.idx, line_starts);

        results.push(CoreferenceData {
            referent,
            canonical,
            coref_type,
            confidence,
            sentence_idx,
            token_idx: token_index,
            source_line,
        });
    }

    results
}

// ── Phase 3: Cross-sentence pronoun resolution ──────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_cross_sentence_pronouns(
    sentence: &SpacySentence,
    sentence_idx: usize,
    prev_sentence: Option<&SpacySentence>,
    same_paragraph: bool,
    already_resolved: &[usize],
    entity_gender_map: &HashMap<String, Gender>,
    topic_entities: &HashMap<Gender, String>,
    line_starts: &[usize],
) -> Vec<CoreferenceData> {
    let tokens = &sentence.tokens;
    let mut results = Vec::new();

    let prev_tokens = match prev_sentence {
        Some(ps) if same_paragraph => &ps.tokens,
        _ => return results,
    };

    // Find prev sentence's subject entity: nsubj/nsubjpass whose head is ROOT
    let prev_subject_idx: Option<usize> = prev_tokens
        .iter()
        .enumerate()
        .find(|(_, t)| {
            let d = normalize_dep(&t.dep);
            (d == "nsubj" || d == "nsubjpass") && {
                let h = t.head;
                h < prev_tokens.len()
                    && normalize_dep(&prev_tokens[h].dep).eq_ignore_ascii_case("root")
            }
        })
        .map(|(i, _)| i);

    for (token_index, token) in tokens.iter().enumerate() {
        if !is_pronoun(token) {
            continue;
        }
        if already_resolved.contains(&token_index) {
            continue;
        }
        if is_reflexive(&token.text) {
            continue;
        }
        if is_expletive_it(token, tokens) {
            continue;
        }

        let gender = match pronoun_gender(&token.text) {
            Some(g) => g,
            None => continue,
        };

        // Build candidates from previous sentence's entity/PROPN tokens
        let mut candidates: Vec<usize> = prev_tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.ent_type.is_empty() || t.pos == "PROPN")
            .map(|(i, _)| i)
            .collect();

        // Filter by gender compatibility
        match gender {
            Gender::Female | Gender::Male => {
                candidates.retain(|&i| {
                    let t = &prev_tokens[i];
                    if t.ent_type != "PERSON" && t.pos != "PROPN" {
                        return false;
                    }
                    // Exclude candidates whose known gender mismatches the pronoun
                    let key = t.text.to_lowercase();
                    match entity_gender_map.get(&key) {
                        Some(&known_gender) => known_gender == gender,
                        None => true,
                    }
                });
            }
            Gender::Neutral => {
                candidates.retain(|&i| prev_tokens[i].ent_type != "PERSON");
                if candidates.is_empty() {
                    if let Some((noun_idx, _)) = prev_tokens
                        .iter()
                        .enumerate()
                        .rfind(|(_, t)| t.pos == "NOUN")
                    {
                        candidates.push(noun_idx);
                    }
                }
            }
            Gender::Plural => {
                candidates.retain(|&i| {
                    let t = &prev_tokens[i];
                    t.ent_type == "PERSON" || t.pos == "PROPN"
                });
            }
        }

        let (canonical, confidence) = if candidates.is_empty() {
            // Fall back to topic_entities
            match topic_entities.get(&gender) {
                Some(name) => (name.clone(), 0.70_f32),
                None => continue,
            }
        } else if candidates.len() == 1 {
            (collect_span_text(candidates[0], prev_tokens), 0.80_f32)
        } else {
            // Multiple candidates: prefer prev sentence's subject
            match prev_subject_idx {
                Some(subj_idx) if candidates.contains(&subj_idx) => {
                    (collect_span_text(subj_idx, prev_tokens), 0.65_f32)
                }
                _ => {
                    // Pick nearest to end of prev sentence (highest index)
                    let best = *candidates.iter().max().unwrap();
                    (collect_span_text(best, prev_tokens), 0.50_f32)
                }
            }
        };

        let coref_type = if normalize_dep(&token.dep) == "poss" {
            CorefType::PossessivePronoun
        } else {
            CorefType::CrossSentencePronoun
        };

        let referent = token.text.clone(); // preserve original case
        let source_line = offset_to_line(token.idx, line_starts);

        results.push(CoreferenceData {
            referent,
            canonical,
            coref_type,
            confidence,
            sentence_idx,
            token_idx: token_index,
            source_line,
        });
    }

    results
}

pub(crate) fn update_gender_map(
    resolutions: &[CoreferenceData],
    entity_gender_map: &mut HashMap<String, Gender>,
) {
    for res in resolutions {
        if let Some(gender) = pronoun_gender(&res.referent) {
            entity_gender_map.insert(res.canonical.to_lowercase(), gender);
        }
    }
}

pub(crate) fn update_topic_entities(
    sentence: &SpacySentence,
    resolutions: &[CoreferenceData],
    entity_gender_map: &HashMap<String, Gender>,
    topic_entities: &mut HashMap<Gender, String>,
) {
    let tokens = &sentence.tokens;

    // If sentence subject is a named entity/PROPN, update topic for its known gender
    let subject = tokens.iter().find(|t| {
        let d = normalize_dep(&t.dep);
        (d == "nsubj" || d == "nsubjpass") && {
            let h = t.head;
            h < tokens.len() && normalize_dep(&tokens[h].dep).eq_ignore_ascii_case("root")
        }
    });

    if let Some(subj) = subject {
        if !subj.ent_type.is_empty() || subj.pos == "PROPN" {
            let key = subj.text.to_lowercase();
            if let Some(&gender) = entity_gender_map.get(&key) {
                topic_entities.insert(gender, subj.text.clone());
            }
        }
    }

    // Resolved entity becomes/stays topic entity for that gender
    for res in resolutions {
        if let Some(gender) = pronoun_gender(&res.referent) {
            topic_entities.insert(gender, res.canonical.clone());
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spacy::{SpacySentence, SpacyToken as SpacyTokenData};

    fn make_token(
        text: &str,
        pos: &str,
        dep: &str,
        head: usize,
        ent_type: &str,
        idx: usize,
    ) -> SpacyTokenData {
        SpacyTokenData {
            text: text.to_string(),
            lemma: text.to_lowercase(),
            pos: pos.to_string(),
            tag: pos.to_string(),
            dep: dep.to_string(),
            head,
            ent_type: ent_type.to_string(),
            ent_iob: if ent_type.is_empty() {
                "O".to_string()
            } else {
                "B".to_string()
            },
            idx,
        }
    }

    fn make_sentence(tokens: Vec<SpacyTokenData>) -> SpacySentence {
        SpacySentence {
            text: String::new(),
            start: 0,
            end: 0,
            tokens,
        }
    }

    // "Sarah, the detective, arrived at the scene."
    // Indices: 0=Sarah, 1=',', 2=the, 3=detective, 4=',', 5=arrived, 6=at, 7=the, 8=scene, 9='.'
    #[test]
    fn appositive_simple() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 5, "PERSON", 0),
            make_token(",", "PUNCT", "punct", 0, "", 5),
            make_token("the", "DET", "det", 3, "", 7),
            make_token("detective", "NOUN", "appos", 0, "", 11),
            make_token(",", "PUNCT", "punct", 0, "", 20),
            make_token("arrived", "VERB", "ROOT", 5, "", 22),
            make_token("at", "ADP", "prep", 5, "", 30),
            make_token("the", "DET", "det", 8, "", 33),
            make_token("scene", "NOUN", "pobj", 6, "", 37),
            make_token(".", "PUNCT", "punct", 5, "", 42),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = extract_appositives_from_sentence(&sentence, 0, &line_starts);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "the detective");
        assert_eq!(results[0].canonical, "Sarah");
        assert!((results[0].confidence - 0.95).abs() < f32::EPSILON);
        assert_eq!(results[0].coref_type, CorefType::Appositive);
    }

    // "Bob Markey, the healer, came to help."
    // Indices: 0=Bob, 1=Markey, 2=',', 3=the, 4=healer, 5=came, ...
    #[test]
    fn appositive_compound_name() {
        let tokens = vec![
            make_token("Bob", "PROPN", "compound", 1, "PERSON", 0),
            make_token("Markey", "PROPN", "nsubj", 5, "PERSON", 4),
            make_token(",", "PUNCT", "punct", 1, "", 10),
            make_token("the", "DET", "det", 4, "", 12),
            make_token("healer", "NOUN", "appos", 1, "", 16),
            make_token("came", "VERB", "ROOT", 5, "", 23),
            make_token("to", "PART", "aux", 7, "", 28),
            make_token("help", "VERB", "xcomp", 5, "", 31),
            make_token(".", "PUNCT", "punct", 5, "", 35),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = extract_appositives_from_sentence(&sentence, 0, &line_starts);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "the healer");
        assert_eq!(results[0].canonical, "Bob Markey");
    }

    // "Sarah chased the cat." — no appositives
    #[test]
    fn no_appositive() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("chased", "VERB", "ROOT", 1, "", 6),
            make_token("the", "DET", "det", 3, "", 13),
            make_token("cat", "NOUN", "dobj", 1, "", 17),
            make_token(".", "PUNCT", "punct", 1, "", 20),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = extract_appositives_from_sentence(&sentence, 0, &line_starts);
        assert!(results.is_empty());
    }

    // "Sarah, the detective, our lead investigator, arrived."
    // Both "detective" and "investigator" have dep=appos, head=0 (Sarah)
    #[test]
    fn appositive_chained() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 7, "PERSON", 0),
            make_token(",", "PUNCT", "punct", 0, "", 5),
            make_token("the", "DET", "det", 3, "", 7),
            make_token("detective", "NOUN", "appos", 0, "", 11),
            make_token(",", "PUNCT", "punct", 0, "", 20),
            make_token("our", "DET", "poss", 7, "", 22),
            make_token("lead", "NOUN", "compound", 7, "", 26),
            make_token("investigator", "NOUN", "appos", 0, "", 31),
            make_token(",", "PUNCT", "punct", 0, "", 43),
            make_token("arrived", "VERB", "ROOT", 9, "", 45),
            make_token(".", "PUNCT", "punct", 9, "", 52),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = extract_appositives_from_sentence(&sentence, 0, &line_starts);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.canonical == "Sarah"));
        let referents: Vec<&str> = results.iter().map(|r| r.referent.as_str()).collect();
        assert!(referents.contains(&"the detective"));
        assert!(referents.contains(&"our lead investigator"));
    }

    // Chain aggregation
    #[test]
    fn chain_aggregation() {
        let corefs = vec![
            CoreferenceData {
                referent: "the detective".to_string(),
                canonical: "Sarah".to_string(),
                coref_type: CorefType::Appositive,
                confidence: 0.95,
                sentence_idx: 0,
                token_idx: 3,
                source_line: 1,
            },
            CoreferenceData {
                referent: "she".to_string(),
                canonical: "Sarah".to_string(),
                coref_type: CorefType::SameSentencePronoun,
                confidence: 0.80,
                sentence_idx: 1,
                token_idx: 0,
                source_line: 2,
            },
            CoreferenceData {
                referent: "she".to_string(),
                canonical: "Sarah".to_string(),
                coref_type: CorefType::SameSentencePronoun,
                confidence: 0.80,
                sentence_idx: 2,
                token_idx: 0,
                source_line: 3,
            },
            CoreferenceData {
                referent: "the healer".to_string(),
                canonical: "Bob".to_string(),
                coref_type: CorefType::Appositive,
                confidence: 0.95,
                sentence_idx: 0,
                token_idx: 4,
                source_line: 1,
            },
        ];

        let mut entity_type_map = HashMap::new();
        entity_type_map.insert("sarah".to_string(), "PERSON".to_string());
        entity_type_map.insert("bob".to_string(), "PERSON".to_string());

        let chains = build_coreference_chains(&corefs, &entity_type_map);
        assert_eq!(chains.len(), 2);

        // Sorted by canonical name: Bob < Sarah
        assert_eq!(chains[0].canonical, "Bob");
        assert_eq!(chains[0].total_mention_count, 1);
        assert_eq!(chains[0].aliases, vec!["the healer"]);

        assert_eq!(chains[1].canonical, "Sarah");
        assert_eq!(chains[1].entity_type, "PERSON");
        assert_eq!(chains[1].total_mention_count, 3);
        // aliases deduplicated: "the detective" and "she"
        assert_eq!(chains[1].aliases.len(), 2);
        assert!(chains[1].aliases.contains(&"the detective".to_string()));
        assert!(chains[1].aliases.contains(&"she".to_string()));
    }

    // ── Phase 2 tests ────────────────────────────────────────────────────────

    // "Sarah saw the cat and she smiled."
    #[test]
    fn same_sentence_with_antecedent() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("saw", "VERB", "ROOT", 1, "", 6),
            make_token("the", "DET", "det", 3, "", 10),
            make_token("cat", "NOUN", "dobj", 1, "", 14),
            make_token("and", "CCONJ", "cc", 5, "", 18),
            make_token("she", "PRON", "nsubj", 5, "", 22),
            make_token("smiled", "VERB", "conj", 1, "", 26),
            make_token(".", "PUNCT", "punct", 1, "", 33),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "she");
        assert_eq!(results[0].canonical, "Sarah");
        assert_eq!(results[0].coref_type, CorefType::SameSentencePronoun);
        assert!((results[0].confidence - 0.90).abs() < f32::EPSILON);
    }

    // "Jane ran away to her home in Azure."
    #[test]
    fn possessive_pronoun_test() {
        let tokens = vec![
            make_token("Jane", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("ran", "VERB", "ROOT", 1, "", 5),
            make_token("away", "ADV", "advmod", 1, "", 9),
            make_token("to", "ADP", "prep", 1, "", 14),
            make_token("her", "PRON", "poss", 5, "", 17),
            make_token("home", "NOUN", "pobj", 3, "", 21),
            make_token("in", "ADP", "prep", 5, "", 26),
            make_token("Azure", "PROPN", "pobj", 6, "GPE", 29),
            make_token(".", "PUNCT", "punct", 1, "", 34),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "her");
        assert_eq!(results[0].canonical, "Jane");
        assert_eq!(results[0].coref_type, CorefType::PossessivePronoun);
        assert!((results[0].confidence - 0.90).abs() < f32::EPSILON);
    }

    // "It rained all day." — expletive, no resolution
    #[test]
    fn expletive_it() {
        let tokens = vec![
            make_token("It", "PRON", "nsubj", 1, "", 0),
            make_token("rained", "VERB", "ROOT", 1, "", 3),
            make_token("all", "DET", "det", 3, "", 10),
            make_token("day", "NOUN", "npadvmod", 1, "", 14),
            make_token(".", "PUNCT", "punct", 1, "", 17),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);
        assert!(results.is_empty());
    }

    // "Sarah picked up the book and read it."
    #[test]
    fn neutral_it_resolved() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("picked", "VERB", "ROOT", 1, "", 6),
            make_token("up", "ADP", "prt", 1, "", 13),
            make_token("the", "DET", "det", 4, "", 16),
            make_token("book", "NOUN", "dobj", 1, "", 20),
            make_token("and", "CCONJ", "cc", 6, "", 25),
            make_token("read", "VERB", "conj", 1, "", 29),
            make_token("it", "PRON", "dobj", 6, "", 34),
            make_token(".", "PUNCT", "punct", 1, "", 36),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "it");
        assert_eq!(results[0].canonical, "the book");
        assert_eq!(results[0].coref_type, CorefType::SameSentencePronoun);
        assert!((results[0].confidence - 0.90).abs() < f32::EPSILON);
    }

    // "Sarah hurt herself." — reflexive skipped
    #[test]
    fn reflexive_skipped() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("hurt", "VERB", "ROOT", 1, "", 6),
            make_token("herself", "PRON", "dobj", 1, "", 11),
            make_token(".", "PUNCT", "punct", 1, "", 18),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);
        assert!(results.is_empty());
    }

    // "Sarah chased the cat." — no pronouns
    #[test]
    fn no_pronoun() {
        let tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("chased", "VERB", "ROOT", 1, "", 6),
            make_token("the", "DET", "det", 3, "", 13),
            make_token("cat", "NOUN", "dobj", 1, "", 17),
            make_token(".", "PUNCT", "punct", 1, "", 20),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);
        assert!(results.is_empty());
    }

    // "She ran to the store." — subject pronoun with no preceding entity → deferred
    #[test]
    fn subject_pronoun_skipped_for_cross_sentence() {
        let tokens = vec![
            make_token("She", "PRON", "nsubj", 1, "", 0),
            make_token("ran", "VERB", "ROOT", 1, "", 4),
            make_token("to", "ADP", "prep", 1, "", 8),
            make_token("the", "DET", "det", 4, "", 11),
            make_token("store", "NOUN", "pobj", 2, "", 15),
            make_token(".", "PUNCT", "punct", 1, "", 19),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let results = resolve_same_sentence_pronouns(&sentence, 0, &line_starts);
        assert!(results.is_empty());
    }

    // ── Phase 3 tests ────────────────────────────────────────────────────────

    // "David was punched." → "He fell to the ground."
    #[test]
    fn cross_sentence_simple() {
        let s1_tokens = vec![
            make_token("David", "PROPN", "nsubjpass", 2, "PERSON", 0),
            make_token("was", "AUX", "auxpass", 2, "", 6),
            make_token("punched", "VERB", "ROOT", 2, "", 10),
            make_token(".", "PUNCT", "punct", 2, "", 17),
        ];
        let s1 = make_sentence(s1_tokens);

        let s2_tokens = vec![
            make_token("He", "PRON", "nsubj", 1, "", 0),
            make_token("fell", "VERB", "ROOT", 1, "", 3),
            make_token("to", "ADP", "prep", 1, "", 8),
            make_token("the", "DET", "det", 4, "", 11),
            make_token("ground", "NOUN", "pobj", 2, "", 15),
            make_token(".", "PUNCT", "punct", 1, "", 21),
        ];
        let s2 = make_sentence(s2_tokens);

        let line_starts = vec![0usize];
        // Phase 2 on s2 returns empty (He is nsubj, no entity before it)
        let phase2 = resolve_same_sentence_pronouns(&s2, 1, &line_starts);
        assert!(phase2.is_empty());

        let entity_gender_map = HashMap::new();
        let topic_entities = HashMap::new();
        let results = resolve_cross_sentence_pronouns(
            &s2,
            1,
            Some(&s1),
            true,
            &[],
            &entity_gender_map,
            &topic_entities,
            &line_starts,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "He");
        assert_eq!(results[0].canonical, "David");
        assert_eq!(results[0].coref_type, CorefType::CrossSentencePronoun);
        assert!((results[0].confidence - 0.80).abs() < f32::EPSILON);
    }

    // "Sarah arrived." → "David sat down." → "She smiled."
    // She resolves to Sarah (from topic_entities), not David (gender-mismatched)
    #[test]
    fn cross_sentence_gender_filter() {
        let s2_tokens = vec![
            make_token("David", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("sat", "VERB", "ROOT", 1, "", 6),
            make_token("down", "ADP", "prt", 1, "", 10),
            make_token(".", "PUNCT", "punct", 1, "", 14),
        ];
        let s2 = make_sentence(s2_tokens);

        let s3_tokens = vec![
            make_token("She", "PRON", "nsubj", 1, "", 0),
            make_token("smiled", "VERB", "ROOT", 1, "", 4),
            make_token(".", "PUNCT", "punct", 1, "", 11),
        ];
        let s3 = make_sentence(s3_tokens);

        let line_starts = vec![0usize];
        let mut entity_gender_map = HashMap::new();
        entity_gender_map.insert("sarah".to_string(), Gender::Female);
        entity_gender_map.insert("david".to_string(), Gender::Male);

        let mut topic_entities = HashMap::new();
        topic_entities.insert(Gender::Female, "Sarah".to_string());

        let results = resolve_cross_sentence_pronouns(
            &s3,
            2,
            Some(&s2),
            true,
            &[],
            &entity_gender_map,
            &topic_entities,
            &line_starts,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].referent, "She");
        assert_eq!(results[0].canonical, "Sarah");
        assert_eq!(results[0].coref_type, CorefType::CrossSentencePronoun);
        assert!((results[0].confidence - 0.70).abs() < f32::EPSILON);
    }

    // Paragraph boundary blocks cross-sentence resolution
    #[test]
    fn paragraph_boundary_blocks() {
        let s1_tokens = vec![
            make_token("Sarah", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("arrived", "VERB", "ROOT", 1, "", 6),
            make_token(".", "PUNCT", "punct", 1, "", 13),
        ];
        let s1 = make_sentence(s1_tokens);

        let s2_tokens = vec![
            make_token("He", "PRON", "nsubj", 1, "", 0),
            make_token("entered", "VERB", "ROOT", 1, "", 3),
            make_token("the", "DET", "det", 3, "", 11),
            make_token("room", "NOUN", "dobj", 1, "", 15),
            make_token(".", "PUNCT", "punct", 1, "", 19),
        ];
        let s2 = make_sentence(s2_tokens);

        let line_starts = vec![0usize];
        let entity_gender_map = HashMap::new();
        let topic_entities = HashMap::new();

        let results = resolve_cross_sentence_pronouns(
            &s2,
            1,
            Some(&s1),
            false,
            &[],
            &entity_gender_map,
            &topic_entities,
            &line_starts,
        );
        assert!(results.is_empty());
    }

    // No previous sentence → nothing resolved
    #[test]
    fn no_previous_sentence() {
        let tokens = vec![
            make_token("She", "PRON", "nsubj", 1, "", 0),
            make_token("ran", "VERB", "ROOT", 1, "", 4),
            make_token(".", "PUNCT", "punct", 1, "", 7),
        ];
        let sentence = make_sentence(tokens);
        let line_starts = vec![0usize];
        let entity_gender_map = HashMap::new();
        let topic_entities = HashMap::new();

        let results = resolve_cross_sentence_pronouns(
            &sentence,
            0,
            None,
            true,
            &[],
            &entity_gender_map,
            &topic_entities,
            &line_starts,
        );
        assert!(results.is_empty());
    }

    // "Jane came back to life." → "She ran away to her home in Azure."
    // Both She and her resolve to Jane (cross-sentence); her gets PossessivePronoun type
    #[test]
    fn cross_sentence_possessive() {
        let s1_tokens = vec![
            make_token("Jane", "PROPN", "nsubj", 1, "PERSON", 0),
            make_token("came", "VERB", "ROOT", 1, "", 5),
            make_token("back", "ADV", "advmod", 1, "", 10),
            make_token("to", "ADP", "prep", 1, "", 15),
            make_token("life", "NOUN", "pobj", 3, "", 18),
            make_token(".", "PUNCT", "punct", 1, "", 22),
        ];
        let s1 = make_sentence(s1_tokens);

        let s2_tokens = vec![
            make_token("She", "PRON", "nsubj", 1, "", 0),
            make_token("ran", "VERB", "ROOT", 1, "", 4),
            make_token("away", "ADV", "advmod", 1, "", 8),
            make_token("to", "ADP", "prep", 1, "", 13),
            make_token("her", "PRON", "poss", 5, "", 17),
            make_token("home", "NOUN", "pobj", 3, "", 21),
            make_token("in", "ADP", "prep", 5, "", 26),
            make_token("Azure", "PROPN", "pobj", 6, "GPE", 29),
            make_token(".", "PUNCT", "punct", 1, "", 35),
        ];
        let s2 = make_sentence(s2_tokens);

        let line_starts = vec![0usize];
        // Phase 2 on s2: She(nsubj) skipped (first entity Azure at idx 7; 0 <= 7);
        //               her(poss) has no candidates before it → empty
        let phase2 = resolve_same_sentence_pronouns(&s2, 1, &line_starts);
        assert!(phase2.is_empty());

        let entity_gender_map = HashMap::new();
        let topic_entities = HashMap::new();
        let results = resolve_cross_sentence_pronouns(
            &s2,
            1,
            Some(&s1),
            true,
            &[],
            &entity_gender_map,
            &topic_entities,
            &line_starts,
        );

        assert_eq!(results.len(), 2);
        let she = results
            .iter()
            .find(|r| r.referent == "She")
            .expect("She not found");
        assert_eq!(she.canonical, "Jane");
        assert_eq!(she.coref_type, CorefType::CrossSentencePronoun);
        assert!((she.confidence - 0.80).abs() < f32::EPSILON);

        let her = results
            .iter()
            .find(|r| r.referent == "her")
            .expect("her not found");
        assert_eq!(her.canonical, "Jane");
        assert_eq!(her.coref_type, CorefType::PossessivePronoun);
        assert!((her.confidence - 0.80).abs() < f32::EPSILON);
    }
}
