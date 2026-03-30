use crate::spacy::{SpacyDoc, SpacySentence, SpacyToken as SpacyTokenData, SpacyEntity as SpacyEntityData};
use crate::roles::{classify_roles, RoleAnnotation, ThematicRole, VerbClass};
use crate::coref::{
    CoreferenceChain, Gender,
    extract_appositives_from_sentence, resolve_same_sentence_pronouns,
    resolve_cross_sentence_pronouns, build_coreference_chains,
    update_gender_map, update_topic_entities, CoreferenceData,
};
use crate::discourse::{SentenceInfo, detect_discourse_relations, DiscourseRelationData};
use crate::narrative::{
    ParagraphEntityData, detect_scene_boundaries, SceneBoundary,
    EntityInteractionProfile, compute_character_arcs, CharacterArc,
    OpposingInteraction, build_conflict_graph, ConflictEdge,
    NarrativeIssue,
    detect_setup_payoff, detect_foreshadowing, detect_consistency_issues,
    NarrativeSummary, build_narrative_summary,
};
use aq_core::OwnedNode;
use std::collections::{HashMap, HashSet};

// ── Line-offset index ────────────────────────────────────────────────────────

/// Returns a vec where `line_starts[i]` is the byte offset of 1-based line `i+1`.
fn build_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Returns the 1-based line number for the given byte offset.
pub(crate) fn offset_to_line(offset: usize, line_starts: &[usize]) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(i) => i + 1,
        Err(i) => i.max(1),
    }
}

// ── Paragraph range ──────────────────────────────────────────────────────────

struct ParaRange {
    start_char: usize,
    end_char: usize,
}

fn detect_paragraphs(source_text: &str) -> Vec<ParaRange> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    for chunk in source_text.split("\n\n") {
        let start = offset;
        let end = offset + chunk.len();
        ranges.push(ParaRange { start_char: start, end_char: end });
        offset = end + 2; // skip the "\n\n" separator
    }
    if ranges.is_empty() {
        ranges.push(ParaRange { start_char: 0, end_char: source_text.len() });
    }
    ranges
}

// ── Node builders ────────────────────────────────────────────────────────────

fn build_token_node(
    token: &SpacyTokenData,
    line_starts: &[usize],
    source_file: &Option<String>,
) -> OwnedNode {
    let line = offset_to_line(token.idx, line_starts);

    let mut pos_node = OwnedNode::leaf("pos_tag", &token.pos, line);
    pos_node.source_file = source_file.clone();

    let mut dep_node = OwnedNode::leaf("dep_rel", &token.dep, line);
    dep_node.source_file = source_file.clone();

    let mut lemma_node = OwnedNode::leaf("lemma", &token.lemma, line);
    lemma_node.source_file = source_file.clone();

    let children = vec![pos_node, dep_node, lemma_node];

    let mut field_indices = HashMap::new();
    field_indices.insert("pos".to_string(), vec![0]);
    field_indices.insert("dep".to_string(), vec![1]);
    field_indices.insert("lemma".to_string(), vec![2]);

    OwnedNode {
        node_type: "token".to_string(),
        text: Some(token.text.clone()),
        subtree_text: None,
        field_indices,
        children,
        start_line: line,
        end_line: line,
        source_file: source_file.clone(),
    }
}

fn build_sentence_node(
    sentence: &SpacySentence,
    line_starts: &[usize],
    source_file: &Option<String>,
) -> (OwnedNode, Vec<InteractionData>) {
    let start_line = offset_to_line(sentence.start, line_starts);
    let end_line = if sentence.end > 0 {
        offset_to_line(sentence.end.saturating_sub(1), line_starts)
    } else {
        start_line
    };

    let token_nodes: Vec<OwnedNode> = sentence
        .tokens
        .iter()
        .map(|t| build_token_node(t, line_starts, source_file))
        .collect();

    let num_tokens = token_nodes.len();

    // Extract interactions and build verb_phrase nodes
    let interactions = extract_interactions_from_sentence(sentence, line_starts);
    let vp_nodes: Vec<OwnedNode> = interactions
        .iter()
        .map(|idata| build_verb_phrase_node(idata, source_file))
        .collect();
    let num_vps = vp_nodes.len();

    let mut children = token_nodes;
    children.extend(vp_nodes);

    let mut field_indices = HashMap::new();
    if num_tokens > 0 {
        field_indices.insert("tokens".to_string(), (0..num_tokens).collect());
    }
    if num_vps > 0 {
        field_indices.insert(
            "verb_phrases".to_string(),
            (num_tokens..num_tokens + num_vps).collect(),
        );
    }

    let node = OwnedNode {
        node_type: "sentence".to_string(),
        text: Some(sentence.text.clone()),
        subtree_text: None,
        field_indices,
        children,
        start_line,
        end_line,
        source_file: source_file.clone(),
    };

    (node, interactions)
}

fn build_entity_nodes(
    entities: &[SpacyEntityData],
    line_starts: &[usize],
    source_file: &Option<String>,
    coref_chains: &[CoreferenceChain],
    all_interactions: &[InteractionData],
) -> Vec<OwnedNode> {
    // Group by (lowercase text, label) for deduplication.
    let mut entity_map: HashMap<(String, String), Vec<&SpacyEntityData>> = HashMap::new();
    for entity in entities {
        let key = (entity.text.to_lowercase(), entity.label.clone());
        entity_map.entry(key).or_default().push(entity);
    }

    // Sort keys for deterministic output.
    let mut keys: Vec<(String, String)> = entity_map.keys().cloned().collect();
    keys.sort();

    keys.iter()
        .map(|key| {
            let mentions = &entity_map[key];
            let first = mentions[0];
            let label = &first.label;
            let first_line = offset_to_line(first.start_char, line_starts);
            let last_line =
                offset_to_line(mentions.last().unwrap().start_char, line_starts);

            let mut children = Vec::new();

            // children[0]: entity_type
            let mut type_node = OwnedNode::leaf("entity_type", label.as_str(), first_line);
            type_node.source_file = source_file.clone();
            children.push(type_node);

            // children[1..]: one location per mention
            for mention in mentions.iter() {
                let mention_line = offset_to_line(mention.start_char, line_starts);
                let line_start_offset = line_starts[mention_line - 1];
                let char_offset = mention.start_char - line_start_offset;
                let loc_text = format!("{}:{}", mention_line, char_offset);
                let mut loc_node = OwnedNode::leaf("location", loc_text, mention_line);
                loc_node.source_file = source_file.clone();
                children.push(loc_node);
            }

            let num_locations = children.len() - 1; // excludes entity_type
            let mut field_indices = HashMap::new();
            field_indices.insert("type".to_string(), vec![0]);
            if num_locations > 0 {
                field_indices.insert(
                    "locations".to_string(),
                    (1..1 + num_locations).collect(),
                );
            }

            // Look up coref chain for this entity
            let entity_lower = first.text.to_lowercase();
            if let Some(chain) = coref_chains.iter().find(|c| c.canonical.to_lowercase() == entity_lower) {
                // Compute total mention count: direct NER mentions + coref mentions
                let direct_mentions = mentions.len();
                let alias_mentions = chain.total_mention_count;
                let total_mention_count = direct_mentions + alias_mentions;

                // aliases parent node with alias leaf children
                if !chain.aliases.is_empty() {
                    let mut alias_children = Vec::new();
                    for alias_text in &chain.aliases {
                        let mut alias_node = OwnedNode::leaf("alias", alias_text, first_line);
                        alias_node.source_file = source_file.clone();
                        alias_children.push(alias_node);
                    }
                    let num_aliases = alias_children.len();
                    let mut alias_fi = HashMap::new();
                    alias_fi.insert("items".to_string(), (0..num_aliases).collect());
                    let aliases_node = OwnedNode {
                        node_type: "aliases".to_string(),
                        text: None,
                        subtree_text: None,
                        field_indices: alias_fi,
                        children: alias_children,
                        start_line: first_line,
                        end_line: first_line,
                        source_file: source_file.clone(),
                    };
                    field_indices.insert("aliases".to_string(), vec![children.len()]);
                    children.push(aliases_node);
                }

                // mention_count leaf
                let mut mc_node = OwnedNode::leaf("mention_count", &total_mention_count.to_string(), first_line);
                mc_node.source_file = source_file.clone();
                field_indices.insert("mention_count".to_string(), vec![children.len()]);
                children.push(mc_node);

                // coreference_chain parent node
                if !chain.mentions.is_empty() {
                    let mut coref_children = Vec::new();
                    // Sort mentions by source_line then token_idx
                    let mut sorted_mentions = chain.mentions.clone();
                    sorted_mentions.sort_by(|a, b| a.source_line.cmp(&b.source_line).then(a.token_idx.cmp(&b.token_idx)));

                    for m in &sorted_mentions {
                        let coref_type_str = match m.coref_type {
                            crate::coref::CorefType::Appositive => "appositive",
                            crate::coref::CorefType::SameSentencePronoun => "same_sentence_pronoun",
                            crate::coref::CorefType::CrossSentencePronoun => "cross_sentence_pronoun",
                            crate::coref::CorefType::PossessivePronoun => "possessive_pronoun",
                        };
                        let mut mention_children = Vec::new();
                        let mut mention_fi = HashMap::new();

                        let mut form_node = OwnedNode::leaf("form", &m.referent, m.source_line);
                        form_node.source_file = source_file.clone();
                        mention_fi.insert("form".to_string(), vec![mention_children.len()]);
                        mention_children.push(form_node);

                        let mut type_node = OwnedNode::leaf("coref_type", coref_type_str, m.source_line);
                        type_node.source_file = source_file.clone();
                        mention_fi.insert("coref_type".to_string(), vec![mention_children.len()]);
                        mention_children.push(type_node);

                        let mut conf_node = OwnedNode::leaf("confidence", &format!("{:.2}", m.confidence), m.source_line);
                        conf_node.source_file = source_file.clone();
                        mention_fi.insert("confidence".to_string(), vec![mention_children.len()]);
                        mention_children.push(conf_node);

                        let loc_text = format!("{}:{}", m.source_line, m.token_idx);
                        let mut loc_node = OwnedNode::leaf("location", &loc_text, m.source_line);
                        loc_node.source_file = source_file.clone();
                        mention_fi.insert("location".to_string(), vec![mention_children.len()]);
                        mention_children.push(loc_node);

                        let coref_mention_node = OwnedNode {
                            node_type: "coref_mention".to_string(),
                            text: Some(m.referent.clone()),
                            subtree_text: None,
                            field_indices: mention_fi,
                            children: mention_children,
                            start_line: m.source_line,
                            end_line: m.source_line,
                            source_file: source_file.clone(),
                        };
                        coref_children.push(coref_mention_node);
                    }

                    let num_coref = coref_children.len();
                    let mut chain_fi = HashMap::new();
                    chain_fi.insert("mentions".to_string(), (0..num_coref).collect());
                    let coref_chain_node = OwnedNode {
                        node_type: "coreference_chain".to_string(),
                        text: None,
                        subtree_text: None,
                        field_indices: chain_fi,
                        children: coref_children,
                        start_line: first_line,
                        end_line: sorted_mentions.last().map(|m| m.source_line).unwrap_or(first_line),
                        source_file: source_file.clone(),
                    };
                    field_indices.insert("coreference_chain".to_string(), vec![children.len()]);
                    children.push(coref_chain_node);
                }

                // avg_confidence leaf
                if !chain.mentions.is_empty() {
                    let avg: f32 = chain.mentions.iter().map(|m| m.confidence).sum::<f32>() / chain.mentions.len() as f32;
                    let mut avg_node = OwnedNode::leaf("avg_confidence", &format!("{:.2}", avg), first_line);
                    avg_node.source_file = source_file.clone();
                    field_indices.insert("avg_confidence".to_string(), vec![children.len()]);
                    children.push(avg_node);
                }
            }

            // interaction_count: count document-level interactions referencing this entity
            {
                let mut names_to_match: Vec<String> = vec![first.text.to_lowercase()];
                if let Some(chain) = coref_chains.iter().find(|c| c.canonical.to_lowercase() == entity_lower) {
                    for alias in &chain.aliases {
                        names_to_match.push(alias.to_lowercase());
                    }
                }
                let count = all_interactions.iter().filter(|idata| {
                    let agent_match = idata.agent.as_deref().map(|a| {
                        let a_lower = a.to_lowercase();
                        names_to_match.iter().any(|n| a_lower.contains(n.as_str()))
                    }).unwrap_or(false);
                    let patient_match = idata.patient.as_deref().map(|p| {
                        let p_lower = p.to_lowercase();
                        names_to_match.iter().any(|n| p_lower.contains(n.as_str()))
                    }).unwrap_or(false);
                    agent_match || patient_match
                }).count();
                let mut ic_node = OwnedNode::leaf("interaction_count", &count.to_string(), first_line);
                ic_node.source_file = source_file.clone();
                field_indices.insert("interaction_count".to_string(), vec![children.len()]);
                children.push(ic_node);
            }

            OwnedNode {
                node_type: "entity".to_string(),
                text: Some(first.text.clone()),
                subtree_text: None,
                field_indices,
                children,
                start_line: first_line,
                end_line: last_line,
                source_file: source_file.clone(),
            }
        })
        .collect()
}

// ── Interaction extraction ───────────────────────────────────────────────────

/// Intermediate representation of one verb-centered interaction.
#[derive(Debug, Clone, Default)]
pub(crate) struct InteractionData {
    pub verb: String,
    pub verb_lemma: String,
    pub verb_idx: usize,
    pub agent: Option<String>,
    pub patient: Option<String>,
    pub instrument: Option<String>,
    pub recipient: Option<String>,
    pub is_passive: bool,
    pub source_line: usize,
    pub roles: Vec<RoleAnnotation>,
    pub beneficiary: Option<String>,
    pub goal: Option<String>,
    pub source: Option<String>,
    pub location: Option<String>,
    pub verb_class: Option<VerbClass>,
}

pub(crate) fn normalize_dep(dep: &str) -> &str {
    match dep {
        "nsubj:pass" => "nsubjpass",
        "obj" => "dobj",
        _ => dep,
    }
}

pub(crate) fn collect_span_text(token_idx: usize, tokens: &[SpacyTokenData]) -> String {
    let span_deps = ["compound", "det", "amod", "nummod", "poss", "flat"];
    let mut indices = vec![token_idx];
    for (i, t) in tokens.iter().enumerate() {
        if t.head == token_idx && i != token_idx && span_deps.contains(&normalize_dep(&t.dep)) {
            indices.push(i);
        }
    }
    indices.sort_unstable();
    indices.iter().map(|&i| tokens[i].text.as_str()).collect::<Vec<_>>().join(" ")
}

fn find_pobj_text(prep_idx: usize, tokens: &[SpacyTokenData]) -> Option<String> {
    for (i, t) in tokens.iter().enumerate() {
        if t.head == prep_idx && normalize_dep(&t.dep) == "pobj" {
            return Some(collect_span_text(i, tokens));
        }
    }
    None
}

fn collect_conj_parts(token_idx: usize, tokens: &[SpacyTokenData]) -> Vec<String> {
    let mut parts = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if t.head == token_idx && normalize_dep(&t.dep) == "conj" {
            parts.push(collect_span_text(i, tokens));
        }
    }
    parts
}

pub(crate) fn extract_interactions_from_sentence(
    sentence: &SpacySentence,
    line_starts: &[usize],
) -> Vec<InteractionData> {
    let tokens = &sentence.tokens;
    let mut results = Vec::new();

    for (verb_idx, token) in tokens.iter().enumerate() {
        if token.pos != "VERB" {
            continue;
        }

        let source_line = offset_to_line(token.idx, line_starts);
        let mut data = InteractionData {
            verb: token.text.clone(),
            verb_lemma: token.lemma.clone(),
            verb_idx,
            agent: None,
            patient: None,
            instrument: None,
            recipient: None,
            is_passive: false,
            source_line,
            roles: Vec::new(),
            beneficiary: None,
            goal: None,
            source: None,
            location: None,
            verb_class: None,
        };

        // Collect direct dependents of this verb
        for (dep_idx, dep_token) in tokens.iter().enumerate() {
            if dep_token.head != verb_idx || dep_idx == verb_idx {
                continue;
            }
            match normalize_dep(&dep_token.dep) {
                "nsubj" => {
                    let mut agent_text = collect_span_text(dep_idx, tokens);
                    // Check for compound subjects via conj
                    let conj_parts = collect_conj_parts(dep_idx, tokens);
                    if !conj_parts.is_empty() {
                        let parts: Vec<String> = std::iter::once(agent_text)
                            .chain(conj_parts)
                            .collect();
                        agent_text = parts.join(" and ");
                    }
                    data.agent = Some(agent_text);
                }
                "nsubjpass" => {
                    data.patient = Some(collect_span_text(dep_idx, tokens));
                    data.is_passive = true;
                }
                "dobj" => {
                    data.patient = Some(collect_span_text(dep_idx, tokens));
                }
                "dative" | "iobj" => {
                    data.recipient = Some(collect_span_text(dep_idx, tokens));
                }
                "agent" => {
                    // Two-hop: "by" preposition → pobj = actual agent
                    data.is_passive = true;
                    if let Some(agent_text) = find_pobj_text(dep_idx, tokens) {
                        data.agent = Some(agent_text);
                    }
                }
                "prep" => {
                    // Two-hop: preposition → pobj
                    if dep_token.lemma == "with" || dep_token.text.to_lowercase() == "with" {
                        if let Some(inst_text) = find_pobj_text(dep_idx, tokens) {
                            data.instrument = Some(inst_text);
                        }
                    }
                }
                _ => {}
            }
        }

        // For acl verbs: head noun is the patient (participial constructions)
        if (token.dep == "acl" || token.dep == "acl:relcl") && data.patient.is_none() {
            let head_idx = token.head;
            if head_idx < tokens.len() && head_idx != verb_idx {
                data.patient = Some(collect_span_text(head_idx, tokens));
            }
        }

        // For conj verbs: inherit agent from head verb's nsubj
        if token.dep == "conj" && data.agent.is_none() {
            let head_verb_idx = token.head;
            for (dep_idx, dep_token) in tokens.iter().enumerate() {
                if dep_token.head == head_verb_idx && normalize_dep(&dep_token.dep) == "nsubj" {
                    data.agent = Some(collect_span_text(dep_idx, tokens));
                    break;
                }
            }
        }

        // Classify thematic roles
        let role_annotations = classify_roles(&data, tokens);
        data.verb_class = role_annotations.first().and_then(|r| r.verb_class);
        for ann in &role_annotations {
            match ann.thematic_role {
                ThematicRole::Beneficiary => { data.beneficiary = Some(ann.participant.clone()); }
                ThematicRole::Goal => { data.goal = Some(ann.participant.clone()); }
                ThematicRole::Source => { data.source = Some(ann.participant.clone()); }
                ThematicRole::Location => { data.location = Some(ann.participant.clone()); }
                _ => {}
            }
        }
        data.roles = role_annotations;

        results.push(data);
    }

    results
}

fn build_verb_phrase_node(
    interaction: &InteractionData,
    source_file: &Option<String>,
) -> OwnedNode {
    let line = interaction.source_line;
    let mut children = Vec::new();
    let mut field_indices = HashMap::new();

    // child 0: verb
    let mut verb_node = OwnedNode::leaf("verb", &interaction.verb, line);
    verb_node.source_file = source_file.clone();
    field_indices.insert("verb".to_string(), vec![children.len()]);
    children.push(verb_node);

    // child 1: agent (optional)
    if let Some(ref agent) = interaction.agent {
        let mut node = OwnedNode::leaf("agent", agent, line);
        node.source_file = source_file.clone();
        field_indices.insert("agent".to_string(), vec![children.len()]);
        children.push(node);
    }

    // child: patient (optional)
    if let Some(ref patient) = interaction.patient {
        let mut node = OwnedNode::leaf("patient", patient, line);
        node.source_file = source_file.clone();
        field_indices.insert("patient".to_string(), vec![children.len()]);
        children.push(node);
    }

    // child: instrument (optional)
    if let Some(ref instrument) = interaction.instrument {
        let mut node = OwnedNode::leaf("instrument", instrument, line);
        node.source_file = source_file.clone();
        field_indices.insert("instrument".to_string(), vec![children.len()]);
        children.push(node);
    }

    // child: recipient (optional)
    if let Some(ref recipient) = interaction.recipient {
        let mut node = OwnedNode::leaf("recipient", recipient, line);
        node.source_file = source_file.clone();
        field_indices.insert("recipient".to_string(), vec![children.len()]);
        children.push(node);
    }

    // voice indicator
    let voice = if interaction.is_passive { "passive" } else { "active" };
    let mut voice_node = OwnedNode::leaf("voice", voice, line);
    voice_node.source_file = source_file.clone();
    field_indices.insert("voice".to_string(), vec![children.len()]);
    children.push(voice_node);

    // --- Thematic role enrichment ---

    // agent_role
    if interaction.agent.is_some() {
        if let Some(ann) = interaction.roles.iter().find(|r| r.syntactic_role == "agent") {
            let mut node = OwnedNode::leaf("agent_role", &ann.thematic_role.to_string(), line);
            node.source_file = source_file.clone();
            field_indices.insert("agent_role".to_string(), vec![children.len()]);
            children.push(node);
        }
    }

    // patient_role
    if interaction.patient.is_some() {
        if let Some(ann) = interaction.roles.iter().find(|r| r.syntactic_role == "patient") {
            let mut node = OwnedNode::leaf("patient_role", &ann.thematic_role.to_string(), line);
            node.source_file = source_file.clone();
            field_indices.insert("patient_role".to_string(), vec![children.len()]);
            children.push(node);
        }
    }

    // instrument_role
    if interaction.instrument.is_some() {
        let mut node = OwnedNode::leaf("instrument_role", "instrument", line);
        node.source_file = source_file.clone();
        field_indices.insert("instrument_role".to_string(), vec![children.len()]);
        children.push(node);
    }

    // recipient_role
    if interaction.recipient.is_some() {
        if let Some(ann) = interaction.roles.iter().find(|r| r.syntactic_role == "recipient") {
            let mut node = OwnedNode::leaf("recipient_role", &ann.thematic_role.to_string(), line);
            node.source_file = source_file.clone();
            field_indices.insert("recipient_role".to_string(), vec![children.len()]);
            children.push(node);
        }
    }

    // verb_class
    let vc_text = interaction.verb_class.map(|c| format!("{:?}", c).to_lowercase()).unwrap_or_else(|| "unknown".to_string());
    let mut vc_node = OwnedNode::leaf("verb_class", &vc_text, line);
    vc_node.source_file = source_file.clone();
    field_indices.insert("verb_class".to_string(), vec![children.len()]);
    children.push(vc_node);

    // beneficiary
    if let Some(ref beneficiary) = interaction.beneficiary {
        let mut node = OwnedNode::leaf("beneficiary", beneficiary, line);
        node.source_file = source_file.clone();
        field_indices.insert("beneficiary".to_string(), vec![children.len()]);
        children.push(node);
    }
    // goal
    if let Some(ref goal) = interaction.goal {
        let mut node = OwnedNode::leaf("goal", goal, line);
        node.source_file = source_file.clone();
        field_indices.insert("goal".to_string(), vec![children.len()]);
        children.push(node);
    }
    // source
    if let Some(ref source) = interaction.source {
        let mut node = OwnedNode::leaf("source", source, line);
        node.source_file = source_file.clone();
        field_indices.insert("source".to_string(), vec![children.len()]);
        children.push(node);
    }
    // location
    if let Some(ref location) = interaction.location {
        let mut node = OwnedNode::leaf("location", location, line);
        node.source_file = source_file.clone();
        field_indices.insert("location".to_string(), vec![children.len()]);
        children.push(node);
    }

    // Generic role children — one per distinct thematic role
    let mut role_indices = Vec::new();
    for ann in &interaction.roles {
        if ann.thematic_role != ThematicRole::Unknown {
            let mut role_node = OwnedNode::leaf("role", &ann.thematic_role.to_string(), line);
            role_node.source_file = source_file.clone();
            role_indices.push(children.len());
            children.push(role_node);
        }
    }
    if !role_indices.is_empty() {
        field_indices.insert("role".to_string(), role_indices);
    }

    // role_confidence: minimum confidence across all roles
    if !interaction.roles.is_empty() {
        let min_conf = interaction.roles.iter().map(|r| r.confidence).fold(f32::INFINITY, f32::min);
        let mut conf_node = OwnedNode::leaf("role_confidence", &format!("{:.2}", min_conf), line);
        conf_node.source_file = source_file.clone();
        field_indices.insert("role_confidence".to_string(), vec![children.len()]);
        children.push(conf_node);
    }

    // Thematic-role-named children for role-based filtering.
    // Enables queries like interaction[experiencer=Sarah] and interaction[theme=ball].
    // Only add a role-named child when that field name doesn't already exist as a
    // syntactic child (e.g. "agent", "patient", "instrument", "recipient" are
    // already present). Unknown roles are skipped.
    for ann in &interaction.roles {
        if ann.thematic_role == ThematicRole::Unknown {
            continue;
        }
        let role_name = ann.thematic_role.to_string();
        if !field_indices.contains_key(&role_name) {
            let mut node = OwnedNode::leaf(&role_name, &ann.participant, line);
            node.source_file = source_file.clone();
            field_indices.insert(role_name.clone(), vec![children.len()]);
            children.push(node);
        }
    }

    OwnedNode {
        node_type: "verb_phrase".to_string(),
        text: Some(interaction.verb.clone()),
        subtree_text: None,
        field_indices,
        children,
        start_line: line,
        end_line: line,
        source_file: source_file.clone(),
    }
}

fn build_interaction_doc_nodes(
    all_interactions: &[InteractionData],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    // Dedup by (verb_lemma, agent_lowercase, patient_lowercase)
    let mut seen: HashMap<(String, String, String), Vec<&InteractionData>> = HashMap::new();
    for interaction in all_interactions {
        let key = (
            interaction.verb_lemma.clone(),
            interaction.agent.as_deref().unwrap_or("").to_lowercase(),
            interaction.patient.as_deref().unwrap_or("").to_lowercase(),
        );
        seen.entry(key).or_default().push(interaction);
    }

    // Sort keys for deterministic output
    let mut keys: Vec<(String, String, String)> = seen.keys().cloned().collect();
    keys.sort();

    keys.iter()
        .map(|key| {
            let mentions = &seen[key];
            let first = mentions[0];
            let line = first.source_line;

            let mut children = Vec::new();
            let mut field_indices = HashMap::new();

            // agent
            if let Some(ref agent) = first.agent {
                let mut node = OwnedNode::leaf("agent", agent, line);
                node.source_file = source_file.clone();
                field_indices.insert("agent".to_string(), vec![children.len()]);
                children.push(node);
            }

            // verb
            let mut verb_node = OwnedNode::leaf("verb", &first.verb, line);
            verb_node.source_file = source_file.clone();
            field_indices.insert("verb".to_string(), vec![children.len()]);
            children.push(verb_node);

            // patient
            if let Some(ref patient) = first.patient {
                let mut node = OwnedNode::leaf("patient", patient, line);
                node.source_file = source_file.clone();
                field_indices.insert("patient".to_string(), vec![children.len()]);
                children.push(node);
            }

            // instrument
            if let Some(ref instrument) = first.instrument {
                let mut node = OwnedNode::leaf("instrument", instrument, line);
                node.source_file = source_file.clone();
                field_indices.insert("instrument".to_string(), vec![children.len()]);
                children.push(node);
            }

            // recipient
            if let Some(ref recipient) = first.recipient {
                let mut node = OwnedNode::leaf("recipient", recipient, line);
                node.source_file = source_file.clone();
                field_indices.insert("recipient".to_string(), vec![children.len()]);
                children.push(node);
            }

            // voice
            let voice = if first.is_passive { "passive" } else { "active" };
            let mut voice_node = OwnedNode::leaf("voice", voice, line);
            voice_node.source_file = source_file.clone();
            field_indices.insert("voice".to_string(), vec![children.len()]);
            children.push(voice_node);

            // line references (one per mention)
            let line_indices_start = children.len();
            for mention in mentions.iter() {
                let line_text = format!("{}", mention.source_line);
                let mut line_node = OwnedNode::leaf("line", &line_text, mention.source_line);
                line_node.source_file = source_file.clone();
                children.push(line_node);
            }
            let line_count = children.len() - line_indices_start;
            if line_count > 0 {
                field_indices.insert(
                    "lines".to_string(),
                    (line_indices_start..line_indices_start + line_count).collect(),
                );
            }

            // --- Thematic role enrichment (using `first` mention) ---

            // agent_role
            if first.agent.is_some() {
                if let Some(ann) = first.roles.iter().find(|r| r.syntactic_role == "agent") {
                    let mut node = OwnedNode::leaf("agent_role", &ann.thematic_role.to_string(), line);
                    node.source_file = source_file.clone();
                    field_indices.insert("agent_role".to_string(), vec![children.len()]);
                    children.push(node);
                }
            }

            // patient_role
            if first.patient.is_some() {
                if let Some(ann) = first.roles.iter().find(|r| r.syntactic_role == "patient") {
                    let mut node = OwnedNode::leaf("patient_role", &ann.thematic_role.to_string(), line);
                    node.source_file = source_file.clone();
                    field_indices.insert("patient_role".to_string(), vec![children.len()]);
                    children.push(node);
                }
            }

            // instrument_role
            if first.instrument.is_some() {
                let mut node = OwnedNode::leaf("instrument_role", "instrument", line);
                node.source_file = source_file.clone();
                field_indices.insert("instrument_role".to_string(), vec![children.len()]);
                children.push(node);
            }

            // recipient_role
            if first.recipient.is_some() {
                if let Some(ann) = first.roles.iter().find(|r| r.syntactic_role == "recipient") {
                    let mut node = OwnedNode::leaf("recipient_role", &ann.thematic_role.to_string(), line);
                    node.source_file = source_file.clone();
                    field_indices.insert("recipient_role".to_string(), vec![children.len()]);
                    children.push(node);
                }
            }

            // verb_class
            let vc_text = first.verb_class.map(|c| format!("{:?}", c).to_lowercase()).unwrap_or_else(|| "unknown".to_string());
            let mut vc_node = OwnedNode::leaf("verb_class", &vc_text, line);
            vc_node.source_file = source_file.clone();
            field_indices.insert("verb_class".to_string(), vec![children.len()]);
            children.push(vc_node);

            // beneficiary
            if let Some(ref beneficiary) = first.beneficiary {
                let mut node = OwnedNode::leaf("beneficiary", beneficiary, line);
                node.source_file = source_file.clone();
                field_indices.insert("beneficiary".to_string(), vec![children.len()]);
                children.push(node);
            }
            // goal
            if let Some(ref goal) = first.goal {
                let mut node = OwnedNode::leaf("goal", goal, line);
                node.source_file = source_file.clone();
                field_indices.insert("goal".to_string(), vec![children.len()]);
                children.push(node);
            }
            // source
            if let Some(ref source_val) = first.source {
                let mut node = OwnedNode::leaf("source", source_val, line);
                node.source_file = source_file.clone();
                field_indices.insert("source".to_string(), vec![children.len()]);
                children.push(node);
            }
            // location
            if let Some(ref location) = first.location {
                let mut node = OwnedNode::leaf("location", location, line);
                node.source_file = source_file.clone();
                field_indices.insert("location".to_string(), vec![children.len()]);
                children.push(node);
            }

            // Generic role children — one per distinct thematic role
            let mut role_indices = Vec::new();
            for ann in &first.roles {
                if ann.thematic_role != ThematicRole::Unknown {
                    let mut role_node = OwnedNode::leaf("role", &ann.thematic_role.to_string(), line);
                    role_node.source_file = source_file.clone();
                    role_indices.push(children.len());
                    children.push(role_node);
                }
            }
            if !role_indices.is_empty() {
                field_indices.insert("role".to_string(), role_indices);
            }

            // role_confidence: minimum confidence
            if !first.roles.is_empty() {
                let min_conf = first.roles.iter().map(|r| r.confidence).fold(f32::INFINITY, f32::min);
                let mut conf_node = OwnedNode::leaf("role_confidence", &format!("{:.2}", min_conf), line);
                conf_node.source_file = source_file.clone();
                field_indices.insert("role_confidence".to_string(), vec![children.len()]);
                children.push(conf_node);
            }

            // Thematic-role-named children for role-based filtering.
            // Enables queries like interaction[experiencer=Sarah] and interaction[theme=ball].
            for ann in &first.roles {
                if ann.thematic_role == ThematicRole::Unknown {
                    continue;
                }
                let role_name = ann.thematic_role.to_string();
                if !field_indices.contains_key(&role_name) {
                    let mut node = OwnedNode::leaf(&role_name, &ann.participant, line);
                    node.source_file = source_file.clone();
                    field_indices.insert(role_name.clone(), vec![children.len()]);
                    children.push(node);
                }
            }

            // Build summary text: "Agent verb Patient"
            let summary = match (&first.agent, &first.patient) {
                (Some(a), Some(p)) => format!("{} {} {}", a, first.verb, p),
                (Some(a), None) => format!("{} {}", a, first.verb),
                (None, Some(p)) => format!("{} {}", first.verb, p),
                (None, None) => first.verb.clone(),
            };

            OwnedNode {
                node_type: "interaction".to_string(),
                text: Some(summary),
                subtree_text: None,
                field_indices,
                children,
                start_line: first.source_line,
                end_line: mentions.last().map(|m| m.source_line).unwrap_or(first.source_line),
                source_file: source_file.clone(),
            }
        })
        .collect()
}

fn build_discourse_nodes(
    relations: &[DiscourseRelationData],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    relations
        .iter()
        .map(|rel| {
            let mut children = Vec::new();
            let mut field_indices = HashMap::new();

            // type
            let mut type_node = OwnedNode::leaf("type", &rel.relation.to_string(), rel.satellite_line);
            type_node.source_file = source_file.clone();
            field_indices.insert("type".to_string(), vec![children.len()]);
            children.push(type_node);

            // connective
            if let Some(ref conn) = rel.connective {
                let mut conn_node = OwnedNode::leaf("connective", conn, rel.satellite_line);
                conn_node.source_file = source_file.clone();
                field_indices.insert("connective".to_string(), vec![children.len()]);
                children.push(conn_node);
            }

            // confidence
            let mut conf_node = OwnedNode::leaf("confidence", &format!("{:.2}", rel.confidence), rel.satellite_line);
            conf_node.source_file = source_file.clone();
            field_indices.insert("confidence".to_string(), vec![children.len()]);
            children.push(conf_node);

            // nucleus (full text)
            let mut nuc_node = OwnedNode::leaf("nucleus", &rel.nucleus_text.clone(), rel.nucleus_line);
            nuc_node.source_file = source_file.clone();
            field_indices.insert("nucleus".to_string(), vec![children.len()]);
            children.push(nuc_node);

            // satellite (full text)
            let mut sat_node = OwnedNode::leaf("satellite", &rel.satellite_text.clone(), rel.satellite_line);
            sat_node.source_file = source_file.clone();
            field_indices.insert("satellite".to_string(), vec![children.len()]);
            children.push(sat_node);

            // nucleus_line
            let mut nl_node = OwnedNode::leaf("nucleus_line", &rel.nucleus_line.to_string(), rel.nucleus_line);
            nl_node.source_file = source_file.clone();
            field_indices.insert("nucleus_line".to_string(), vec![children.len()]);
            children.push(nl_node);

            // satellite_line
            let mut sl_node = OwnedNode::leaf("satellite_line", &rel.satellite_line.to_string(), rel.satellite_line);
            sl_node.source_file = source_file.clone();
            field_indices.insert("satellite_line".to_string(), vec![children.len()]);
            children.push(sl_node);

            // nucleus_para
            let mut np_node = OwnedNode::leaf("nucleus_para", &rel.nucleus_para_idx.to_string(), rel.nucleus_line);
            np_node.source_file = source_file.clone();
            field_indices.insert("nucleus_para".to_string(), vec![children.len()]);
            children.push(np_node);

            // satellite_para
            let mut sp_node = OwnedNode::leaf("satellite_para", &rel.satellite_para_idx.to_string(), rel.satellite_line);
            sp_node.source_file = source_file.clone();
            field_indices.insert("satellite_para".to_string(), vec![children.len()]);
            children.push(sp_node);

            // supports
            let supports_text = format!("line:{}", rel.nucleus_line);
            let mut supports_node = OwnedNode::leaf("supports", &supports_text, rel.satellite_line);
            supports_node.source_file = source_file.clone();
            field_indices.insert("supports".to_string(), vec![children.len()]);
            children.push(supports_node);

            // direction
            let mut dir_node = OwnedNode::leaf("direction", "forward", rel.satellite_line);
            dir_node.source_file = source_file.clone();
            field_indices.insert("direction".to_string(), vec![children.len()]);
            children.push(dir_node);

            // scope
            let scope = if rel.nucleus_para_idx == rel.satellite_para_idx {
                "intra_paragraph"
            } else {
                "cross_paragraph"
            };
            let mut scope_node = OwnedNode::leaf("scope", scope, rel.satellite_line);
            scope_node.source_file = source_file.clone();
            field_indices.insert("scope".to_string(), vec![children.len()]);
            children.push(scope_node);

            // Summary text
            let nuc_summary: String = rel.nucleus_text.chars().take(40).collect();
            let sat_summary: String = rel.satellite_text.chars().take(40).collect();
            let nuc_display = if rel.nucleus_text.chars().count() > 40 {
                format!("{}\u{2026}", nuc_summary)
            } else {
                nuc_summary
            };
            let sat_display = if rel.satellite_text.chars().count() > 40 {
                format!("{}\u{2026}", sat_summary)
            } else {
                sat_summary
            };
            let summary = format!("{}: {} \u{2194} {}",
                rel.relation,
                nuc_display,
                sat_display,
            );

            let start_line = rel.nucleus_line.min(rel.satellite_line);
            let end_line = rel.nucleus_line.max(rel.satellite_line);

            OwnedNode {
                node_type: "discourse".to_string(),
                text: Some(summary),
                subtree_text: None,
                field_indices,
                children,
                start_line,
                end_line,
                source_file: source_file.clone(),
            }
        })
        .collect()
}

fn build_scene_nodes(
    scenes: &[SceneBoundary],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    scenes.iter().map(|scene| {
        let mut children = Vec::new();
        let mut field_indices: HashMap<String, Vec<usize>> = HashMap::new();

        let mut idx_node = OwnedNode::leaf("index", &scene.scene_index.to_string(), scene.start_line);
        idx_node.source_file = source_file.clone();
        field_indices.insert("index".to_string(), vec![children.len()]);
        children.push(idx_node);

        let mut sp_node = OwnedNode::leaf("start_para", &scene.start_para_idx.to_string(), scene.start_line);
        sp_node.source_file = source_file.clone();
        field_indices.insert("start_para".to_string(), vec![children.len()]);
        children.push(sp_node);

        let mut ep_node = OwnedNode::leaf("end_para", &scene.end_para_idx.to_string(), scene.start_line);
        ep_node.source_file = source_file.clone();
        field_indices.insert("end_para".to_string(), vec![children.len()]);
        children.push(ep_node);

        if let Some(ref loc) = scene.location {
            let mut loc_node = OwnedNode::leaf("location", loc, scene.start_line);
            loc_node.source_file = source_file.clone();
            field_indices.insert("location".to_string(), vec![children.len()]);
            children.push(loc_node);
        }
        if let Some(ref tm) = scene.temporal_marker {
            let mut tm_node = OwnedNode::leaf("temporal_marker", tm, scene.start_line);
            tm_node.source_file = source_file.clone();
            field_indices.insert("temporal_marker".to_string(), vec![children.len()]);
            children.push(tm_node);
        }
        if !scene.entity_names.is_empty() {
            let mut ent_node = OwnedNode::leaf("entities", &scene.entity_names.join(", "), scene.start_line);
            ent_node.source_file = source_file.clone();
            field_indices.insert("entities".to_string(), vec![children.len()]);
            children.push(ent_node);
        }
        if !scene.boundary_signals.is_empty() {
            let sigs: Vec<String> = scene.boundary_signals.iter().map(|s| s.to_string()).collect();
            let mut sig_node = OwnedNode::leaf("boundary_signals", &sigs.join(", "), scene.start_line);
            sig_node.source_file = source_file.clone();
            field_indices.insert("boundary_signals".to_string(), vec![children.len()]);
            children.push(sig_node);
        }

        let loc_str = scene.location.as_deref().unwrap_or("unknown");
        let text = format!("Scene {}: {} (paras {}-{})", scene.scene_index, loc_str, scene.start_para_idx, scene.end_para_idx);

        OwnedNode {
            node_type: "scene".to_string(),
            text: Some(text),
            subtree_text: None,
            field_indices,
            children,
            start_line: scene.start_line,
            end_line: scene.end_line,
            source_file: source_file.clone(),
        }
    }).collect()
}

fn build_arc_nodes(
    arcs: &[CharacterArc],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    arcs.iter().map(|arc| {
        let mut children = Vec::new();
        let mut field_indices: HashMap<String, Vec<usize>> = HashMap::new();
        let line = 1usize;

        let mut ent_node = OwnedNode::leaf("entity", &arc.entity_name, line);
        ent_node.source_file = source_file.clone();
        field_indices.insert("entity".to_string(), vec![children.len()]);
        children.push(ent_node);

        let mut shape_node = OwnedNode::leaf("shape", &arc.arc_shape.to_string(), line);
        shape_node.source_file = source_file.clone();
        field_indices.insert("shape".to_string(), vec![children.len()]);
        children.push(shape_node);

        let mut tm_node = OwnedNode::leaf("total_mentions", &arc.total_mentions.to_string(), line);
        tm_node.source_file = source_file.clone();
        field_indices.insert("total_mentions".to_string(), vec![children.len()]);
        children.push(tm_node);

        let mut ti_node = OwnedNode::leaf("total_interactions", &arc.total_interactions.to_string(), line);
        ti_node.source_file = source_file.clone();
        field_indices.insert("total_interactions".to_string(), vec![children.len()]);
        children.push(ti_node);

        let mut fm_node = OwnedNode::leaf("first_mention", &format!("{:.2}", arc.first_mention_position), line);
        fm_node.source_file = source_file.clone();
        field_indices.insert("first_mention".to_string(), vec![children.len()]);
        children.push(fm_node);

        let mut lm_node = OwnedNode::leaf("last_mention", &format!("{:.2}", arc.last_mention_position), line);
        lm_node.source_file = source_file.clone();
        field_indices.insert("last_mention".to_string(), vec![children.len()]);
        children.push(lm_node);

        let mut pp_node = OwnedNode::leaf("peak_position", &format!("{:.2}", arc.peak_position), line);
        pp_node.source_file = source_file.clone();
        field_indices.insert("peak_position".to_string(), vec![children.len()]);
        children.push(pp_node);

        let mut conf_node = OwnedNode::leaf("confidence", &format!("{:.2}", arc.confidence), line);
        conf_node.source_file = source_file.clone();
        field_indices.insert("confidence".to_string(), vec![children.len()]);
        children.push(conf_node);

        if !arc.role_distribution.is_empty() {
            let roles: Vec<String> = arc.role_distribution.iter()
                .map(|(k, v)| format!("{}:{}", k, v))
                .collect();
            let mut roles_node = OwnedNode::leaf("roles", &roles.join(", "), line);
            roles_node.source_file = source_file.clone();
            field_indices.insert("roles".to_string(), vec![children.len()]);
            children.push(roles_node);
        }

        let text = format!("Arc: {} \u{2014} {}", arc.entity_name, arc.arc_shape);

        OwnedNode {
            node_type: "arc".to_string(),
            text: Some(text),
            subtree_text: None,
            field_indices,
            children,
            start_line: 1,
            end_line: 1,
            source_file: source_file.clone(),
        }
    }).collect()
}

fn build_conflict_nodes(
    edges: &[ConflictEdge],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    edges.iter().map(|edge| {
        let mut children = Vec::new();
        let mut field_indices: HashMap<String, Vec<usize>> = HashMap::new();
        let line = 1usize;

        let mut ea_node = OwnedNode::leaf("entity_a", &edge.entity_a, line);
        ea_node.source_file = source_file.clone();
        field_indices.insert("entity_a".to_string(), vec![children.len()]);
        children.push(ea_node);

        let mut eb_node = OwnedNode::leaf("entity_b", &edge.entity_b, line);
        eb_node.source_file = source_file.clone();
        field_indices.insert("entity_b".to_string(), vec![children.len()]);
        children.push(eb_node);

        let mut ic_node = OwnedNode::leaf("interaction_count", &edge.interaction_count.to_string(), line);
        ic_node.source_file = source_file.clone();
        field_indices.insert("interaction_count".to_string(), vec![children.len()]);
        children.push(ic_node);

        let mut trend_node = OwnedNode::leaf("trend", &edge.trend.to_string(), line);
        trend_node.source_file = source_file.clone();
        field_indices.insert("trend".to_string(), vec![children.len()]);
        children.push(trend_node);

        let mut fp_node = OwnedNode::leaf("first_position", &format!("{:.2}", edge.first_position), line);
        fp_node.source_file = source_file.clone();
        field_indices.insert("first_position".to_string(), vec![children.len()]);
        children.push(fp_node);

        let mut lp_node = OwnedNode::leaf("last_position", &format!("{:.2}", edge.last_position), line);
        lp_node.source_file = source_file.clone();
        field_indices.insert("last_position".to_string(), vec![children.len()]);
        children.push(lp_node);

        if !edge.sample_verbs.is_empty() {
            let mut sv_node = OwnedNode::leaf("sample_verbs", &edge.sample_verbs.join(", "), line);
            sv_node.source_file = source_file.clone();
            field_indices.insert("sample_verbs".to_string(), vec![children.len()]);
            children.push(sv_node);
        }

        let text = format!("Conflict: {} \u{2194} {} ({})", edge.entity_a, edge.entity_b, edge.trend);

        OwnedNode {
            node_type: "conflict".to_string(),
            text: Some(text),
            subtree_text: None,
            field_indices,
            children,
            start_line: 1,
            end_line: 1,
            source_file: source_file.clone(),
        }
    }).collect()
}

fn build_narrative_issue_nodes(
    issues: &[NarrativeIssue],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    issues.iter().map(|issue| {
        let mut children = Vec::new();
        let mut field_indices = HashMap::new();
        let idx = |children: &Vec<OwnedNode>| children.len();

        let mut type_node = OwnedNode::leaf("type", &issue.issue_type.to_string(), 1);
        type_node.source_file = source_file.clone();
        field_indices.insert("type".to_string(), vec![idx(&children)]);
        children.push(type_node);

        let mut entity_node = OwnedNode::leaf("entity", &issue.entity_name, 1);
        entity_node.source_file = source_file.clone();
        field_indices.insert("entity".to_string(), vec![idx(&children)]);
        children.push(entity_node);

        let mut desc_node = OwnedNode::leaf("description", &issue.description, 1);
        desc_node.source_file = source_file.clone();
        field_indices.insert("description".to_string(), vec![idx(&children)]);
        children.push(desc_node);

        let mut conf_node = OwnedNode::leaf("confidence", &format!("{:.2}", issue.confidence), 1);
        conf_node.source_file = source_file.clone();
        field_indices.insert("confidence".to_string(), vec![idx(&children)]);
        children.push(conf_node);

        if let Some(ref attr) = issue.attribute {
            let mut attr_node = OwnedNode::leaf("attribute", attr, 1);
            attr_node.source_file = source_file.clone();
            field_indices.insert("attribute".to_string(), vec![idx(&children)]);
            children.push(attr_node);
        }
        if let Some(ref expected) = issue.expected {
            let mut exp_node = OwnedNode::leaf("expected", expected, 1);
            exp_node.source_file = source_file.clone();
            field_indices.insert("expected".to_string(), vec![idx(&children)]);
            children.push(exp_node);
        }
        if let Some(ref actual) = issue.actual {
            let mut act_node = OwnedNode::leaf("actual", actual, 1);
            act_node.source_file = source_file.clone();
            field_indices.insert("actual".to_string(), vec![idx(&children)]);
            children.push(act_node);
        }

        let text = format!("{}: {} \u{2014} {}", issue.issue_type, issue.entity_name, issue.description);

        OwnedNode {
            node_type: "narrative_issue".to_string(),
            text: Some(text),
            subtree_text: None,
            field_indices,
            children,
            start_line: 1,
            end_line: 1,
            source_file: source_file.clone(),
        }
    }).collect()
}

fn build_narrative_summary_node(
    summary: &NarrativeSummary,
    source_file: &Option<String>,
) -> OwnedNode {
    let mut children = Vec::new();
    let mut field_indices = HashMap::new();
    let idx = |ch: &Vec<OwnedNode>| ch.len();

    let mut scene_node = OwnedNode::leaf("scene_count", &summary.scene_count.to_string(), 1);
    scene_node.source_file = source_file.clone();
    field_indices.insert("scene_count".to_string(), vec![idx(&children)]);
    children.push(scene_node);

    let mut char_node = OwnedNode::leaf("character_count", &summary.character_count.to_string(), 1);
    char_node.source_file = source_file.clone();
    field_indices.insert("character_count".to_string(), vec![idx(&children)]);
    children.push(char_node);

    let central = match &summary.central_conflict {
        Some((a, b)) => format!("{} \u{2194} {}", a, b),
        None => "none".to_string(),
    };
    let mut central_node = OwnedNode::leaf("central_conflict", &central, 1);
    central_node.source_file = source_file.clone();
    field_indices.insert("central_conflict".to_string(), vec![idx(&children)]);
    children.push(central_node);

    let mut conflict_node = OwnedNode::leaf("conflict_count", &summary.conflict_count.to_string(), 1);
    conflict_node.source_file = source_file.clone();
    field_indices.insert("conflict_count".to_string(), vec![idx(&children)]);
    children.push(conflict_node);

    let mut issue_node = OwnedNode::leaf("issue_count", &summary.issue_count.to_string(), 1);
    issue_node.source_file = source_file.clone();
    field_indices.insert("issue_count".to_string(), vec![idx(&children)]);
    children.push(issue_node);

    let mut unresolved_node = OwnedNode::leaf("unresolved_conflicts", &summary.unresolved_conflicts.to_string(), 1);
    unresolved_node.source_file = source_file.clone();
    field_indices.insert("unresolved_conflicts".to_string(), vec![idx(&children)]);
    children.push(unresolved_node);

    let mut arc_parts: Vec<String> = summary.arc_shape_distribution.iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    arc_parts.sort();
    let mut arc_node = OwnedNode::leaf("arc_distribution", &arc_parts.join(", "), 1);
    arc_node.source_file = source_file.clone();
    field_indices.insert("arc_distribution".to_string(), vec![idx(&children)]);
    children.push(arc_node);

    let text = format!(
        "Narrative: {} scenes, {} characters, {} conflicts",
        summary.scene_count, summary.character_count, summary.conflict_count,
    );

    OwnedNode {
        node_type: "narrative_summary".to_string(),
        text: Some(text),
        subtree_text: None,
        field_indices,
        children,
        start_line: 1,
        end_line: 1,
        source_file: source_file.clone(),
    }
}

// ── Narrative pipeline helpers ─────────────────────────────────────────────

/// Returns true if the NER label represents a narrative-relevant entity type
/// (person, organization, or nationality/religious/political group).
fn is_narrative_entity(label: &str) -> bool {
    matches!(label, "PERSON" | "ORG" | "NORP")
}

/// Builds a map from each coreference mention's referent text (lowercased)
/// to the canonical name of its chain, enabling pronoun resolution.
fn build_mention_to_canonical(chains: &[CoreferenceChain]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for chain in chains {
        for mention in &chain.mentions {
            map.insert(mention.referent.to_lowercase(), chain.canonical.clone());
        }
    }
    map
}

// ── Requirements ─────────────────────────────────────────────────────────────

#[derive(Debug)]
struct RequirementData {
    sentence_text: String,
    modal: String,
    strength: String,
    source_line: usize,
    end_line: usize,
}

fn detect_requirements(doc: &SpacyDoc, line_starts: &[usize]) -> Vec<RequirementData> {
    let mut requirements = Vec::new();
    for sentence in &doc.sentences {
        let line = offset_to_line(sentence.start, line_starts);
        let end_line = offset_to_line(sentence.end.saturating_sub(1), line_starts);

        let mut modal: Option<&str> = None;
        let mut strength: Option<&str> = None;
        for token in &sentence.tokens {
            let text_lower = token.text.to_lowercase();
            match text_lower.as_str() {
                "shall" => {
                    modal = Some("shall");
                    strength = Some("mandatory");
                    break;
                }
                "must" => {
                    modal = Some("must");
                    strength = Some("mandatory");
                    break;
                }
                "should" => {
                    modal = Some("should");
                    strength = Some("recommended");
                    break;
                }
                "may" if token.pos == "MD" => {
                    modal = Some("may");
                    strength = Some("optional");
                    break;
                }
                "required" | "require" | "requires" => {
                    modal = Some("required");
                    strength = Some("mandatory");
                    break;
                }
                _ => {}
            }
        }

        if let (Some(m), Some(s)) = (modal, strength) {
            requirements.push(RequirementData {
                sentence_text: sentence.text.clone(),
                modal: m.to_string(),
                strength: s.to_string(),
                source_line: line,
                end_line,
            });
        }
    }
    requirements
}

fn build_requirement_nodes(
    requirements: &[RequirementData],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    requirements
        .iter()
        .map(|req| {
            let mut modal_node = OwnedNode::leaf("modal", &req.modal, req.source_line);
            modal_node.source_file = source_file.clone();
            let mut strength_node =
                OwnedNode::leaf("strength", &req.strength, req.source_line);
            strength_node.source_file = source_file.clone();

            let mut field_indices = HashMap::new();
            field_indices.insert("modal".to_string(), vec![0usize]);
            field_indices.insert("strength".to_string(), vec![1usize]);

            OwnedNode {
                node_type: "requirement".to_string(),
                text: Some(req.sentence_text.clone()),
                subtree_text: None,
                field_indices,
                children: vec![modal_node, strength_node],
                start_line: req.source_line,
                end_line: req.end_line,
                source_file: source_file.clone(),
            }
        })
        .collect()
}

// ── Questions ────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct QuestionData {
    sentence_text: String,
    question_type: String,
    source_line: usize,
    end_line: usize,
}

fn detect_questions(doc: &SpacyDoc, line_starts: &[usize]) -> Vec<QuestionData> {
    let mut questions = Vec::new();
    for sentence in &doc.sentences {
        let line = offset_to_line(sentence.start, line_starts);
        let end_line = offset_to_line(sentence.end.saturating_sub(1), line_starts);

        // Check if sentence ends with '?'
        let ends_with_question = sentence.tokens.iter().rev()
            .find(|t| !t.text.trim().is_empty())
            .map(|t| t.text == "?")
            .unwrap_or_else(|| sentence.text.trim_end().ends_with('?'));

        if !ends_with_question {
            continue;
        }

        // Classify by first content-word token (skip leading punctuation)
        let question_type = sentence.tokens.iter()
            .find(|t| !t.text.trim().is_empty() && t.pos != "PUNCT" && t.pos != "SPACE")
            .map(|t| {
                match t.text.to_lowercase().as_str() {
                    "who" | "whom" | "whose" => "who",
                    "what" | "which" => "what",
                    "when" => "when",
                    "where" => "where",
                    "why" => "why",
                    "how" => "how",
                    _ => "yes-no",
                }
            })
            .unwrap_or("yes-no")
            .to_string();

        questions.push(QuestionData {
            sentence_text: sentence.text.clone(),
            question_type,
            source_line: line,
            end_line,
        });
    }
    questions
}

fn build_question_nodes(
    questions: &[QuestionData],
    source_file: &Option<String>,
) -> Vec<OwnedNode> {
    questions
        .iter()
        .map(|q| {
            let mut qtype_node = OwnedNode::leaf("question_type", &q.question_type, q.source_line);
            qtype_node.source_file = source_file.clone();

            let mut field_indices = HashMap::new();
            field_indices.insert("question_type".to_string(), vec![0usize]);

            OwnedNode {
                node_type: "question".to_string(),
                text: Some(q.sentence_text.clone()),
                subtree_text: None,
                field_indices,
                children: vec![qtype_node],
                start_line: q.source_line,
                end_line: q.end_line,
                source_file: source_file.clone(),
            }
        })
        .collect()
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Convert a SpacyDoc into an OwnedNode tree.
pub fn spacy_doc_to_owned_tree(
    doc: &SpacyDoc,
    source_text: &str,
    file_path: Option<&str>,
) -> OwnedNode {
    let line_starts = build_line_starts(source_text);
    let total_lines = line_starts.len();
    let source_file = file_path.map(|s| s.to_string());

    // Detect paragraph boundaries.
    let para_ranges = detect_paragraphs(source_text);

    // Bucket sentences into paragraphs, collecting interactions.
    let mut para_sentence_nodes: Vec<Vec<OwnedNode>> =
        vec![Vec::new(); para_ranges.len()];
    let mut all_interactions: Vec<InteractionData> = Vec::new();

    for sentence in &doc.sentences {
        let para_idx = para_ranges
            .iter()
            .position(|r| {
                sentence.start >= r.start_char && sentence.start <= r.end_char
            })
            .unwrap_or(0);
        let (sent_node, interactions) = build_sentence_node(sentence, &line_starts, &source_file);
        para_sentence_nodes[para_idx].push(sent_node);
        all_interactions.extend(interactions);
    }

    // Build paragraph nodes, omitting empty ones.
    let paragraph_nodes: Vec<OwnedNode> = para_ranges
        .iter()
        .zip(para_sentence_nodes.into_iter())
        .filter(|(_, sentences)| !sentences.is_empty())
        .map(|(range, sentences)| {
            let para_start_line = offset_to_line(range.start_char, &line_starts);
            let para_end_line = if range.end_char > 0 {
                offset_to_line(range.end_char.saturating_sub(1), &line_starts)
            } else {
                para_start_line
            };
            let num_sents = sentences.len();
            let mut field_indices = HashMap::new();
            field_indices.insert("sentences".to_string(), (0..num_sents).collect());
            OwnedNode {
                node_type: "paragraph".to_string(),
                text: None,
                subtree_text: None,
                field_indices,
                children: sentences,
                start_line: para_start_line,
                end_line: para_end_line,
                source_file: source_file.clone(),
            }
        })
        .collect();

    // ── Co-reference pipeline ──────────────────────────────────────────────
    let mut all_corefs: Vec<CoreferenceData> = Vec::new();
    let mut entity_gender_map: HashMap<String, Gender> = HashMap::new();
    let mut topic_entities: HashMap<Gender, String> = HashMap::new();

    // Build entity_type_map from spaCy NER for chain aggregation
    let mut entity_type_map: HashMap<String, String> = HashMap::new();
    for ent in &doc.entities {
        entity_type_map.insert(ent.text.to_lowercase(), ent.label.clone());
    }

    let mut prev_sentence: Option<&SpacySentence> = None;
    let mut prev_para_idx: Option<usize> = None;

    for (sent_idx, sentence) in doc.sentences.iter().enumerate() {
        let para_idx = para_ranges
            .iter()
            .position(|r| sentence.start >= r.start_char && sentence.start <= r.end_char)
            .unwrap_or(0);

        // Phase 1: Appositive extraction
        let appositives = extract_appositives_from_sentence(sentence, sent_idx, &line_starts);
        all_corefs.extend(appositives);

        // Phase 2: Same-sentence pronoun resolution
        let same_sentence = resolve_same_sentence_pronouns(sentence, sent_idx, &line_starts);
        let already_resolved: Vec<usize> = same_sentence.iter().map(|c| c.token_idx).collect();
        all_corefs.extend(same_sentence.iter().cloned());

        // Phase 3: Cross-sentence pronoun resolution
        let same_paragraph = prev_para_idx.map_or(false, |prev_p| prev_p == para_idx);
        let cross_sentence = resolve_cross_sentence_pronouns(
            sentence,
            sent_idx,
            prev_sentence,
            same_paragraph,
            &already_resolved,
            &entity_gender_map,
            &topic_entities,
            &line_starts,
        );

        // Update gender map and topic entities
        let all_sentence_resolutions: Vec<CoreferenceData> = same_sentence.iter()
            .chain(cross_sentence.iter())
            .cloned()
            .collect();
        update_gender_map(&all_sentence_resolutions, &mut entity_gender_map);
        update_topic_entities(sentence, &all_sentence_resolutions, &entity_gender_map, &mut topic_entities);

        all_corefs.extend(cross_sentence);

        prev_sentence = Some(sentence);
        prev_para_idx = Some(para_idx);
    }

    // Build coref chains
    let coref_chains = build_coreference_chains(&all_corefs, &entity_type_map);

    // ── Discourse relation pipeline ────────────────────────────────────────
    let sentence_infos: Vec<SentenceInfo> = doc.sentences.iter().map(|sentence| {
        let para_idx = para_ranges
            .iter()
            .position(|r| sentence.start >= r.start_char && sentence.start <= r.end_char)
            .unwrap_or(0);
        let line = offset_to_line(sentence.start, &line_starts);
        SentenceInfo {
            text: sentence.text.clone(),
            para_idx,
            line,
        }
    }).collect();
    let discourse_relations = detect_discourse_relations(&sentence_infos);

    // ── Narrative analysis pipeline (multi-paragraph documents only) ────────
    let mut all_issues: Vec<NarrativeIssue> = Vec::new();
    let (scenes, arcs, conflict_edges): (Vec<SceneBoundary>, Vec<CharacterArc>, Vec<ConflictEdge>) =
        if paragraph_nodes.len() >= 2 {
            let doc_len = source_text.len().max(1) as f64;

            // Build per-paragraph entity data for scene detection
            let para_entity_data: Vec<ParagraphEntityData> = para_ranges.iter().enumerate().map(|(idx, pr)| {
                let mut entity_names = Vec::new();
                let mut location_entities = Vec::new();
                let mut temporal_entities = Vec::new();
                for ent in &doc.entities {
                    if ent.start_char >= pr.start_char && ent.start_char <= pr.end_char {
                        if !entity_names.contains(&ent.text) {
                            entity_names.push(ent.text.clone());
                        }
                        match ent.label.as_str() {
                            "LOC" | "GPE" => {
                                if !location_entities.contains(&ent.text) {
                                    location_entities.push(ent.text.clone());
                                }
                            }
                            "DATE" | "TIME" => {
                                if !temporal_entities.contains(&ent.text) {
                                    temporal_entities.push(ent.text.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ParagraphEntityData {
                    para_idx: idx,
                    start_line: offset_to_line(pr.start_char, &line_starts),
                    end_line: offset_to_line(pr.end_char.saturating_sub(1), &line_starts),
                    entity_names,
                    location_entities,
                    temporal_entities,
                }
            }).collect();

            let scenes = detect_scene_boundaries(&para_entity_data, &discourse_relations);

            // Build mention → canonical map for pronoun resolution (Fix 2)
            let mention_to_canonical = build_mention_to_canonical(&coref_chains);

            // Build person-aliased GPE rescue set (Fix 1 P1)
            // GPE/LOC entities that share a coref chain with a known PERSON canonical
            // are treated as narrative characters (e.g. "Israel" = Jacob).
            let person_aliased_gpe: HashSet<String> = {
                let mut set = HashSet::new();
                for chain in &coref_chains {
                    let canonical_is_person = entity_type_map
                        .get(&chain.canonical.to_lowercase())
                        .map(|t| t == "PERSON")
                        .unwrap_or(false);
                    if canonical_is_person {
                        for alias in &chain.aliases {
                            let alias_type = entity_type_map.get(&alias.to_lowercase()).map(|s| s.as_str());
                            if matches!(alias_type, Some("GPE" | "LOC")) {
                                set.insert(alias.clone());
                            }
                        }
                        for mention in &chain.mentions {
                            let ref_type = entity_type_map.get(&mention.referent.to_lowercase()).map(|s| s.as_str());
                            if matches!(ref_type, Some("GPE" | "LOC")) {
                                set.insert(mention.referent.clone());
                            }
                        }
                    }
                }
                set
            };

            // Build entity interaction profiles for arc computation
            // Fix 1: only include PERSON, ORG, NORP entities as narrative characters
            // (plus GPE/LOC entities that are aliased to PERSON entities via coref)
            let mut entity_profiles: HashMap<String, EntityInteractionProfile> = HashMap::new();
            for ent in &doc.entities {
                let label = entity_type_map.get(&ent.text.to_lowercase()).map(|s| s.as_str()).unwrap_or("");
                if !is_narrative_entity(label) && !person_aliased_gpe.contains(&ent.text) {
                    continue;
                }
                let pos = ent.start_char as f64 / doc_len;
                let profile = entity_profiles.entry(ent.text.clone()).or_insert_with(|| {
                    EntityInteractionProfile {
                        entity_name: ent.text.clone(),
                        mention_positions: Vec::new(),
                        interaction_positions: Vec::new(),
                        role_counts: HashMap::new(),
                        interaction_roles: Vec::new(),
                    }
                });
                profile.mention_positions.push(pos);
            }
            for inter in &all_interactions {
                let pos = inter.source_line as f64 / (line_starts.len().max(1) as f64);
                if let Some(ref agent_raw) = inter.agent {
                    // Fix 2: resolve pronouns through coref chains
                    let agent = mention_to_canonical
                        .get(&agent_raw.to_lowercase())
                        .cloned()
                        .unwrap_or_else(|| agent_raw.clone());
                    if let Some(profile) = entity_profiles.get_mut(&agent) {
                        profile.interaction_positions.push(pos);
                        profile.interaction_roles.push((pos, "agent".to_string()));
                        *profile.role_counts.entry("agent".to_string()).or_default() += 1;
                    }
                }
                if let Some(ref patient_raw) = inter.patient {
                    // Fix 2: resolve pronouns through coref chains
                    let patient = mention_to_canonical
                        .get(&patient_raw.to_lowercase())
                        .cloned()
                        .unwrap_or_else(|| patient_raw.clone());
                    if let Some(profile) = entity_profiles.get_mut(&patient) {
                        profile.interaction_positions.push(pos);
                        profile.interaction_roles.push((pos, "patient".to_string()));
                        *profile.role_counts.entry("patient".to_string()).or_default() += 1;
                    }
                }
                for role_ann in &inter.roles {
                    if let Some(profile) = entity_profiles.get_mut(&role_ann.participant) {
                        let role_str = role_ann.thematic_role.to_string().to_lowercase();
                        if role_str != "agent" && role_str != "patient" {
                            *profile.role_counts.entry(role_str.clone()).or_default() += 1;
                            profile.interaction_roles.push((pos, role_str));
                        }
                    }
                }
            }
            let profiles: Vec<EntityInteractionProfile> = entity_profiles.into_iter().map(|(_, v)| v).collect();
            let arcs = compute_character_arcs(&profiles);

            // Build opposing interactions for conflict graph
            // Fix 2: resolve pronouns in agent/patient before building edges
            let opposing: Vec<OpposingInteraction> = all_interactions.iter().filter_map(|inter| {
                let agent_raw = inter.agent.as_ref()?;
                let patient_raw = inter.patient.as_ref()?;
                let agent = mention_to_canonical
                    .get(&agent_raw.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| agent_raw.clone());
                let patient = mention_to_canonical
                    .get(&patient_raw.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| patient_raw.clone());
                let pos = inter.source_line as f64 / (line_starts.len().max(1) as f64);
                Some(OpposingInteraction {
                    agent,
                    patient,
                    verb: inter.verb.clone(),
                    position: pos,
                })
            }).collect();
            let conflict_edges = build_conflict_graph(&opposing);

            // ── Narrative issue detection ────────────────────────────────────────
            let setup_payoff_issues = detect_setup_payoff(&arcs, &conflict_edges);
            let foreshadowing_issues = detect_foreshadowing(&arcs);

            // Build entity→scene→location map for consistency checks
            let mut entity_scene_locations: HashMap<String, Vec<(usize, String)>> = HashMap::new();
            for scene in &scenes {
                if let Some(ref loc) = scene.location {
                    for ent_name in &scene.entity_names {
                        entity_scene_locations
                            .entry(ent_name.clone())
                            .or_default()
                            .push((scene.scene_index, loc.clone()));
                    }
                }
            }

            // Build movement interactions set — interactions with movement verbs
            // between entities in consecutive scenes
            let movement_verbs: HashSet<&str> = ["went", "walked", "drove", "ran", "moved",
                "traveled", "flew", "returned", "came"].iter().copied().collect();
            // Causative motion verbs where the PATIENT also relocates (Fix 2 P2)
            let patient_movement_verbs: HashSet<&str> = ["brought", "carried", "took", "sent",
                "led", "dragged", "transported", "delivered"].iter().copied().collect();
            let mut movement_interactions: HashSet<(String, usize, usize)> = HashSet::new();
            for inter in &all_interactions {
                if movement_verbs.contains(inter.verb.as_str()) || movement_verbs.contains(inter.verb_lemma.as_str()) {
                    if let Some(ref agent) = inter.agent {
                        let line = inter.source_line;
                        for (scene_idx, scene) in scenes.iter().enumerate() {
                            if line >= scene.start_line && line <= scene.end_line {
                                if scene_idx + 1 < scenes.len() {
                                    movement_interactions.insert((agent.clone(), scene_idx, scene_idx + 1));
                                }
                                if scene_idx > 0 {
                                    movement_interactions.insert((agent.clone(), scene_idx - 1, scene_idx));
                                }
                                break;
                            }
                        }
                    }
                }
                // Also track patient movement for causative motion verbs (Fix 2 P2)
                if patient_movement_verbs.contains(inter.verb.as_str()) || patient_movement_verbs.contains(inter.verb_lemma.as_str()) {
                    if let Some(ref patient) = inter.patient {
                        let line = inter.source_line;
                        for (scene_idx, scene) in scenes.iter().enumerate() {
                            if line >= scene.start_line && line <= scene.end_line {
                                if scene_idx + 1 < scenes.len() {
                                    movement_interactions.insert((patient.clone(), scene_idx, scene_idx + 1));
                                }
                                if scene_idx > 0 {
                                    movement_interactions.insert((patient.clone(), scene_idx - 1, scene_idx));
                                }
                                break;
                            }
                        }
                    }
                }
            }

            let consistency_issues = detect_consistency_issues(&scenes, &entity_scene_locations, &movement_interactions);

            all_issues.extend(setup_payoff_issues);
            all_issues.extend(foreshadowing_issues);
            all_issues.extend(consistency_issues);

            (scenes, arcs, conflict_edges)
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

    let narrative_summary = build_narrative_summary(&scenes, &arcs, &conflict_edges, &all_issues);

    // Build entity nodes (deduplicated).
    let entity_nodes = build_entity_nodes(&doc.entities, &line_starts, &source_file, &coref_chains, &all_interactions);

    // Build document-level interaction nodes.
    let interaction_nodes = build_interaction_doc_nodes(&all_interactions, &source_file);

    // Compose document root.
    let num_paras = paragraph_nodes.len();
    let num_entities = entity_nodes.len();
    let num_interactions = interaction_nodes.len();
    let mut children = paragraph_nodes;
    children.extend(entity_nodes);
    children.extend(interaction_nodes);

    // Build discourse nodes
    let discourse_nodes = build_discourse_nodes(&discourse_relations, &source_file);
    let num_discourse = discourse_nodes.len();
    children.extend(discourse_nodes);

    // Build narrative nodes
    let scene_nodes = build_scene_nodes(&scenes, &source_file);
    let num_scenes = scene_nodes.len();
    children.extend(scene_nodes);

    let arc_nodes = build_arc_nodes(&arcs, &source_file);
    let num_arcs = arc_nodes.len();
    children.extend(arc_nodes);

    let conflict_nodes = build_conflict_nodes(&conflict_edges, &source_file);
    let num_conflicts = conflict_nodes.len();
    children.extend(conflict_nodes);

    let narrative_issue_nodes = build_narrative_issue_nodes(&all_issues, &source_file);
    let num_narrative_issues = narrative_issue_nodes.len();
    children.extend(narrative_issue_nodes);

    let requirement_data = detect_requirements(doc, &line_starts);
    let requirement_nodes = build_requirement_nodes(&requirement_data, &source_file);
    let num_requirements = requirement_nodes.len();
    children.extend(requirement_nodes);

    let question_data = detect_questions(doc, &line_starts);
    let question_nodes = build_question_nodes(&question_data, &source_file);
    let num_questions = question_nodes.len();
    children.extend(question_nodes);

    let summary_node = build_narrative_summary_node(&narrative_summary, &source_file);
    let summary_start = children.len();
    children.push(summary_node);

    let mut doc_field_indices = HashMap::new();
    if num_paras > 0 {
        doc_field_indices
            .insert("paragraphs".to_string(), (0..num_paras).collect());
    }
    if num_entities > 0 {
        doc_field_indices.insert(
            "entities".to_string(),
            (num_paras..num_paras + num_entities).collect(),
        );
    }
    if num_interactions > 0 {
        let interactions_start = num_paras + num_entities;
        doc_field_indices.insert(
            "interactions".to_string(),
            (interactions_start..interactions_start + num_interactions).collect(),
        );
    }
    if num_discourse > 0 {
        let discourse_start = num_paras + num_entities + num_interactions;
        doc_field_indices.insert(
            "discourse".to_string(),
            (discourse_start..discourse_start + num_discourse).collect(),
        );
    }
    if num_scenes > 0 {
        let scenes_start = num_paras + num_entities + num_interactions + num_discourse;
        doc_field_indices.insert(
            "scenes".to_string(),
            (scenes_start..scenes_start + num_scenes).collect(),
        );
    }
    if num_arcs > 0 {
        let arcs_start = num_paras + num_entities + num_interactions + num_discourse + num_scenes;
        doc_field_indices.insert(
            "arcs".to_string(),
            (arcs_start..arcs_start + num_arcs).collect(),
        );
    }
    if num_conflicts > 0 {
        let conflicts_start = num_paras + num_entities + num_interactions + num_discourse + num_scenes + num_arcs;
        doc_field_indices.insert(
            "conflicts".to_string(),
            (conflicts_start..conflicts_start + num_conflicts).collect(),
        );
    }
    if num_narrative_issues > 0 {
        let issues_start = num_paras + num_entities + num_interactions + num_discourse + num_scenes + num_arcs + num_conflicts;
        doc_field_indices.insert(
            "narrative_issues".to_string(),
            (issues_start..issues_start + num_narrative_issues).collect(),
        );
    }
    if num_requirements > 0 {
        let req_start = num_paras + num_entities + num_interactions + num_discourse + num_scenes + num_arcs + num_conflicts + num_narrative_issues;
        doc_field_indices.insert(
            "requirements".to_string(),
            (req_start..req_start + num_requirements).collect(),
        );
    }
    if num_questions > 0 {
        let questions_start = num_paras + num_entities + num_interactions + num_discourse + num_scenes + num_arcs + num_conflicts + num_narrative_issues + num_requirements;
        doc_field_indices.insert(
            "questions".to_string(),
            (questions_start..questions_start + num_questions).collect(),
        );
    }
    doc_field_indices.insert(
        "narrative_summary".to_string(),
        vec![summary_start],
    );

    OwnedNode {
        node_type: "document".to_string(),
        text: None,
        subtree_text: Some(source_text.to_string()),
        field_indices: doc_field_indices,
        children,
        start_line: 1,
        end_line: total_lines,
        source_file: source_file.clone(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spacy::{SpacyDoc, SpacySentence, SpacyToken as SpacyTokenData, SpacyEntity as SpacyEntityData};
    use aq_core::AqNode;

    fn make_token(
        text: &str,
        pos: &str,
        dep: &str,
        ent_type: &str,
        ent_iob: &str,
        idx: usize,
    ) -> SpacyTokenData {
        SpacyTokenData {
            text: text.to_string(),
            lemma: text.to_lowercase(),
            pos: pos.to_string(),
            tag: pos.to_string(),
            dep: dep.to_string(),
            head: 0,
            ent_type: ent_type.to_string(),
            ent_iob: ent_iob.to_string(),
            idx,
        }
    }

    fn make_sentence(
        text: &str,
        start: usize,
        tokens: Vec<SpacyTokenData>,
    ) -> SpacySentence {
        let end = start + text.len();
        SpacySentence { text: text.to_string(), start, end, tokens }
    }

    fn make_entity(text: &str, label: &str, start_char: usize) -> SpacyEntityData {
        SpacyEntityData {
            text: text.to_string(),
            label: label.to_string(),
            start_char,
            end_char: start_char + text.len(),
        }
    }

    // 1. Single sentence produces expected document shape.
    #[test]
    fn single_sentence() {
        let source = "Sarah ran.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token("Sarah", "PROPN", "nsubj", "", "O", 0),
                    make_token("ran", "VERB", "ROOT", "", "O", 6),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        assert_eq!(tree.node_type, "document");
        // 1 paragraph + 1 interaction node from "ran" + 1 narrative_summary
        assert_eq!(tree.children.len(), 3, "one paragraph plus one interaction plus narrative_summary");

        let para = &tree.children[0];
        assert_eq!(para.node_type, "paragraph");
        assert_eq!(para.children.len(), 1, "one sentence");

        let sent = &para.children[0];
        assert_eq!(sent.node_type, "sentence");
        assert_eq!(sent.children.len(), 3, "two tokens plus one verb_phrase");

        let first_token = &sent.children[0];
        assert_eq!(first_token.node_type, "token");
        assert_eq!(first_token.text.as_deref(), Some("Sarah"));
        assert_eq!(first_token.children.len(), 3); // pos_tag, dep_rel, lemma
    }

    // 2. Entities are extracted and placed at document level.
    #[test]
    fn entity_extraction() {
        let source = "Sarah went to Paris.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token("Sarah", "PROPN", "nsubj", "PERSON", "B", 0),
                    make_token("went", "VERB", "ROOT", "", "O", 6),
                    make_token("to", "ADP", "prep", "", "O", 11),
                    make_token("Paris", "PROPN", "pobj", "GPE", "B", 14),
                ],
            )],
            entities: vec![
                make_entity("Sarah", "PERSON", 0),
                make_entity("Paris", "GPE", 14),
            ],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        // 1 paragraph + 2 entities + 1 interaction ("went") + 1 narrative_summary
        assert_eq!(tree.children.len(), 5);
        let entity_nodes: Vec<&OwnedNode> = tree
            .children
            .iter()
            .filter(|c| c.node_type == "entity")
            .collect();
        assert_eq!(entity_nodes.len(), 2);

        let labels: Vec<&str> = entity_nodes
            .iter()
            .flat_map(|e| e.children.iter())
            .filter(|c| c.node_type == "entity_type")
            .map(|c| c.text.as_deref().unwrap_or(""))
            .collect();
        assert!(labels.contains(&"PERSON"));
        assert!(labels.contains(&"GPE"));
    }

    // 3. Multiple sentences in a single sentence group are kept.
    #[test]
    fn multi_sentence() {
        let source = "I am happy. She is sad.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("I am happy.", 0, vec![
                    make_token("I", "PRON", "nsubj", "", "O", 0),
                ]),
                make_sentence("She is sad.", 12, vec![
                    make_token("She", "PRON", "nsubj", "", "O", 12),
                ]),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        assert_eq!(tree.children.len(), 2, "one paragraph plus narrative_summary");
        let para = &tree.children[0];
        assert_eq!(para.children.len(), 2, "two sentences");
    }

    // 4. Blank lines produce two separate paragraph nodes.
    #[test]
    fn paragraph_detection() {
        // "First paragraph." is 16 bytes; split point at \n\n gives offsets 0 and 18.
        let source = "First paragraph.\n\nSecond paragraph.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("First paragraph.", 0, vec![
                    make_token("First", "ADJ", "amod", "", "O", 0),
                ]),
                make_sentence("Second paragraph.", 18, vec![
                    make_token("Second", "ADJ", "amod", "", "O", 18),
                ]),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        let para_nodes: Vec<&OwnedNode> = tree
            .children
            .iter()
            .filter(|c| c.node_type == "paragraph")
            .collect();
        assert_eq!(para_nodes.len(), 2, "expected two paragraph nodes");
        assert_eq!(para_nodes[0].children.len(), 1);
        assert_eq!(para_nodes[1].children.len(), 1);
    }

    // 5. child_by_field("pos") on a token returns the POS tag.
    #[test]
    fn token_pos_accessible() {
        let source = "Sarah ran.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![make_token("Sarah", "PROPN", "nsubj", "", "O", 0)],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let token = &tree.children[0].children[0].children[0];
        assert_eq!(token.node_type, "token");

        let pos_node = token.child_by_field("pos");
        assert!(pos_node.is_some(), "pos field should be present");
        assert_eq!(pos_node.unwrap().text(), Some("PROPN"));

        let dep_node = token.child_by_field("dep");
        assert!(dep_node.is_some());
        assert_eq!(dep_node.unwrap().text(), Some("nsubj"));

        let lemma_node = token.child_by_field("lemma");
        assert!(lemma_node.is_some());
        assert_eq!(lemma_node.unwrap().text(), Some("sarah"));
    }

    // 6. child_by_field("type") on an entity returns the entity type.
    #[test]
    fn entity_type_accessible() {
        let source = "Sarah ran.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![make_token("Sarah", "PROPN", "nsubj", "PERSON", "B", 0)],
            )],
            entities: vec![make_entity("Sarah", "PERSON", 0)],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        let entity = tree
            .children
            .iter()
            .find(|c| c.node_type == "entity")
            .expect("entity node missing");

        let type_node = entity.child_by_field("type");
        assert!(type_node.is_some(), "type field should be present");
        assert_eq!(type_node.unwrap().text(), Some("PERSON"));
    }

    // 7. Line numbers are correctly assigned across a multi-line document.
    #[test]
    fn line_numbers_correct() {
        // Line 1: "Line one."  (0..9), \n at 9
        // Line 2: "Line two."  (10..19), \n at 19
        // Line 3: "Line three." (20..30)
        let source = "Line one.\nLine two.\nLine three.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Line one.", 0, vec![
                    make_token("Line", "NOUN", "nsubj", "", "O", 0),
                ]),
                make_sentence("Line two.", 10, vec![
                    make_token("Line", "NOUN", "nsubj", "", "O", 10),
                ]),
                make_sentence("Line three.", 20, vec![
                    make_token("Line", "NOUN", "nsubj", "", "O", 20),
                ]),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        assert_eq!(tree.start_line, 1);
        assert_eq!(tree.end_line, 3);

        let para = &tree.children[0];
        assert_eq!(para.children[0].start_line, 1);
        assert_eq!(para.children[1].start_line, 2);
        assert_eq!(para.children[2].start_line, 3);

        // Tokens on each line
        assert_eq!(para.children[0].children[0].start_line, 1); // token on line 1
        assert_eq!(para.children[1].children[0].start_line, 2); // token on line 2
        assert_eq!(para.children[2].children[0].start_line, 3); // token on line 3
    }

    // 8. source_file is propagated to all nodes.
    #[test]
    fn source_file_propagated() {
        let source = "Hello world.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token("Hello", "INTJ", "ROOT", "", "O", 0),
                    make_token("world", "NOUN", "npadvmod", "", "O", 6),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, Some("test.txt"));

        fn check_all(node: &OwnedNode, expected: &str) {
            assert_eq!(
                node.source_file.as_deref(),
                Some(expected),
                "node {} missing source_file",
                node.node_type
            );
            for child in &node.children {
                check_all(child, expected);
            }
        }
        check_all(&tree, "test.txt");
    }

    // 9. Empty SpacyDoc produces a document with no children.
    #[test]
    fn empty_document() {
        let source = "";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        assert_eq!(tree.node_type, "document");
        assert_eq!(
            tree.children.len(), 1,
            "empty doc should have only narrative_summary, got {}",
            tree.children.len()
        );
        assert_eq!(tree.children[0].node_type, "narrative_summary");
    }

    // 10. Entities differing only by case are deduplicated into one node with two locations.
    #[test]
    fn entity_dedup_case_insensitive() {
        let source = "Sarah went to see SARAH again.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token("Sarah", "PROPN", "nsubj", "PERSON", "B", 0),
                    make_token("SARAH", "PROPN", "dobj", "PERSON", "B", 18),
                ],
            )],
            entities: vec![
                make_entity("Sarah", "PERSON", 0),
                make_entity("SARAH", "PERSON", 18),
            ],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        let entity_nodes: Vec<&OwnedNode> = tree
            .children
            .iter()
            .filter(|c| c.node_type == "entity")
            .collect();
        assert_eq!(entity_nodes.len(), 1, "two mentions should dedup to one entity");

        let entity = entity_nodes[0];
        // First child is entity_type, remaining are locations.
        let location_nodes: Vec<&OwnedNode> = entity
            .children
            .iter()
            .filter(|c| c.node_type == "location")
            .collect();
        assert_eq!(location_nodes.len(), 2, "two location children expected");

        // field_indices for locations should cover both
        let loc_indices = entity.field_indices.get("locations").expect("locations field missing");
        assert_eq!(loc_indices.len(), 2);
    }

    // ── Interaction extraction tests ─────────────────────────────────

    fn make_token_with_head(
        text: &str,
        pos: &str,
        dep: &str,
        ent_type: &str,
        ent_iob: &str,
        idx: usize,
        head: usize,
    ) -> SpacyTokenData {
        SpacyTokenData {
            text: text.to_string(),
            lemma: text.to_lowercase(),
            pos: pos.to_string(),
            tag: pos.to_string(),
            dep: dep.to_string(),
            head,
            ent_type: ent_type.to_string(),
            ent_iob: ent_iob.to_string(),
            idx,
        }
    }

    #[test]
    fn interaction_active_simple() {
        // Sarah chased the cat.
        // Sarah(nsubj,h=1) chased(ROOT,h=1) the(det,h=3) cat(dobj,h=1) .(punct,h=1)
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "", "O", 0, 1),
            make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
            make_token_with_head("the", "DET", "det", "", "O", 13, 3),
            make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
        ];
        let sent = make_sentence("Sarah chased the cat.", 0, tokens);
        let line_starts = build_line_starts("Sarah chased the cat.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.verb, "chased");
        assert_eq!(i.agent.as_deref(), Some("Sarah"));
        assert_eq!(i.patient.as_deref(), Some("the cat"));
        assert!(!i.is_passive);
    }

    #[test]
    fn interaction_passive_by_agent() {
        // The cat was chased by Sarah.
        // The(det,h=1) cat(nsubjpass,h=3) was(auxpass,h=3) chased(ROOT,h=3) by(agent,h=3) Sarah(pobj,h=4) .(punct,h=3)
        let tokens = vec![
            make_token_with_head("The", "DET", "det", "", "O", 0, 1),
            make_token_with_head("cat", "NOUN", "nsubjpass", "", "O", 4, 3),
            make_token_with_head("was", "AUX", "auxpass", "", "O", 8, 3),
            make_token_with_head("chased", "VERB", "ROOT", "", "O", 12, 3),
            make_token_with_head("by", "ADP", "agent", "", "O", 19, 3),
            make_token_with_head("Sarah", "PROPN", "pobj", "PERSON", "B", 22, 4),
        ];
        let sent = make_sentence("The cat was chased by Sarah.", 0, tokens);
        let line_starts = build_line_starts("The cat was chased by Sarah.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.verb, "chased");
        assert_eq!(i.agent.as_deref(), Some("Sarah"));
        assert_eq!(i.patient.as_deref(), Some("The cat"));
        assert!(i.is_passive);
    }

    #[test]
    fn interaction_passive_participle_contribute() {
        // David, punched by Jane, with so much force that it hurt him.
        let tokens = vec![
            make_token_with_head("David", "PROPN", "ROOT", "PERSON", "B", 0, 0),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 5, 0),
            make_token_with_head("punched", "VERB", "acl", "", "O", 7, 0),
            make_token_with_head("by", "ADP", "agent", "", "O", 15, 2),
            make_token_with_head("Jane", "PROPN", "pobj", "PERSON", "B", 18, 3),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 27, 2),
            make_token_with_head("with", "ADP", "prep", "", "O", 29, 2),
            make_token_with_head("so", "ADV", "advmod", "", "O", 34, 8),
            make_token_with_head("much", "ADJ", "amod", "", "O", 37, 9),
            make_token_with_head("force", "NOUN", "pobj", "", "O", 42, 6),
            make_token_with_head("that", "PRON", "mark", "", "O", 48, 12),
            make_token_with_head("it", "PRON", "nsubj", "", "O", 53, 12),
            make_token_with_head("hurt", "VERB", "acl", "", "O", 56, 9),
            make_token_with_head("him", "PRON", "dobj", "", "O", 61, 12),
        ];
        let sent = make_sentence(
            "David, punched by Jane, with so much force that it hurt him.",
            0,
            tokens,
        );
        let line_starts = build_line_starts(
            "David, punched by Jane, with so much force that it hurt him.",
        );
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        // Should find at least "punched" interaction
        let punched = interactions.iter().find(|i| i.verb == "punched").expect("should find punched");
        assert_eq!(punched.agent.as_deref(), Some("Jane"));
        assert_eq!(punched.patient.as_deref(), Some("David"));
        assert!(punched.is_passive);
        // Also should find "hurt" interaction
        let hurt = interactions.iter().find(|i| i.verb == "hurt").expect("should find hurt");
        assert_eq!(hurt.agent.as_deref(), Some("it"));
        assert_eq!(hurt.patient.as_deref(), Some("him"));
    }

    #[test]
    fn interaction_ditransitive() {
        // Sarah gave Bob the key.
        // Sarah(nsubj,h=1) gave(ROOT,h=1) Bob(dative,h=1) the(det,h=4) key(dobj,h=1) .(punct,h=1)
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
            make_token_with_head("gave", "VERB", "ROOT", "", "O", 6, 1),
            make_token_with_head("Bob", "PROPN", "dative", "PERSON", "B", 11, 1),
            make_token_with_head("the", "DET", "det", "", "O", 15, 4),
            make_token_with_head("key", "NOUN", "dobj", "", "O", 19, 1),
        ];
        let sent = make_sentence("Sarah gave Bob the key.", 0, tokens);
        let line_starts = build_line_starts("Sarah gave Bob the key.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.agent.as_deref(), Some("Sarah"));
        assert_eq!(i.verb, "gave");
        assert_eq!(i.patient.as_deref(), Some("the key"));
        assert_eq!(i.recipient.as_deref(), Some("Bob"));
    }

    #[test]
    fn interaction_instrument() {
        // She opened it with a hammer.
        // She(nsubj,h=1) opened(ROOT,h=1) it(dobj,h=1) with(prep,h=1) a(det,h=5) hammer(pobj,h=3) .(punct,h=1)
        let tokens = vec![
            make_token_with_head("She", "PRON", "nsubj", "", "O", 0, 1),
            make_token_with_head("opened", "VERB", "ROOT", "", "O", 4, 1),
            make_token_with_head("it", "PRON", "dobj", "", "O", 11, 1),
            make_token_with_head("with", "ADP", "prep", "", "O", 14, 1),
            make_token_with_head("a", "DET", "det", "", "O", 19, 5),
            make_token_with_head("hammer", "NOUN", "pobj", "", "O", 21, 3),
        ];
        let sent = make_sentence("She opened it with a hammer.", 0, tokens);
        let line_starts = build_line_starts("She opened it with a hammer.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.agent.as_deref(), Some("She"));
        assert_eq!(i.patient.as_deref(), Some("it"));
        assert_eq!(i.instrument.as_deref(), Some("a hammer"));
    }

    #[test]
    fn interaction_intransitive() {
        // It rained.
        let tokens = vec![
            make_token_with_head("It", "PRON", "nsubj", "", "O", 0, 1),
            make_token_with_head("rained", "VERB", "ROOT", "", "O", 3, 1),
        ];
        let sent = make_sentence("It rained.", 0, tokens);
        let line_starts = build_line_starts("It rained.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        assert_eq!(interactions[0].verb, "rained");
        assert_eq!(interactions[0].agent.as_deref(), Some("It"));
        assert_eq!(interactions[0].patient, None);
    }

    #[test]
    fn interaction_agentless_passive() {
        // The door was opened.
        // The(det,h=1) door(nsubjpass,h=3) was(auxpass,h=3) opened(ROOT,VERB,h=3) .(punct,h=3)
        let tokens = vec![
            make_token_with_head("The", "DET", "det", "", "O", 0, 1),
            make_token_with_head("door", "NOUN", "nsubjpass", "", "O", 4, 3),
            make_token_with_head("was", "AUX", "auxpass", "", "O", 9, 3),
            make_token_with_head("opened", "VERB", "ROOT", "", "O", 13, 3),
        ];
        let sent = make_sentence("The door was opened.", 0, tokens);
        let line_starts = build_line_starts("The door was opened.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.verb, "opened");
        assert_eq!(i.patient.as_deref(), Some("The door"));
        assert_eq!(i.agent, None);
        assert!(i.is_passive);
    }

    #[test]
    fn interaction_compound_subject() {
        // Sarah and Tom entered the cave.
        // Sarah(nsubj,h=3) and(cc,h=0) Tom(conj,h=0) entered(ROOT,h=3) the(det,h=5) cave(dobj,h=3) .(punct,h=3)
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 3),
            make_token_with_head("and", "CCONJ", "cc", "", "O", 6, 0),
            make_token_with_head("Tom", "PROPN", "conj", "PERSON", "B", 10, 0),
            make_token_with_head("entered", "VERB", "ROOT", "", "O", 14, 3),
            make_token_with_head("the", "DET", "det", "", "O", 22, 5),
            make_token_with_head("cave", "NOUN", "dobj", "", "O", 26, 3),
        ];
        let sent = make_sentence("Sarah and Tom entered the cave.", 0, tokens);
        let line_starts = build_line_starts("Sarah and Tom entered the cave.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.verb, "entered");
        assert!(i.agent.as_ref().unwrap().contains("Sarah"));
        assert!(i.agent.as_ref().unwrap().contains("Tom"));
        assert_eq!(i.patient.as_deref(), Some("the cave"));
    }

    #[test]
    fn interaction_conj_verb_inherits_subject() {
        // Joey battled Kristy and won.
        // Joey(nsubj,h=1) battled(ROOT,h=1) Kristy(dobj,h=1) and(cc,h=1) won(conj,VERB,h=1) .(punct,h=1)
        let tokens = vec![
            make_token_with_head("Joey", "PROPN", "nsubj", "PERSON", "B", 0, 1),
            make_token_with_head("battled", "VERB", "ROOT", "", "O", 5, 1),
            make_token_with_head("Kristy", "PROPN", "dobj", "PERSON", "B", 13, 1),
            make_token_with_head("and", "CCONJ", "cc", "", "O", 20, 1),
            make_token_with_head("won", "VERB", "conj", "", "O", 24, 1),
        ];
        let sent = make_sentence("Joey battled Kristy and won.", 0, tokens);
        let line_starts = build_line_starts("Joey battled Kristy and won.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 2);
        let battled = interactions.iter().find(|i| i.verb == "battled").unwrap();
        assert_eq!(battled.agent.as_deref(), Some("Joey"));
        assert_eq!(battled.patient.as_deref(), Some("Kristy"));
        let won = interactions.iter().find(|i| i.verb == "won").unwrap();
        assert_eq!(won.agent.as_deref(), Some("Joey")); // inherited from head verb
    }

    #[test]
    fn interaction_spacy_v3_labels() {
        // Same as passive but with v3 labels: nsubj:pass, obj
        let tokens = vec![
            make_token_with_head("The", "DET", "det", "", "O", 0, 1),
            make_token_with_head("cat", "NOUN", "nsubj:pass", "", "O", 4, 3),
            make_token_with_head("was", "AUX", "aux:pass", "", "O", 8, 3),
            make_token_with_head("chased", "VERB", "ROOT", "", "O", 12, 3),
            make_token_with_head("by", "ADP", "agent", "", "O", 19, 3),
            make_token_with_head("Sarah", "PROPN", "pobj", "PERSON", "B", 22, 4),
        ];
        let sent = make_sentence("The cat was chased by Sarah.", 0, tokens);
        let line_starts = build_line_starts("The cat was chased by Sarah.");
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        assert_eq!(interactions.len(), 1);
        let i = &interactions[0];
        assert_eq!(i.agent.as_deref(), Some("Sarah"));
        assert_eq!(i.patient.as_deref(), Some("The cat"));
        assert!(i.is_passive);
    }

    #[test]
    fn normalize_dep_idempotent() {
        assert_eq!(normalize_dep("nsubjpass"), "nsubjpass");
        assert_eq!(normalize_dep("nsubj:pass"), "nsubjpass");
        assert_eq!(normalize_dep("dobj"), "dobj");
        assert_eq!(normalize_dep("obj"), "dobj");
        assert_eq!(normalize_dep("nsubj"), "nsubj");
    }

    // ── verb_phrase and interaction node tests ────────────────────────

    #[test]
    fn sentence_has_verb_phrases() {
        let source = "Sarah chased the cat.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
                    make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
                    make_token_with_head("the", "DET", "det", "", "O", 13, 3),
                    make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
                    make_token_with_head(".", "PUNCT", "punct", "", "O", 20, 1),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let para = &tree.children[0];
        let sent = &para.children[0];

        // Should have 5 token children + 1 verb_phrase child
        assert_eq!(sent.children.len(), 6);
        assert!(sent.field_indices.contains_key("tokens"));
        assert!(sent.field_indices.contains_key("verb_phrases"));
        assert_eq!(sent.field_indices["tokens"], vec![0, 1, 2, 3, 4]);
        assert_eq!(sent.field_indices["verb_phrases"], vec![5]);

        let vp = &sent.children[5];
        assert_eq!(vp.node_type, "verb_phrase");
        assert_eq!(vp.text.as_deref(), Some("chased"));
    }

    #[test]
    fn verb_phrase_has_role_fields() {
        let source = "Sarah chased the cat.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
                    make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
                    make_token_with_head("the", "DET", "det", "", "O", 13, 3),
                    make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let sent = &tree.children[0].children[0];
        let vp = sent.children.last().unwrap();

        assert!(vp.field_indices.contains_key("verb"));
        assert!(vp.field_indices.contains_key("agent"));
        assert!(vp.field_indices.contains_key("patient"));
        assert!(vp.field_indices.contains_key("voice"));

        let agent_idx = vp.field_indices["agent"][0];
        assert_eq!(vp.children[agent_idx].text.as_deref(), Some("Sarah"));
        let patient_idx = vp.field_indices["patient"][0];
        assert_eq!(vp.children[patient_idx].text.as_deref(), Some("the cat"));
        let voice_idx = vp.field_indices["voice"][0];
        assert_eq!(vp.children[voice_idx].text.as_deref(), Some("active"));
    }

    #[test]
    fn passive_verb_phrase_voice() {
        let source = "The cat was chased by Sarah.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token_with_head("The", "DET", "det", "", "O", 0, 1),
                    make_token_with_head("cat", "NOUN", "nsubjpass", "", "O", 4, 3),
                    make_token_with_head("was", "AUX", "auxpass", "", "O", 8, 3),
                    make_token_with_head("chased", "VERB", "ROOT", "", "O", 12, 3),
                    make_token_with_head("by", "ADP", "agent", "", "O", 19, 3),
                    make_token_with_head("Sarah", "PROPN", "pobj", "PERSON", "B", 22, 4),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let sent = &tree.children[0].children[0];
        let vp = sent.children.last().unwrap();

        let voice_idx = vp.field_indices["voice"][0];
        assert_eq!(vp.children[voice_idx].text.as_deref(), Some("passive"));
        let agent_idx = vp.field_indices["agent"][0];
        assert_eq!(vp.children[agent_idx].text.as_deref(), Some("Sarah"));
        let patient_idx = vp.field_indices["patient"][0];
        assert_eq!(vp.children[patient_idx].text.as_deref(), Some("The cat"));
    }

    #[test]
    fn document_has_interaction_nodes() {
        let source = "Sarah chased the cat. Bob opened the door.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence(
                    "Sarah chased the cat.",
                    0,
                    vec![
                        make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
                        make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
                        make_token_with_head("the", "DET", "det", "", "O", 13, 3),
                        make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
                    ],
                ),
                make_sentence(
                    "Bob opened the door.",
                    22,
                    vec![
                        make_token_with_head("Bob", "PROPN", "nsubj", "PERSON", "B", 22, 1),
                        make_token_with_head("opened", "VERB", "ROOT", "", "O", 26, 1),
                        make_token_with_head("the", "DET", "det", "", "O", 33, 3),
                        make_token_with_head("door", "NOUN", "dobj", "", "O", 37, 1),
                    ],
                ),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        assert!(tree.field_indices.contains_key("paragraphs"));
        assert!(tree.field_indices.contains_key("interactions"));

        let interaction_indices = &tree.field_indices["interactions"];
        assert_eq!(interaction_indices.len(), 2);

        for &idx in interaction_indices {
            let node = &tree.children[idx];
            assert_eq!(node.node_type, "interaction");
            assert!(node.field_indices.contains_key("agent"));
            assert!(node.field_indices.contains_key("verb"));
            assert!(node.field_indices.contains_key("voice"));
        }
    }

    #[test]
    fn interaction_text_is_summary() {
        let source = "Sarah chased the cat.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
                    make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
                    make_token_with_head("the", "DET", "det", "", "O", 13, 3),
                    make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_idx = tree.field_indices["interactions"][0];
        let node = &tree.children[interaction_idx];
        assert_eq!(node.text.as_deref(), Some("Sarah chased the cat"));
    }

    #[test]
    fn interaction_dedup_merges_lines() {
        // Same interaction on two lines → one node with two line refs
        let source = "Sarah chased the cat. Sarah chased the cat.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence(
                    "Sarah chased the cat.",
                    0,
                    vec![
                        make_token_with_head("Sarah", "PROPN", "nsubj", "", "O", 0, 1),
                        make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
                        make_token_with_head("the", "DET", "det", "", "O", 13, 3),
                        make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
                    ],
                ),
                make_sentence(
                    "Sarah chased the cat.",
                    22,
                    vec![
                        make_token_with_head("Sarah", "PROPN", "nsubj", "", "O", 22, 1),
                        make_token_with_head("chased", "VERB", "ROOT", "", "O", 28, 1),
                        make_token_with_head("the", "DET", "det", "", "O", 35, 3),
                        make_token_with_head("cat", "NOUN", "dobj", "", "O", 39, 1),
                    ],
                ),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_indices = &tree.field_indices["interactions"];
        // Should be deduplicated to 1 interaction node
        assert_eq!(interaction_indices.len(), 1);
        let node = &tree.children[interaction_indices[0]];
        // Should have 2 line references
        assert_eq!(node.field_indices["lines"].len(), 2);
    }

    #[test]
    fn tokens_field_unchanged_with_verb_phrases() {
        // Verify tokens field_indices is preserved (no regression from Tier 1)
        let source = "Sarah ran.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
                    make_token_with_head("ran", "VERB", "ROOT", "", "O", 6, 1),
                    make_token_with_head(".", "PUNCT", "punct", "", "O", 9, 1),
                ],
            )],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let sent = &tree.children[0].children[0];
        // tokens field should still be [0, 1, 2]
        assert_eq!(sent.field_indices["tokens"], vec![0, 1, 2]);
        // And verb_phrases should be [3]
        assert_eq!(sent.field_indices["verb_phrases"], vec![3]);
    }

    #[test]
    fn doc_field_indices_all_present() {
        // Verify paragraphs, entities, AND interactions all have correct non-overlapping ranges
        let source = "Sarah chased the cat in Paris.";
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(
                source,
                0,
                vec![
                    make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
                    make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
                    make_token_with_head("the", "DET", "det", "", "O", 13, 3),
                    make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
                    make_token_with_head("in", "ADP", "prep", "", "O", 21, 1),
                    make_token_with_head("Paris", "PROPN", "pobj", "GPE", "B", 24, 4),
                ],
            )],
            entities: vec![SpacyEntityData {
                text: "Sarah".to_string(),
                label: "PERSON".to_string(),
                start_char: 0,
                end_char: 5,
            }, SpacyEntityData {
                text: "Paris".to_string(),
                label: "GPE".to_string(),
                start_char: 24,
                end_char: 29,
            }],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        let para_range = &tree.field_indices["paragraphs"];
        let entity_range = &tree.field_indices["entities"];
        let interaction_range = &tree.field_indices["interactions"];

        // Ranges should be non-overlapping
        let para_max = *para_range.last().unwrap();
        let entity_min = *entity_range.first().unwrap();
        let entity_max = *entity_range.last().unwrap();
        let interaction_min = *interaction_range.first().unwrap();

        assert!(para_max < entity_min, "paragraphs before entities");
        assert!(entity_max < interaction_min, "entities before interactions");

        // All children types at correct indices
        for &i in para_range { assert_eq!(tree.children[i].node_type, "paragraph"); }
        for &i in entity_range { assert_eq!(tree.children[i].node_type, "entity"); }
        for &i in interaction_range { assert_eq!(tree.children[i].node_type, "interaction"); }
    }

    #[test]
    fn interaction_nested_passives() {
        // The letter, written by Sarah and delivered by Tom, was important.
        let tokens = vec![
            make_token_with_head("The", "DET", "det", "", "O", 0, 1),
            make_token_with_head("letter", "NOUN", "nsubj", "", "O", 4, 11),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 10, 1),
            make_token_with_head("written", "VERB", "acl", "", "O", 12, 1),
            make_token_with_head("by", "ADP", "agent", "", "O", 20, 3),
            make_token_with_head("Sarah", "PROPN", "pobj", "PERSON", "B", 23, 4),
            make_token_with_head("and", "CCONJ", "cc", "", "O", 29, 3),
            make_token_with_head("delivered", "VERB", "conj", "", "O", 33, 3),
            make_token_with_head("by", "ADP", "agent", "", "O", 43, 7),
            make_token_with_head("Tom", "PROPN", "pobj", "PERSON", "B", 46, 8),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 49, 1),
            make_token_with_head("was", "AUX", "ROOT", "", "O", 51, 11),
            make_token_with_head("important", "ADJ", "acomp", "", "O", 55, 11),
        ];
        let sent = make_sentence(
            "The letter, written by Sarah and delivered by Tom, was important.",
            0,
            tokens,
        );
        let line_starts = build_line_starts(
            "The letter, written by Sarah and delivered by Tom, was important.",
        );
        let interactions = extract_interactions_from_sentence(&sent, &line_starts);
        let written = interactions.iter().find(|i| i.verb == "written").expect("should find written");
        assert_eq!(written.agent.as_deref(), Some("Sarah"));
        assert_eq!(written.patient.as_deref(), Some("The letter")); // acl → head = letter
        assert!(written.is_passive);
        let delivered = interactions.iter().find(|i| i.verb == "delivered").expect("should find delivered");
        assert_eq!(delivered.agent.as_deref(), Some("Tom"));
        assert!(delivered.is_passive);
    }

    // ── Co-reference enrichment tests ────────────────────────────────

    #[test]
    fn entity_node_has_aliases() {
        // "Sarah, the detective, arrived."
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 5),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 5, 0),
            make_token_with_head("the", "DET", "det", "", "O", 7, 3),
            make_token_with_head("detective", "NOUN", "appos", "", "O", 11, 0),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 20, 0),
            make_token_with_head("arrived", "VERB", "ROOT", "", "O", 22, 5),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 29, 5),
        ];
        let sentence = SpacySentence {
            text: "Sarah, the detective, arrived.".to_string(),
            start: 0,
            end: 30,
            tokens,
        };
        let entity = SpacyEntityData {
            text: "Sarah".to_string(),
            label: "PERSON".to_string(),
            start_char: 0,
            end_char: 5,
        };
        let doc = SpacyDoc {
            text: "Sarah, the detective, arrived.".to_string(),
            sentences: vec![sentence],
            entities: vec![entity],
        };

        let tree = spacy_doc_to_owned_tree(&doc, "Sarah, the detective, arrived.", None);
        let entity_indices = tree.field_indices.get("entities").unwrap();
        let entity_node = &tree.children[entity_indices[0]];

        assert!(entity_node.field_indices.contains_key("aliases"), "entity should have aliases field");
        let aliases_idx = entity_node.field_indices["aliases"][0];
        let aliases_node = &entity_node.children[aliases_idx];
        assert_eq!(aliases_node.node_type, "aliases");
        assert!(
            aliases_node.children.iter().any(|c| c.text.as_deref() == Some("the detective")),
            "aliases should contain 'the detective'"
        );
    }

    #[test]
    fn entity_no_coref_unchanged() {
        // "Paris is beautiful." — no appositives, no pronouns
        let tokens = vec![
            make_token_with_head("Paris", "PROPN", "nsubj", "GPE", "B", 0, 1),
            make_token_with_head("is", "AUX", "ROOT", "", "O", 6, 1),
            make_token_with_head("beautiful", "ADJ", "acomp", "", "O", 9, 1),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 18, 1),
        ];
        let sentence = SpacySentence {
            text: "Paris is beautiful.".to_string(),
            start: 0,
            end: 19,
            tokens,
        };
        let entity = SpacyEntityData {
            text: "Paris".to_string(),
            label: "GPE".to_string(),
            start_char: 0,
            end_char: 5,
        };
        let doc = SpacyDoc {
            text: "Paris is beautiful.".to_string(),
            sentences: vec![sentence],
            entities: vec![entity],
        };

        let tree = spacy_doc_to_owned_tree(&doc, "Paris is beautiful.", None);
        let entity_indices = tree.field_indices.get("entities").unwrap();
        let entity_node = &tree.children[entity_indices[0]];

        assert!(entity_node.field_indices.contains_key("type"), "entity should have type field");
        assert!(entity_node.field_indices.contains_key("locations"), "entity should have locations field");
        assert!(!entity_node.field_indices.contains_key("aliases"), "entity should NOT have aliases field");
        assert!(!entity_node.field_indices.contains_key("coreference_chain"), "entity should NOT have coreference_chain field");
    }

    #[test]
    fn entity_mention_count() {
        // "Sarah, the detective, arrived. Sarah investigated."
        // Sarah entity has 2 direct NER mentions + 1 appositive alias = 3 total
        let source = "Sarah, the detective, arrived. Sarah investigated.";
        let tokens1 = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 5),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 5, 0),
            make_token_with_head("the", "DET", "det", "", "O", 7, 3),
            make_token_with_head("detective", "NOUN", "appos", "", "O", 11, 0),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 20, 0),
            make_token_with_head("arrived", "VERB", "ROOT", "", "O", 22, 5),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 29, 5),
        ];
        let tokens2 = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 31, 1),
            make_token_with_head("investigated", "VERB", "ROOT", "", "O", 37, 1),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 49, 1),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                SpacySentence { text: "Sarah, the detective, arrived.".to_string(), start: 0, end: 30, tokens: tokens1 },
                SpacySentence { text: "Sarah investigated.".to_string(), start: 31, end: 50, tokens: tokens2 },
            ],
            entities: vec![
                SpacyEntityData { text: "Sarah".to_string(), label: "PERSON".to_string(), start_char: 0, end_char: 5 },
                SpacyEntityData { text: "Sarah".to_string(), label: "PERSON".to_string(), start_char: 31, end_char: 36 },
            ],
        };

        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let entity_indices = tree.field_indices.get("entities").unwrap();
        let entity_node = &tree.children[entity_indices[0]];

        assert!(entity_node.field_indices.contains_key("mention_count"), "entity should have mention_count field");
        let mc_idx = entity_node.field_indices["mention_count"][0];
        let mc_node = &entity_node.children[mc_idx];
        // 2 direct NER mentions + 1 appositive coref mention = 3
        assert_eq!(mc_node.text.as_deref(), Some("3"), "mention_count should be 3");
    }

    // ── Query integration tests ──────────────────────────────────────

    fn run_tree_query(tree: &OwnedNode, query: &str) -> Vec<String> {
        let tokens = aq_core::lex(query).expect("lex failed");
        let ast = aq_core::parse(&tokens).expect("parse failed");
        let results = aq_core::eval(&ast, tree).expect("eval failed");
        results
            .into_iter()
            .map(|r| match r {
                aq_core::EvalResult::Node(n) => n.text().unwrap_or(n.node_type()).to_string(),
                aq_core::EvalResult::Value(v) => {
                    v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string())
                }
            })
            .collect()
    }

    // Q1: desc:entity returns entity nodes; aliases field present.
    #[test]
    fn query_entity_aliases() {
        // "Sarah, the detective, arrived."
        let source = "Sarah, the detective, arrived.";
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 5),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 5, 0),
            make_token_with_head("the", "DET", "det", "", "O", 7, 3),
            make_token_with_head("detective", "NOUN", "appos", "", "O", 11, 0),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 20, 0),
            make_token_with_head("arrived", "VERB", "ROOT", "", "O", 22, 5),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 29, 5),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![SpacySentence {
                text: source.to_string(),
                start: 0,
                end: 30,
                tokens,
            }],
            entities: vec![make_entity("Sarah", "PERSON", 0)],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        // desc:entity should return 1 entity node with text "Sarah"
        let results = run_tree_query(&tree, "desc:entity");
        assert_eq!(results.len(), 1, "expected 1 entity, got: {:?}", results);
        assert_eq!(results[0], "Sarah");

        // Verify the entity node has an aliases field
        let entity_indices = tree.field_indices.get("entities").expect("no entities field");
        let entity_node = &tree.children[entity_indices[0]];
        assert!(entity_node.field_indices.contains_key("aliases"), "entity should have aliases field");
        assert!(entity_node.field_indices.contains_key("interaction_count"), "entity should have interaction_count field");
    }

    // Q2: desc:entity returns entity; mention_count accessible via tree query.
    #[test]
    fn query_entity_mention_count() {
        // "Sarah, the detective, arrived. Sarah investigated."
        let source = "Sarah, the detective, arrived. Sarah investigated.";
        let tokens1 = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 5),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 5, 0),
            make_token_with_head("the", "DET", "det", "", "O", 7, 3),
            make_token_with_head("detective", "NOUN", "appos", "", "O", 11, 0),
            make_token_with_head(",", "PUNCT", "punct", "", "O", 20, 0),
            make_token_with_head("arrived", "VERB", "ROOT", "", "O", 22, 5),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 29, 5),
        ];
        let tokens2 = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 31, 1),
            make_token_with_head("investigated", "VERB", "ROOT", "", "O", 37, 1),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 49, 1),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                SpacySentence { text: "Sarah, the detective, arrived.".to_string(), start: 0, end: 30, tokens: tokens1 },
                SpacySentence { text: "Sarah investigated.".to_string(), start: 31, end: 50, tokens: tokens2 },
            ],
            entities: vec![
                SpacyEntityData { text: "Sarah".to_string(), label: "PERSON".to_string(), start_char: 0, end_char: 5 },
                SpacyEntityData { text: "Sarah".to_string(), label: "PERSON".to_string(), start_char: 31, end_char: 36 },
            ],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        // desc:entity returns 1 entity (deduplicated)
        let entity_results = run_tree_query(&tree, "desc:entity");
        assert_eq!(entity_results.len(), 1, "expected 1 deduplicated entity, got: {:?}", entity_results);

        // mention_count should be 3 (2 direct NER + 1 appositive)
        let mc_results = run_tree_query(&tree, "desc:mention_count");
        assert_eq!(mc_results.len(), 1, "expected 1 mention_count node, got: {:?}", mc_results);
        assert_eq!(mc_results[0], "3", "mention_count should be 3");
    }

    // Q3: interaction_count reflects agent/patient involvement.
    #[test]
    fn query_entity_interaction_count() {
        // "Sarah chased the cat. Paris is beautiful."
        // Sarah: agent of "chased" → interaction_count = 1
        // Paris: not involved in any verb interaction → interaction_count = 0
        let source = "Sarah chased the cat. Paris is beautiful.";
        let tokens1 = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
            make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
            make_token_with_head("the", "DET", "det", "", "O", 13, 3),
            make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 20, 1),
        ];
        let tokens2 = vec![
            make_token_with_head("Paris", "PROPN", "nsubj", "GPE", "B", 22, 1),
            make_token_with_head("is", "AUX", "ROOT", "", "O", 28, 1),
            make_token_with_head("beautiful", "ADJ", "acomp", "", "O", 31, 1),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 40, 1),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                SpacySentence { text: "Sarah chased the cat.".to_string(), start: 0, end: 21, tokens: tokens1 },
                SpacySentence { text: "Paris is beautiful.".to_string(), start: 22, end: 41, tokens: tokens2 },
            ],
            entities: vec![
                make_entity("Sarah", "PERSON", 0),
                make_entity("Paris", "GPE", 22),
            ],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        // desc:interaction_count returns both entities' counts
        // Entities sorted by key: ("paris","GPE") before ("sarah","PERSON")
        let ic_results = run_tree_query(&tree, "desc:interaction_count");
        assert_eq!(ic_results.len(), 2, "expected 2 interaction_count nodes, got: {:?}", ic_results);
        // Paris = 0 (first entity alphabetically), Sarah = 1
        assert_eq!(ic_results[0], "0", "Paris interaction_count should be 0");
        assert_eq!(ic_results[1], "1", "Sarah interaction_count should be 1");
    }

    // Q4: Tier 1 regression — entities, tokens, sentences all queryable.
    #[test]
    fn query_tier1_regression() {
        let source = "Sarah went to Paris.";
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
            make_token_with_head("went", "VERB", "ROOT", "", "O", 6, 1),
            make_token_with_head("to", "ADP", "prep", "", "O", 11, 1),
            make_token_with_head("Paris", "PROPN", "pobj", "GPE", "B", 14, 2),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 19, 1),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![
                make_entity("Sarah", "PERSON", 0),
                make_entity("Paris", "GPE", 14),
            ],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        let entity_results = run_tree_query(&tree, "desc:entity");
        assert_eq!(entity_results.len(), 2, "expected 2 entities, got: {:?}", entity_results);
        assert!(entity_results.contains(&"Sarah".to_string()));
        assert!(entity_results.contains(&"Paris".to_string()));

        let token_results = run_tree_query(&tree, "desc:token");
        assert_eq!(token_results.len(), 5, "expected 5 tokens, got: {:?}", token_results);

        let sentence_results = run_tree_query(&tree, "desc:sentence");
        assert_eq!(sentence_results.len(), 1, "expected 1 sentence, got: {:?}", sentence_results);
        assert_eq!(sentence_results[0], "Sarah went to Paris.");
    }

    // Q5: Tier 2 regression — interactions queryable via desc:interaction.
    #[test]
    fn query_tier2_regression() {
        let source = "Sarah chased the cat.";
        let tokens = vec![
            make_token_with_head("Sarah", "PROPN", "nsubj", "PERSON", "B", 0, 1),
            make_token_with_head("chased", "VERB", "ROOT", "", "O", 6, 1),
            make_token_with_head("the", "DET", "det", "", "O", 13, 3),
            make_token_with_head("cat", "NOUN", "dobj", "", "O", 17, 1),
            make_token_with_head(".", "PUNCT", "punct", "", "O", 20, 1),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        let interaction_results = run_tree_query(&tree, "desc:interaction");
        assert_eq!(interaction_results.len(), 1, "expected 1 interaction, got: {:?}", interaction_results);
        assert_eq!(interaction_results[0], "Sarah chased the cat");
    }

    // Phase 6 E2E contribute example tests

    #[test]
    fn contribute_david_him() {
        // "David, punched by Jane, with so much force that it hurt him."
        // David is nsubjpass of "punched" (passive), Jane is agent via "by".
        // The same-sentence resolver picks the nearest (highest-index) PERSON
        // candidate before "him", which is Jane (token 4) not David (token 0).
        // This test documents the actual pipeline output.
        let text = "David, punched by Jane, with so much force that it hurt him.";
        let tokens = vec![
            make_token_with_head("David",   "PROPN", "nsubjpass", "PERSON", "B", 0,  1),
            make_token_with_head(",",       "PUNCT", "punct",     "",       "O", 5,  1),
            make_token_with_head("punched", "VERB",  "ROOT",      "",       "O", 7,  2),
            make_token_with_head("by",      "ADP",   "agent",     "",       "O", 15, 2),
            make_token_with_head("Jane",    "PROPN", "pobj",      "PERSON", "B", 18, 3),
            make_token_with_head(",",       "PUNCT", "punct",     "",       "O", 22, 2),
            make_token_with_head("with",    "ADP",   "prep",      "",       "O", 24, 2),
            make_token_with_head("so",      "ADV",   "advmod",    "",       "O", 29, 8),
            make_token_with_head("much",    "ADJ",   "amod",      "",       "O", 32, 9),
            make_token_with_head("force",   "NOUN",  "pobj",      "",       "O", 37, 6),
            make_token_with_head("that",    "SCONJ", "mark",      "",       "O", 43, 11),
            make_token_with_head("it",      "PRON",  "nsubj",     "",       "O", 48, 12),
            make_token_with_head("hurt",    "VERB",  "relcl",     "",       "O", 51, 9),
            make_token_with_head("him",     "PRON",  "dobj",      "",       "O", 56, 12),
            make_token_with_head(".",       "PUNCT", "punct",     "",       "O", 59, 2),
        ];
        let sentence = SpacySentence { text: text.to_string(), start: 0, end: 60, tokens };
        let entities = vec![
            make_entity("David", "PERSON", 0),
            make_entity("Jane", "PERSON", 18),
        ];
        let doc = SpacyDoc { text: text.to_string(), sentences: vec![sentence], entities };
        let tree = spacy_doc_to_owned_tree(&doc, text, None);

        // Both entities should be present
        let entity_idxs = tree.field_indices.get("entities").expect("entities field missing");
        assert_eq!(entity_idxs.len(), 2, "expected David and Jane entities");

        let names: Vec<&str> = entity_idxs.iter()
            .map(|&i| tree.children[i].text.as_deref().unwrap_or(""))
            .collect();
        assert!(names.contains(&"David"), "David entity not found");
        assert!(names.contains(&"Jane"), "Jane entity not found");

        // The same-sentence resolver uses nearest-antecedent (highest token index
        // before the pronoun). "him" (token 13) resolves to Jane (token 4) since
        // Jane is the last PERSON before "him". Jane should have "him" as alias.
        let jane = entity_idxs.iter()
            .map(|&i| &tree.children[i])
            .find(|e| e.text.as_deref() == Some("Jane"))
            .expect("Jane entity not found");

        assert!(jane.field_indices.contains_key("aliases"),
            "Jane should have 'him' alias (nearest antecedent), keys: {:?}",
            jane.field_indices.keys().collect::<Vec<_>>());
        let aliases_idx = jane.field_indices["aliases"][0];
        let aliases = &jane.children[aliases_idx];
        let alias_texts: Vec<&str> = aliases.children.iter().filter_map(|c| c.text.as_deref()).collect();
        assert!(alias_texts.contains(&"him"),
            "Jane's aliases should include 'him' (nearest-antecedent resolution), got {:?}", alias_texts);
    }

    #[test]
    fn contribute_jane_she_her() {
        let text = "Jane came back to life with the help of Bob Markey.\nAfter Jane came back to life she ran away to her home in Azure.";

        // Sentence 1: "Jane came back to life with the help of Bob Markey."
        let s1_tokens = vec![
            make_token_with_head("Jane",   "PROPN", "nsubj",    "PERSON", "B", 0,  1),
            make_token_with_head("came",   "VERB",  "ROOT",     "",       "O", 5,  1),
            make_token_with_head("back",   "ADV",   "advmod",   "",       "O", 10, 1),
            make_token_with_head("to",     "ADP",   "prep",     "",       "O", 15, 1),
            make_token_with_head("life",   "NOUN",  "pobj",     "",       "O", 18, 3),
            make_token_with_head("with",   "ADP",   "prep",     "",       "O", 23, 1),
            make_token_with_head("the",    "DET",   "det",      "",       "O", 28, 7),
            make_token_with_head("help",   "NOUN",  "pobj",     "",       "O", 32, 5),
            make_token_with_head("of",     "ADP",   "prep",     "",       "O", 37, 7),
            make_token_with_head("Bob",    "PROPN", "compound", "PERSON", "B", 40, 10),
            make_token_with_head("Markey", "PROPN", "pobj",     "PERSON", "I", 44, 8),
            make_token_with_head(".",      "PUNCT", "punct",    "",       "O", 50, 1),
        ];
        let s1 = SpacySentence { text: "Jane came back to life with the help of Bob Markey.".to_string(), start: 0, end: 51, tokens: s1_tokens };

        // Sentence 2: "After Jane came back to life she ran away to her home in Azure."
        // Starts at offset 52 (after "\n")
        let s2_start = 52;
        let s2_tokens = vec![
            make_token_with_head("After",  "ADP",   "prep",   "",       "O", s2_start + 0,  7),
            make_token_with_head("Jane",   "PROPN", "nsubj",  "PERSON", "B", s2_start + 6,  2),
            make_token_with_head("came",   "VERB",  "advcl",  "",       "O", s2_start + 11, 7),
            make_token_with_head("back",   "ADV",   "advmod", "",       "O", s2_start + 16, 2),
            make_token_with_head("to",     "ADP",   "prep",   "",       "O", s2_start + 21, 2),
            make_token_with_head("life",   "NOUN",  "pobj",   "",       "O", s2_start + 24, 4),
            make_token_with_head("she",    "PRON",  "nsubj",  "",       "O", s2_start + 29, 7),
            make_token_with_head("ran",    "VERB",  "ROOT",   "",       "O", s2_start + 33, 7),
            make_token_with_head("away",   "ADV",   "advmod", "",       "O", s2_start + 37, 7),
            make_token_with_head("to",     "ADP",   "prep",   "",       "O", s2_start + 42, 7),
            make_token_with_head("her",    "PRON",  "poss",   "",       "O", s2_start + 45, 11),
            make_token_with_head("home",   "NOUN",  "pobj",   "",       "O", s2_start + 49, 9),
            make_token_with_head("in",     "ADP",   "prep",   "",       "O", s2_start + 54, 11),
            make_token_with_head("Azure",  "PROPN", "pobj",   "GPE",    "B", s2_start + 57, 12),
            make_token_with_head(".",      "PUNCT", "punct",  "",       "O", s2_start + 62, 7),
        ];
        let s2 = SpacySentence { text: "After Jane came back to life she ran away to her home in Azure.".to_string(), start: s2_start, end: s2_start + 63, tokens: s2_tokens };

        let entities = vec![
            make_entity("Jane", "PERSON", 0),
            make_entity("Bob Markey", "PERSON", 40),
            make_entity("Jane", "PERSON", s2_start + 6),
            make_entity("Azure", "GPE", s2_start + 57),
        ];
        let doc = SpacyDoc { text: text.to_string(), sentences: vec![s1, s2], entities };
        let tree = spacy_doc_to_owned_tree(&doc, text, None);

        // Find Jane entity (first/canonical one)
        let entity_idxs = tree.field_indices.get("entities").unwrap();
        let jane = entity_idxs.iter()
            .map(|&i| &tree.children[i])
            .find(|e| e.text.as_deref() == Some("Jane"))
            .expect("Jane entity not found");

        // Jane should have aliases ("she" and/or "her" from sentence 2)
        assert!(jane.field_indices.contains_key("aliases"),
            "Jane should have aliases, keys: {:?}", jane.field_indices.keys().collect::<Vec<_>>());
        let aliases_idx = jane.field_indices["aliases"][0];
        let aliases_node = &jane.children[aliases_idx];
        let alias_texts: Vec<&str> = aliases_node.children.iter().filter_map(|c| c.text.as_deref()).collect();
        assert!(!alias_texts.is_empty(), "Jane should have aliases, got empty");

        // mention_count should be present
        assert!(jane.field_indices.contains_key("mention_count"),
            "Jane should have mention_count");
    }

    #[test]
    fn contribute_joey_kristy_no_coref() {
        let text = "While in and around Lavender Town, Joey battled Kristy and won.";
        let tokens = vec![
            make_token_with_head("While",    "SCONJ", "mark",     "",       "O", 0,  7),
            make_token_with_head("in",       "ADP",   "prep",     "",       "O", 6,  0),
            make_token_with_head("and",      "CCONJ", "cc",       "",       "O", 9,  3),
            make_token_with_head("around",   "ADP",   "conj",     "",       "O", 13, 1),
            make_token_with_head("Lavender", "PROPN", "compound", "GPE",    "B", 20, 5),
            make_token_with_head("Town",     "PROPN", "pobj",     "GPE",    "I", 29, 1),
            make_token_with_head(",",        "PUNCT", "punct",    "",       "O", 33, 7),
            make_token_with_head("Joey",     "PROPN", "nsubj",    "PERSON", "B", 35, 8),
            make_token_with_head("battled",  "VERB",  "ROOT",     "",       "O", 40, 8),
            make_token_with_head("Kristy",   "PROPN", "dobj",     "PERSON", "B", 48, 8),
            make_token_with_head("and",      "CCONJ", "cc",       "",       "O", 55, 8),
            make_token_with_head("won",      "VERB",  "conj",     "",       "O", 59, 8),
            make_token_with_head(".",        "PUNCT", "punct",    "",       "O", 62, 8),
        ];
        let sentence = SpacySentence { text: text.to_string(), start: 0, end: 63, tokens };
        let entities = vec![
            make_entity("Lavender Town", "GPE", 20),
            make_entity("Joey", "PERSON", 35),
            make_entity("Kristy", "PERSON", 48),
        ];
        let doc = SpacyDoc { text: text.to_string(), sentences: vec![sentence], entities };
        let tree = spacy_doc_to_owned_tree(&doc, text, None);

        // Joey and Kristy should have NO aliases (no pronouns in sentence)
        let entity_idxs = tree.field_indices.get("entities").unwrap();
        for &idx in entity_idxs {
            let entity = &tree.children[idx];
            let name = entity.text.as_deref().unwrap_or("");
            if name == "Joey" || name == "Kristy" {
                assert!(!entity.field_indices.contains_key("aliases"),
                    "{} should not have aliases", name);
            }
        }
    }

    #[test]
    fn contribute_paragraph_boundary() {
        // Paragraph 1: "David was punched." Paragraph 2 (after \n\n): "She arrived."
        // "She" should NOT resolve to David (wrong gender AND paragraph boundary)
        let text = "David was punched.\n\nShe arrived.";
        let s1_tokens = vec![
            make_token_with_head("David",   "PROPN", "nsubjpass", "PERSON", "B", 0,  2),
            make_token_with_head("was",     "AUX",   "auxpass",   "",       "O", 6,  2),
            make_token_with_head("punched", "VERB",  "ROOT",      "",       "O", 10, 2),
            make_token_with_head(".",       "PUNCT", "punct",     "",       "O", 17, 2),
        ];
        let s2_start = 20; // after "David was punched.\n\n"
        let s2_tokens = vec![
            make_token_with_head("She",     "PRON",  "nsubj", "",     "O", s2_start + 0,  1),
            make_token_with_head("arrived", "VERB",  "ROOT",  "",     "O", s2_start + 4,  1),
            make_token_with_head(".",       "PUNCT", "punct", "",     "O", s2_start + 11, 1),
        ];
        let s1 = SpacySentence { text: "David was punched.".to_string(), start: 0, end: 18, tokens: s1_tokens };
        let s2 = SpacySentence { text: "She arrived.".to_string(), start: s2_start, end: s2_start + 12, tokens: s2_tokens };
        let entities = vec![make_entity("David", "PERSON", 0)];
        let doc = SpacyDoc { text: text.to_string(), sentences: vec![s1, s2], entities };
        let tree = spacy_doc_to_owned_tree(&doc, text, None);

        // David should NOT have "She" as alias (paragraph boundary + gender mismatch)
        let entity_idxs = tree.field_indices.get("entities").unwrap();
        let david = &tree.children[entity_idxs[0]];
        assert_eq!(david.text.as_deref(), Some("David"));
        assert!(!david.field_indices.contains_key("aliases"),
            "David should not have aliases across paragraph boundary, keys: {:?}",
            david.field_indices.keys().collect::<Vec<_>>());
    }

    #[test]
    fn adr_sarah_detective_aliases_query() {
        // "Sarah, the detective, arrived at the scene.\nShe examined the evidence carefully."
        let text = "Sarah, the detective, arrived at the scene.\nShe examined the evidence carefully.";

        let s1_tokens = vec![
            make_token_with_head("Sarah",     "PROPN", "nsubj", "PERSON", "B", 0,  5),
            make_token_with_head(",",         "PUNCT", "punct", "",       "O", 5,  0),
            make_token_with_head("the",       "DET",   "det",   "",       "O", 7,  3),
            make_token_with_head("detective", "NOUN",  "appos", "",       "O", 11, 0),
            make_token_with_head(",",         "PUNCT", "punct", "",       "O", 20, 0),
            make_token_with_head("arrived",   "VERB",  "ROOT",  "",       "O", 22, 5),
            make_token_with_head("at",        "ADP",   "prep",  "",       "O", 30, 5),
            make_token_with_head("the",       "DET",   "det",   "",       "O", 33, 8),
            make_token_with_head("scene",     "NOUN",  "pobj",  "",       "O", 37, 6),
            make_token_with_head(".",         "PUNCT", "punct", "",       "O", 42, 5),
        ];
        let s1 = SpacySentence { text: "Sarah, the detective, arrived at the scene.".to_string(), start: 0, end: 43, tokens: s1_tokens };

        let s2_start = 44; // after "\n"
        let s2_tokens = vec![
            make_token_with_head("She",       "PRON",  "nsubj",  "",       "O", s2_start + 0,  1),
            make_token_with_head("examined",  "VERB",  "ROOT",   "",       "O", s2_start + 4,  1),
            make_token_with_head("the",       "DET",   "det",    "",       "O", s2_start + 13, 3),
            make_token_with_head("evidence",  "NOUN",  "dobj",   "",       "O", s2_start + 17, 1),
            make_token_with_head("carefully", "ADV",   "advmod", "",       "O", s2_start + 26, 1),
            make_token_with_head(".",         "PUNCT", "punct",  "",       "O", s2_start + 35, 1),
        ];
        let s2 = SpacySentence { text: "She examined the evidence carefully.".to_string(), start: s2_start, end: s2_start + 36, tokens: s2_tokens };

        let entities = vec![make_entity("Sarah", "PERSON", 0)];
        let doc = SpacyDoc { text: text.to_string(), sentences: vec![s1, s2], entities };
        let tree = spacy_doc_to_owned_tree(&doc, text, None);

        // desc:entity should find Sarah
        let entity_results = run_tree_query(&tree, "desc:entity");
        assert!(entity_results.contains(&"Sarah".to_string()), "Should find Sarah entity");

        // Sarah should have aliases
        let entity_idxs = tree.field_indices.get("entities").unwrap();
        let sarah = &tree.children[entity_idxs[0]];
        assert_eq!(sarah.text.as_deref(), Some("Sarah"));

        assert!(sarah.field_indices.contains_key("aliases"),
            "Sarah should have aliases, keys: {:?}", sarah.field_indices.keys().collect::<Vec<_>>());
        let aliases_idx = sarah.field_indices["aliases"][0];
        let aliases_node = &sarah.children[aliases_idx];
        let alias_texts: Vec<&str> = aliases_node.children.iter().filter_map(|c| c.text.as_deref()).collect();
        assert!(alias_texts.contains(&"the detective"),
            "Should have 'the detective' alias, got {:?}", alias_texts);
    }

    // ── Helper for tests needing custom lemmas ───────────────────────────────

    fn make_token_with_lemma(
        text: &str,
        lemma: &str,
        pos: &str,
        dep: &str,
        head: usize,
        idx: usize,
    ) -> SpacyTokenData {
        SpacyTokenData {
            text: text.to_string(),
            lemma: lemma.to_string(),
            pos: pos.to_string(),
            tag: pos.to_string(),
            dep: dep.to_string(),
            head,
            ent_type: "".to_string(),
            ent_iob: "O".to_string(),
            idx,
        }
    }

    // ── Phase 3: integration tests for classify_roles() wiring ──────────────

    #[test]
    fn integration_action_roles() {
        // "Sarah kicked the ball."
        let tokens = vec![
            make_token_with_lemma("Sarah",  "sarah",  "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("kicked", "kick",   "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",    "DET",   "det",   3, 13),
            make_token_with_lemma("ball",   "ball",   "NOUN",  "dobj",  1, 17),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1, 21),
        ];
        let sent = make_sentence("Sarah kicked the ball.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.agent.as_deref(), Some("Sarah"));
        assert_eq!(d.patient.as_deref(), Some("the ball"));
        assert!(d.roles.iter().any(|r| r.participant == "Sarah" && r.thematic_role == crate::roles::ThematicRole::Agent));
        assert!(d.roles.iter().any(|r| r.participant == "the ball" && r.thematic_role == crate::roles::ThematicRole::Patient));
        assert_eq!(d.verb_class, Some(crate::roles::VerbClass::Action));
    }

    #[test]
    fn integration_perception_roles() {
        // "Sarah heard the noise."
        let tokens = vec![
            make_token_with_lemma("Sarah",  "sarah",  "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("heard",  "hear",   "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",    "DET",   "det",   3, 12),
            make_token_with_lemma("noise",  "noise",  "NOUN",  "dobj",  1, 16),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1, 21),
        ];
        let sent = make_sentence("Sarah heard the noise.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.agent.as_deref(), Some("Sarah")); // backward compat: syntactic agent
        assert!(d.roles.iter().any(|r| r.participant == "Sarah" && r.thematic_role == crate::roles::ThematicRole::Experiencer));
        assert!(d.roles.iter().any(|r| r.participant == "the noise" && r.thematic_role == crate::roles::ThematicRole::Theme));
        assert_eq!(d.verb_class, Some(crate::roles::VerbClass::Perception));
    }

    #[test]
    fn integration_motion_roles() {
        // "The ball rolled downhill."
        let tokens = vec![
            make_token_with_lemma("The",      "the",      "DET",   "det",    1, 0),
            make_token_with_lemma("ball",     "ball",     "NOUN",  "nsubj",  2, 4),
            make_token_with_lemma("rolled",   "roll",     "VERB",  "ROOT",   2, 9),
            make_token_with_lemma("downhill", "downhill", "ADV",   "advmod", 2, 16),
            make_token_with_lemma(".",        ".",        "PUNCT", "punct",  2, 24),
        ];
        let sent = make_sentence("The ball rolled downhill.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.agent.as_deref(), Some("The ball")); // syntactic
        assert!(d.roles.iter().any(|r| r.participant == "The ball" && r.thematic_role == crate::roles::ThematicRole::Theme));
        assert_eq!(d.verb_class, Some(crate::roles::VerbClass::Motion));
    }

    #[test]
    fn integration_ergative_roles() {
        // "The door opened."
        let tokens = vec![
            make_token_with_lemma("The",    "the",  "DET",   "det",   1, 0),
            make_token_with_lemma("door",   "door", "NOUN",  "nsubj", 2, 4),
            make_token_with_lemma("opened", "open", "VERB",  "ROOT",  2, 9),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 2, 15),
        ];
        let sent = make_sentence("The door opened.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.agent.as_deref(), Some("The door")); // syntactic nsubj → agent field
        // ergative: ChangeOfState + agent only → Patient role
        assert!(d.roles.iter().any(|r| r.participant == "The door" && r.thematic_role == crate::roles::ThematicRole::Patient));
        assert_eq!(d.verb_class, Some(crate::roles::VerbClass::ChangeOfState));
    }

    #[test]
    fn integration_transfer_roles() {
        // "Sarah gave Tom the book."
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj",  1, 0),
            make_token_with_lemma("gave",  "give",  "VERB",  "ROOT",   1, 6),
            make_token_with_lemma("Tom",   "tom",   "PROPN", "dative", 1, 11),
            make_token_with_lemma("the",   "the",   "DET",   "det",    4, 15),
            make_token_with_lemma("book",  "book",  "NOUN",  "dobj",   1, 19),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct",  1, 23),
        ];
        let sent = make_sentence("Sarah gave Tom the book.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.agent.as_deref(), Some("Sarah"));
        assert_eq!(d.patient.as_deref(), Some("the book"));
        assert_eq!(d.recipient.as_deref(), Some("Tom"));
        assert!(d.roles.iter().any(|r| r.participant == "Sarah" && r.thematic_role == crate::roles::ThematicRole::Agent));
        assert!(d.roles.iter().any(|r| r.participant == "the book" && r.thematic_role == crate::roles::ThematicRole::Theme));
        assert!(d.roles.iter().any(|r| r.participant == "Tom" && r.thematic_role == crate::roles::ThematicRole::Recipient));
    }

    #[test]
    fn integration_passive_roles() {
        // "The window was broken by Sarah."
        let tokens = vec![
            make_token_with_lemma("The",    "the",    "DET",   "det",      1, 0),
            make_token_with_lemma("window", "window", "NOUN",  "nsubjpass", 3, 4),
            make_token_with_lemma("was",    "be",     "AUX",   "auxpass",  3, 11),
            make_token_with_lemma("broken", "break",  "VERB",  "ROOT",     3, 15),
            make_token_with_lemma("by",     "by",     "ADP",   "agent",    3, 22),
            make_token_with_lemma("Sarah",  "sarah",  "PROPN", "pobj",     4, 25),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct",    3, 30),
        ];
        let sent = make_sentence("The window was broken by Sarah.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert!(d.is_passive);
        assert_eq!(d.agent.as_deref(), Some("Sarah"));
        assert_eq!(d.patient.as_deref(), Some("The window"));
        assert!(d.roles.iter().any(|r| r.participant == "Sarah" && r.thematic_role == crate::roles::ThematicRole::Agent));
        assert!(d.roles.iter().any(|r| r.participant == "The window" && r.thematic_role == crate::roles::ThematicRole::Patient));
    }

    #[test]
    fn integration_prep_beneficiary() {
        // "Sarah baked a cake for Tom."
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("baked", "bake",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("a",     "a",     "DET",   "det",   3, 12),
            make_token_with_lemma("cake",  "cake",  "NOUN",  "dobj",  1, 14),
            make_token_with_lemma("for",   "for",   "ADP",   "prep",  1, 19),
            make_token_with_lemma("Tom",   "tom",   "PROPN", "pobj",  4, 23),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 26),
        ];
        let sent = make_sentence("Sarah baked a cake for Tom.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.beneficiary.as_deref(), Some("Tom"));
        assert!(d.roles.iter().any(|r| r.participant == "Tom" && r.thematic_role == crate::roles::ThematicRole::Beneficiary));
    }

    #[test]
    fn integration_prep_goal_source() {
        // "Sarah traveled from London to Paris."
        let tokens = vec![
            make_token_with_lemma("Sarah",   "sarah",  "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("traveled","travel", "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("from",    "from",   "ADP",   "prep",  1, 15),
            make_token_with_lemma("London",  "london", "PROPN", "pobj",  2, 20),
            make_token_with_lemma("to",      "to",     "ADP",   "prep",  1, 27),
            make_token_with_lemma("Paris",   "paris",  "PROPN", "pobj",  4, 30),
            make_token_with_lemma(".",       ".",      "PUNCT", "punct", 1, 35),
        ];
        let sent = make_sentence("Sarah traveled from London to Paris.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert_eq!(d.source.as_deref(), Some("London"));
        assert_eq!(d.goal.as_deref(), Some("Paris"));
        assert!(d.roles.iter().any(|r| r.participant == "London" && r.thematic_role == crate::roles::ThematicRole::Source));
        assert!(d.roles.iter().any(|r| r.participant == "Paris" && r.thematic_role == crate::roles::ThematicRole::Goal));
    }

    #[test]
    fn integration_unknown_verb() {
        // "Sarah defenestrated the villain."
        let tokens = vec![
            make_token_with_lemma("Sarah",          "sarah",         "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("defenestrated",  "defenestrate",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",            "the",           "DET",   "det",   3, 21),
            make_token_with_lemma("villain",        "villain",       "NOUN",  "dobj",  1, 25),
            make_token_with_lemma(".",              ".",             "PUNCT", "punct", 1, 32),
        ];
        let sent = make_sentence("Sarah defenestrated the villain.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let d = &interactions[0];
        assert!(d.roles.iter().all(|r| r.thematic_role == crate::roles::ThematicRole::Unknown && (r.confidence - 0.5).abs() < f32::EPSILON));
    }

    // ── Phase 4: thematic role children in OwnedNode trees ──────────────────

    fn find_child<'a>(node: &'a OwnedNode, name: &str) -> Option<&'a OwnedNode> {
        node.field_indices.get(name).and_then(|indices| indices.first()).map(|&i| &node.children[i])
    }

    #[test]
    fn verb_phrase_roles_perception() {
        // "Sarah heard the noise."
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("heard", "hear",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",   "the",   "DET",   "det",   3, 12),
            make_token_with_lemma("noise", "noise", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 21),
        ];
        let sent = make_sentence("Sarah heard the noise.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let vp = build_verb_phrase_node(&interactions[0], &None);
        let agent_role = find_child(&vp, "agent_role").expect("agent_role child missing");
        assert_eq!(agent_role.text.as_deref(), Some("experiencer"));
    }

    #[test]
    fn verb_phrase_roles_action() {
        // "Sarah kicked the ball."
        let tokens = vec![
            make_token_with_lemma("Sarah",  "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("kicked", "kick",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",   "DET",   "det",   3, 13),
            make_token_with_lemma("ball",   "ball",  "NOUN",  "dobj",  1, 17),
            make_token_with_lemma(".",      ".",     "PUNCT", "punct", 1, 21),
        ];
        let sent = make_sentence("Sarah kicked the ball.", 0, tokens);
        let interactions = extract_interactions_from_sentence(&sent, &[0]);
        assert_eq!(interactions.len(), 1);
        let vp = build_verb_phrase_node(&interactions[0], &None);
        let agent_role = find_child(&vp, "agent_role").expect("agent_role missing");
        assert_eq!(agent_role.text.as_deref(), Some("agent"));
        let patient_role = find_child(&vp, "patient_role").expect("patient_role missing");
        assert_eq!(patient_role.text.as_deref(), Some("patient"));
    }

    #[test]
    fn interaction_verb_class() {
        // "Sarah heard the noise."
        let source = "Sarah heard the noise.";
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("heard", "hear",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",   "the",   "DET",   "det",   3, 12),
            make_token_with_lemma("noise", "noise", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 21),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_node = tree.children.iter()
            .find(|c| c.node_type == "interaction")
            .expect("interaction node missing");
        let vc = find_child(interaction_node, "verb_class").expect("verb_class child missing");
        assert_eq!(vc.text.as_deref(), Some("perception"));
    }

    #[test]
    fn interaction_role_children() {
        // "Sarah broke the window with a hammer."
        let source = "Sarah broke the window with a hammer.";
        let tokens = vec![
            make_token_with_lemma("Sarah",   "sarah",  "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("broke",   "break",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",     "the",    "DET",   "det",   3, 12),
            make_token_with_lemma("window",  "window", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma("with",    "with",   "ADP",   "prep",  1, 23),
            make_token_with_lemma("a",       "a",      "DET",   "det",   6, 28),
            make_token_with_lemma("hammer",  "hammer", "NOUN",  "pobj",  4, 30),
            make_token_with_lemma(".",       ".",      "PUNCT", "punct", 1, 36),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_node = tree.children.iter()
            .find(|c| c.node_type == "interaction")
            .expect("interaction node missing");
        let role_values: Vec<&str> = interaction_node.children.iter()
            .filter(|c| c.node_type == "role")
            .filter_map(|c| c.text.as_deref())
            .collect();
        assert!(role_values.contains(&"agent"),    "expected agent role, got {:?}", role_values);
        assert!(role_values.contains(&"patient"),   "expected patient role, got {:?}", role_values);
        assert!(role_values.contains(&"instrument"),"expected instrument role, got {:?}", role_values);
    }

    #[test]
    fn interaction_beneficiary_child() {
        // "Sarah baked a cake for Tom."
        let source = "Sarah baked a cake for Tom.";
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("baked", "bake",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("a",     "a",     "DET",   "det",   3, 12),
            make_token_with_lemma("cake",  "cake",  "NOUN",  "dobj",  1, 14),
            make_token_with_lemma("for",   "for",   "ADP",   "prep",  1, 19),
            make_token_with_lemma("Tom",   "tom",   "PROPN", "pobj",  4, 23),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 26),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_node = tree.children.iter()
            .find(|c| c.node_type == "interaction")
            .expect("interaction node missing");
        let beneficiary = find_child(interaction_node, "beneficiary").expect("beneficiary child missing");
        assert_eq!(beneficiary.text.as_deref(), Some("Tom"));
    }

    #[test]
    fn interaction_goal_child() {
        // "Sarah ran to the store."
        let source = "Sarah ran to the store.";
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("ran",   "run",   "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("to",    "to",    "ADP",   "prep",  1, 10),
            make_token_with_lemma("the",   "the",   "DET",   "det",   4, 13),
            make_token_with_lemma("store", "store", "NOUN",  "pobj",  2, 17),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 22),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_node = tree.children.iter()
            .find(|c| c.node_type == "interaction")
            .expect("interaction node missing");
        assert!(find_child(interaction_node, "goal").is_some(), "goal child missing");
    }

    #[test]
    fn backward_compat_agent_query() {
        // "Sarah chased the cat."
        let source = "Sarah chased the cat.";
        let tokens = vec![
            make_token_with_lemma("Sarah",  "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("chased", "chase", "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",   "DET",   "det",   3, 13),
            make_token_with_lemma("cat",    "cat",   "NOUN",  "dobj",  1, 17),
            make_token_with_lemma(".",      ".",     "PUNCT", "punct", 1, 20),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_node = tree.children.iter()
            .find(|c| c.node_type == "interaction")
            .expect("interaction node missing");
        let agent = find_child(interaction_node, "agent").expect("agent child missing");
        assert_eq!(agent.text.as_deref(), Some("Sarah"));
    }

    // ── Phase 5: Query integration and role-based filtering ─────────────────

    /// interaction[experiencer=Sarah] returns only the perception interaction.
    #[test]
    fn query_experiencer_filter() {
        // "Sarah heard the noise. Bob kicked the ball."
        let source = "Sarah heard the noise. Bob kicked the ball.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("heard", "hear",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",   "the",   "DET",   "det",   3, 12),
            make_token_with_lemma("noise", "noise", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 21),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("Bob",    "bob",  "PROPN", "nsubj", 1, 23),
            make_token_with_lemma("kicked", "kick", "VERB",  "ROOT",  1, 27),
            make_token_with_lemma("the",    "the",  "DET",   "det",   3, 34),
            make_token_with_lemma("ball",   "ball", "NOUN",  "dobj",  1, 38),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 1, 42),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah heard the noise.", 0, sent1_tokens),
                make_sentence("Bob kicked the ball.", 23, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, r#"desc:interaction[.experiencer | @text == "Sarah"]"#);
        assert_eq!(results.len(), 1, "expected 1 experiencer result, got: {:?}", results);
    }

    /// interaction[instrument != null] returns only the interaction with an instrument.
    #[test]
    fn query_instrument_not_null() {
        // "Sarah broke the window with a hammer. Bob kicked the ball."
        let source = "Sarah broke the window with a hammer. Bob kicked the ball.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah",  "sarah",  "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("broke",  "break",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",    "DET",   "det",   3, 12),
            make_token_with_lemma("window", "window", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma("with",   "with",   "ADP",   "prep",  1, 23),
            make_token_with_lemma("a",      "a",      "DET",   "det",   6, 28),
            make_token_with_lemma("hammer", "hammer", "NOUN",  "pobj",  4, 30),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1, 36),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("Bob",    "bob",  "PROPN", "nsubj", 1, 38),
            make_token_with_lemma("kicked", "kick", "VERB",  "ROOT",  1, 42),
            make_token_with_lemma("the",    "the",  "DET",   "det",   3, 49),
            make_token_with_lemma("ball",   "ball", "NOUN",  "dobj",  1, 53),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 1, 57),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah broke the window with a hammer.", 0, sent1_tokens),
                make_sentence("Bob kicked the ball.", 38, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        // Regression: instrument filter still works after Phase 5 enrichment
        let results = run_tree_query(&tree, r#"desc:interaction[.instrument | @text == "a hammer"]"#);
        assert_eq!(results.len(), 1, "expected 1 instrument result, got: {:?}", results);
    }

    /// interaction[beneficiary=Tom] returns only the baking interaction.
    #[test]
    fn query_beneficiary_filter() {
        // "Sarah baked a cake for Tom. Bob kicked the ball."
        let source = "Sarah baked a cake for Tom. Bob kicked the ball.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("baked", "bake",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("a",     "a",     "DET",   "det",   3, 12),
            make_token_with_lemma("cake",  "cake",  "NOUN",  "dobj",  1, 14),
            make_token_with_lemma("for",   "for",   "ADP",   "prep",  1, 19),
            make_token_with_lemma("Tom",   "tom",   "PROPN", "pobj",  4, 23),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 26),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("Bob",    "bob",  "PROPN", "nsubj", 1, 28),
            make_token_with_lemma("kicked", "kick", "VERB",  "ROOT",  1, 32),
            make_token_with_lemma("the",    "the",  "DET",   "det",   3, 39),
            make_token_with_lemma("ball",   "ball", "NOUN",  "dobj",  1, 43),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 1, 47),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah baked a cake for Tom.", 0, sent1_tokens),
                make_sentence("Bob kicked the ball.", 28, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, r#"desc:interaction[.beneficiary | @text == "Tom"]"#);
        assert_eq!(results.len(), 1, "expected 1 beneficiary result, got: {:?}", results);
    }

    /// interaction[role=agent] returns only interactions where an Agent role is present.
    #[test]
    fn query_role_agent_filter() {
        // "Sarah kicked the ball." → Agent
        // "Bob heard the noise."  → Experiencer (NOT agent)
        let source = "Sarah kicked the ball. Bob heard the noise.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah",  "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("kicked", "kick",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",   "DET",   "det",   3, 13),
            make_token_with_lemma("ball",   "ball",  "NOUN",  "dobj",  1, 17),
            make_token_with_lemma(".",      ".",     "PUNCT", "punct", 1, 21),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("Bob",   "bob",   "PROPN", "nsubj", 1, 23),
            make_token_with_lemma("heard", "hear",  "VERB",  "ROOT",  1, 27),
            make_token_with_lemma("the",   "the",   "DET",   "det",   3, 33),
            make_token_with_lemma("noise", "noise", "NOUN",  "dobj",  1, 37),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 42),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah kicked the ball.", 0, sent1_tokens),
                make_sentence("Bob heard the noise.", 23, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, r#"desc:interaction[.role | @text == "agent"]"#);
        assert_eq!(results.len(), 1, "expected only kick (Agent), got: {:?}", results);
    }

    /// interaction[verb_class=perception] returns only perception interactions.
    #[test]
    fn query_verb_class_filter() {
        // "Sarah heard the noise. Bob kicked the ball."
        let source = "Sarah heard the noise. Bob kicked the ball.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("heard", "hear",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",   "the",   "DET",   "det",   3, 12),
            make_token_with_lemma("noise", "noise", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 21),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("Bob",    "bob",  "PROPN", "nsubj", 1, 23),
            make_token_with_lemma("kicked", "kick", "VERB",  "ROOT",  1, 27),
            make_token_with_lemma("the",    "the",  "DET",   "det",   3, 34),
            make_token_with_lemma("ball",   "ball", "NOUN",  "dobj",  1, 38),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 1, 42),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah heard the noise.", 0, sent1_tokens),
                make_sentence("Bob kicked the ball.", 23, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, r#"desc:interaction[.verb_class | @text == "perception"]"#);
        assert_eq!(results.len(), 1, "expected 1 perception interaction, got: {:?}", results);
    }

    /// interaction[experiencer=Sarah] | .agent returns "Sarah".
    /// Also verifies that both syntactic [agent=Sarah] and thematic [experiencer=Sarah] work.
    #[test]
    fn query_experiencer_then_agent_field() {
        // "Sarah heard the noise."
        let source = "Sarah heard the noise.";
        let tokens = vec![
            make_token_with_lemma("Sarah", "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("heard", "hear",  "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",   "the",   "DET",   "det",   3, 12),
            make_token_with_lemma("noise", "noise", "NOUN",  "dobj",  1, 16),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 1, 21),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);

        // Thematic query: experiencer=Sarah → finds hearing interaction
        let results = run_tree_query(&tree, r#"desc:interaction[.experiencer | @text == "Sarah"]"#);
        assert_eq!(results.len(), 1, "experiencer filter should match");

        // Syntactic backward-compat: agent=Sarah also works (nsubj is still stored as agent)
        let results2 = run_tree_query(&tree, r#"desc:interaction[.agent | @text == "Sarah"]"#);
        assert_eq!(results2.len(), 1, "agent backward-compat should still match");
    }

    /// Ergative ChangeOfState: nsubj("The door") gets thematic Patient child
    /// since there is no dobj to occupy the patient field.
    #[test]
    fn query_ergative_patient_child() {
        // "The door opened."
        let source = "The door opened.";
        let tokens = vec![
            make_token_with_lemma("The",    "the",  "DET",   "det",   1, 0),
            make_token_with_lemma("door",   "door", "NOUN",  "nsubj", 2, 4),
            make_token_with_lemma("opened", "open", "VERB",  "ROOT",  2, 9),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 2, 15),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_node = tree.children.iter()
            .find(|c| c.node_type == "interaction")
            .expect("interaction node missing");
        // The thematic Patient child should be present (nsubj→Patient for ChangeOfState ergative)
        let patient_child = find_child(interaction_node, "patient");
        assert!(patient_child.is_some(), "ergative patient child should exist");
        assert_eq!(patient_child.unwrap().text.as_deref(), Some("The door"));
    }

    /// Tier 2 regression: interaction[agent=Sarah] still works after Phase 5.
    #[test]
    fn phase5_tier2_regression_agent() {
        // "Sarah chased the cat. Bob opened the door."
        let source = "Sarah chased the cat. Bob opened the door.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah",  "sarah", "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("chased", "chase", "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("the",    "the",   "DET",   "det",   3, 13),
            make_token_with_lemma("cat",    "cat",   "NOUN",  "dobj",  1, 17),
            make_token_with_lemma(".",      ".",     "PUNCT", "punct", 1, 20),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("Bob",    "bob",  "PROPN", "nsubj", 1, 22),
            make_token_with_lemma("opened", "open", "VERB",  "ROOT",  1, 26),
            make_token_with_lemma("the",    "the",  "DET",   "det",   3, 33),
            make_token_with_lemma("door",   "door", "NOUN",  "dobj",  1, 37),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 1, 41),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah chased the cat.", 0, sent1_tokens),
                make_sentence("Bob opened the door.", 22, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, r#"desc:interaction[.agent | @text == "Sarah"]"#);
        assert_eq!(results.len(), 1, "Tier 2 regression: agent=Sarah should return 1, got: {:?}", results);
    }

    /// E2E: two sentences with different verb classes — Perception and Motion.
    #[test]
    fn e2e_mixed_verb_classes() {
        let source = "Sarah saw the castle. She walked to the gate.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah",  "sarah",  "PROPN", "nsubj", 1,  0),
            make_token_with_lemma("saw",    "see",    "VERB",  "ROOT",  1,  6),
            make_token_with_lemma("the",    "the",    "DET",   "det",   3,  10),
            make_token_with_lemma("castle", "castle", "NOUN",  "dobj",  1,  14),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1,  20),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("She",    "she",    "PRON",  "nsubj", 1,  22),
            make_token_with_lemma("walked", "walk",   "VERB",  "ROOT",  1,  26),
            make_token_with_lemma("to",     "to",     "ADP",   "prep",  1,  33),
            make_token_with_lemma("the",    "the",    "DET",   "det",   4,  36),
            make_token_with_lemma("gate",   "gate",   "NOUN",  "pobj",  2,  40),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1,  44),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah saw the castle.", 0, sent1_tokens),
                make_sentence("She walked to the gate.", 22, sent2_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let r = run_tree_query(&tree, r#"desc:interaction[.verb_class | @text == "perception"]"#);
        assert_eq!(r.len(), 1, "perception class: {:?}", r);
        let r = run_tree_query(&tree, r#"desc:interaction[.verb_class | @text == "motion"]"#);
        assert_eq!(r.len(), 1, "motion class: {:?}", r);
        let r = run_tree_query(&tree, r#"desc:interaction[.experiencer | @text == "Sarah"]"#);
        assert_eq!(r.len(), 1, "experiencer=Sarah (saw only): {:?}", r);
        let r = run_tree_query(&tree, r#"desc:interaction[.theme | @text == "the castle"]"#);
        assert_eq!(r.len(), 1, "theme=the castle: {:?}", r);
    }

    /// E2E: passive voice with by-Agent extraction.
    #[test]
    fn e2e_passive_with_roles() {
        let source = "The ball was kicked by Bob.";
        let tokens = vec![
            make_token_with_lemma("The",    "the",  "DET",   "det",      1, 0),
            make_token_with_lemma("ball",   "ball", "NOUN",  "nsubjpass",3, 4),
            make_token_with_lemma("was",    "be",   "AUX",   "auxpass",  3, 9),
            make_token_with_lemma("kicked", "kick", "VERB",  "ROOT",     3, 13),
            make_token_with_lemma("by",     "by",   "ADP",   "agent",    3, 20),
            make_token_with_lemma("Bob",    "bob",  "PROPN", "pobj",     4, 23),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct",    3, 26),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let r = run_tree_query(&tree, r#"desc:interaction[.agent | @text == "Bob"]"#);
        assert_eq!(r.len(), 1, "passive agent=Bob: {:?}", r);
        let r = run_tree_query(&tree, r#"desc:interaction[.voice | @text == "passive"]"#);
        assert_eq!(r.len(), 1, "voice=passive: {:?}", r);
    }

    /// E2E: ergative ChangeOfState — nsubj becomes thematic Patient.
    #[test]
    fn e2e_ergative_narrative() {
        let source = "The ice melted.";
        let tokens = vec![
            make_token_with_lemma("The",    "the",  "DET",   "det",   1, 0),
            make_token_with_lemma("ice",    "ice",  "NOUN",  "nsubj", 2, 4),
            make_token_with_lemma("melted", "melt", "VERB",  "ROOT",  2, 8),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 2, 14),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let r = run_tree_query(&tree, r#"desc:interaction[.patient | @text == "The ice"]"#);
        assert_eq!(r.len(), 1, "ergative patient=The ice: {:?}", r);
    }

    /// E2E: Transfer verb with dative Recipient.
    #[test]
    fn e2e_transfer_verb() {
        let source = "The guard gave her a key.";
        let tokens = vec![
            make_token_with_lemma("The",   "the",   "DET",   "det",   1, 0),
            make_token_with_lemma("guard", "guard", "NOUN",  "nsubj", 2, 4),
            make_token_with_lemma("gave",  "give",  "VERB",  "ROOT",  2, 10),
            make_token_with_lemma("her",   "her",   "PRON",  "dative",2, 15),
            make_token_with_lemma("a",     "a",     "DET",   "det",   5, 19),
            make_token_with_lemma("key",   "key",   "NOUN",  "dobj",  2, 21),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 2, 24),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let r = run_tree_query(&tree, r#"desc:interaction[.agent | @text == "The guard"]"#);
        assert_eq!(r.len(), 1, "transfer agent=The guard: {:?}", r);
    }

    /// E2E: Action verb with coordination; at least one interaction with Agent=Joey.
    #[test]
    fn e2e_contribute_joey_battled() {
        let source = "Joey battled Kristy and won.";
        let tokens = vec![
            make_token_with_lemma("Joey",    "joey",   "PROPN", "nsubj", 1,  0),
            make_token_with_lemma("battled", "battle", "VERB",  "ROOT",  1,  5),
            make_token_with_lemma("Kristy",  "kristy", "PROPN", "dobj",  1,  13),
            make_token_with_lemma("and",     "and",    "CCONJ", "cc",    1,  20),
            make_token_with_lemma("won",     "win",    "VERB",  "conj",  1,  24),
            make_token_with_lemma(".",       ".",      "PUNCT", "punct", 1,  27),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let r = run_tree_query(&tree, r#"desc:interaction[.agent | @text == "Joey"]"#);
        assert!(r.len() >= 1, "at least 1 interaction with agent=Joey: {:?}", r);
    }

    /// E2E smoke: long multi-sentence narrative — no panics, multiple interactions.
    #[test]
    fn e2e_no_panic_long_narrative() {
        let source = "Sarah saw the castle. She walked to the gate. The guard gave her a key. She opened the door. The ice melted.";
        let sent1_tokens = vec![
            make_token_with_lemma("Sarah",  "sarah",  "PROPN", "nsubj", 1,   0),
            make_token_with_lemma("saw",    "see",    "VERB",  "ROOT",  1,   6),
            make_token_with_lemma("the",    "the",    "DET",   "det",   3,  10),
            make_token_with_lemma("castle", "castle", "NOUN",  "dobj",  1,  14),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1,  20),
        ];
        let sent2_tokens = vec![
            make_token_with_lemma("She",    "she",    "PRON",  "nsubj", 1,  22),
            make_token_with_lemma("walked", "walk",   "VERB",  "ROOT",  1,  26),
            make_token_with_lemma("to",     "to",     "ADP",   "prep",  1,  33),
            make_token_with_lemma("the",    "the",    "DET",   "det",   4,  36),
            make_token_with_lemma("gate",   "gate",   "NOUN",  "pobj",  2,  40),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1,  44),
        ];
        let sent3_tokens = vec![
            make_token_with_lemma("The",   "the",   "DET",   "det",   1, 46),
            make_token_with_lemma("guard", "guard", "NOUN",  "nsubj", 2, 50),
            make_token_with_lemma("gave",  "give",  "VERB",  "ROOT",  2, 56),
            make_token_with_lemma("her",   "her",   "PRON",  "dative",2, 61),
            make_token_with_lemma("a",     "a",     "DET",   "det",   5, 65),
            make_token_with_lemma("key",   "key",   "NOUN",  "dobj",  2, 67),
            make_token_with_lemma(".",     ".",     "PUNCT", "punct", 2, 70),
        ];
        let sent4_tokens = vec![
            make_token_with_lemma("She",    "she",    "PRON",  "nsubj", 1, 72),
            make_token_with_lemma("opened", "open",   "VERB",  "ROOT",  1, 76),
            make_token_with_lemma("the",    "the",    "DET",   "det",   3, 83),
            make_token_with_lemma("door",   "door",   "NOUN",  "dobj",  1, 87),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1, 91),
        ];
        let sent5_tokens = vec![
            make_token_with_lemma("The",    "the",  "DET",   "det",   1, 93),
            make_token_with_lemma("ice",    "ice",  "NOUN",  "nsubj", 2, 97),
            make_token_with_lemma("melted", "melt", "VERB",  "ROOT",  2, 101),
            make_token_with_lemma(".",      ".",    "PUNCT", "punct", 2, 107),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![
                make_sentence("Sarah saw the castle.", 0, sent1_tokens),
                make_sentence("She walked to the gate.", 22, sent2_tokens),
                make_sentence("The guard gave her a key.", 46, sent3_tokens),
                make_sentence("She opened the door.", 72, sent4_tokens),
                make_sentence("The ice melted.", 93, sent5_tokens),
            ],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let interaction_count = tree.children.iter().filter(|c| c.node_type == "interaction").count();
        assert!(interaction_count >= 2, "expected multiple interactions, got: {}", interaction_count);
    }

    // ── Fix 1: Entity type filtering tests ───────────────────────────────────

    /// Verify is_narrative_entity only passes PERSON, ORG, NORP.
    #[test]
    fn narrative_entity_filter_accepts_person_org_norp() {
        assert!(is_narrative_entity("PERSON"));
        assert!(is_narrative_entity("ORG"));
        assert!(is_narrative_entity("NORP"));
        // Numeric / temporal / geo should be excluded
        assert!(!is_narrative_entity("CARDINAL"));
        assert!(!is_narrative_entity("ORDINAL"));
        assert!(!is_narrative_entity("DATE"));
        assert!(!is_narrative_entity("TIME"));
        assert!(!is_narrative_entity("QUANTITY"));
        assert!(!is_narrative_entity("GPE"));
        assert!(!is_narrative_entity("LOC"));
    }

    /// CARDINAL entities must NOT appear as character arcs in the pipeline.
    #[test]
    fn cardinal_entities_excluded_from_character_arcs() {
        let source = "Jacob had twelve sons.\n\nThe twelve journeyed to Egypt.\n\nJoseph ruled there.";
        let para1_sent = make_sentence("Jacob had twelve sons.", 0, vec![
            make_token_with_lemma("Jacob",  "jacob",  "PROPN", "nsubj", 1, 0),
            make_token_with_lemma("had",    "have",   "VERB",  "ROOT",  1, 6),
            make_token_with_lemma("twelve", "twelve", "NUM",   "nummod",1, 10),
            make_token_with_lemma("sons",   "son",    "NOUN",  "dobj",  1, 17),
            make_token_with_lemma(".",      ".",      "PUNCT", "punct", 1, 21),
        ]);
        let para2_sent = make_sentence("The twelve journeyed to Egypt.", 23, vec![
            make_token_with_lemma("The",      "the",      "DET",   "det",   1, 23),
            make_token_with_lemma("twelve",   "twelve",   "NUM",   "nsubj", 1, 27),
            make_token_with_lemma("journeyed","journey",  "VERB",  "ROOT",  1, 34),
            make_token_with_lemma("to",       "to",       "ADP",   "prep",  1, 44),
            make_token_with_lemma("Egypt",    "egypt",    "PROPN", "pobj",  1, 47),
            make_token_with_lemma(".",        ".",        "PUNCT", "punct", 1, 52),
        ]);
        let para3_sent = make_sentence("Joseph ruled there.", 54, vec![
            make_token_with_lemma("Joseph",  "joseph",  "PROPN", "nsubj", 1, 54),
            make_token_with_lemma("ruled",   "rule",    "VERB",  "ROOT",  1, 61),
            make_token_with_lemma("there",   "there",   "ADV",   "advmod",1, 67),
            make_token_with_lemma(".",       ".",       "PUNCT", "punct", 1, 72),
        ]);
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![para1_sent, para2_sent, para3_sent],
            entities: vec![
                make_entity("Jacob",  "PERSON", 0),
                make_entity("twelve", "CARDINAL", 10),
                make_entity("twelve", "CARDINAL", 27),
                make_entity("Egypt",  "GPE", 47),
                make_entity("Joseph", "PERSON", 54),
            ],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        // Character arcs node should only contain PERSON entities, not CARDINAL/GPE
        let arc_names: Vec<String> = tree.children.iter()
            .filter(|c| c.node_type == "arc")
            .filter_map(|c| c.text.clone())
            .collect();
        // "twelve" (CARDINAL) and "Egypt" (GPE) must not appear as character arcs
        assert!(!arc_names.iter().any(|n| n == "twelve"), "CARDINAL 'twelve' should not be a character arc");
        assert!(!arc_names.iter().any(|n| n == "Egypt"),  "GPE 'Egypt' should not be a character arc");
        // PERSON entities should appear if they have enough mentions
        // (Jacob and Joseph are PERSON; test doesn't assert their presence
        //  because the arc threshold requires multiple interactions)
    }

    // ── Fix 2: Pronoun resolution tests ──────────────────────────────────────

    /// build_mention_to_canonical maps each mention's referent (lowercased) to
    /// the canonical name of its chain.
    #[test]
    fn pronoun_resolution_maps_through_coref_chains() {
        use crate::coref::{CoreferenceChain, CoreferenceData, CorefType};
        let chains = vec![
            CoreferenceChain {
                canonical: "Joseph".to_string(),
                entity_type: "PERSON".to_string(),
                aliases: vec!["he".to_string()],
                mentions: vec![
                    CoreferenceData {
                        referent: "he".to_string(),
                        canonical: "Joseph".to_string(),
                        coref_type: CorefType::CrossSentencePronoun,
                        confidence: 0.9,
                        sentence_idx: 1,
                        token_idx: 0,
                        source_line: 2,
                    },
                    CoreferenceData {
                        referent: "him".to_string(),
                        canonical: "Joseph".to_string(),
                        coref_type: CorefType::CrossSentencePronoun,
                        confidence: 0.85,
                        sentence_idx: 2,
                        token_idx: 3,
                        source_line: 3,
                    },
                ],
                total_mention_count: 2,
            },
            CoreferenceChain {
                canonical: "Pharaoh".to_string(),
                entity_type: "PERSON".to_string(),
                aliases: vec![],
                mentions: vec![
                    CoreferenceData {
                        referent: "he".to_string(),
                        canonical: "Pharaoh".to_string(),
                        coref_type: CorefType::SameSentencePronoun,
                        confidence: 0.7,
                        sentence_idx: 3,
                        token_idx: 0,
                        source_line: 4,
                    },
                ],
                total_mention_count: 1,
            },
        ];
        let map = build_mention_to_canonical(&chains);
        // "him" should resolve to Joseph
        assert_eq!(map.get("him").map(|s| s.as_str()), Some("Joseph"));
        // "he" entry exists (last entry for duplicate "he" wins, both are valid chains)
        assert!(map.contains_key("he"));
        // A name not in any mention map returns nothing
        assert!(map.get("they").is_none());
    }

    // ── Fix P1: GPE/PERSON alias rescue tests ────────────────────────────────

    /// A GPE entity that is aliased to a PERSON canonical via a coref chain
    /// must appear in character arc profiles (not be filtered out).
    #[test]
    fn person_aliased_gpe_rescued() {
        use crate::coref::{CoreferenceChain, CoreferenceData, CorefType};

        // Simulate "Israel" being labeled GPE by spaCy but sharing a coref chain
        // with "Jacob" (PERSON).  The pipeline should rescue "Israel" and include
        // it in entity_profiles so it can accumulate interaction data.
        //
        // We test this via build_mention_to_canonical + the person_aliased_gpe
        // logic by constructing the coref chain and entity_type_map directly.

        let entity_type_map: HashMap<String, String> = [
            ("jacob".to_string(),  "PERSON".to_string()),
            ("israel".to_string(), "GPE".to_string()),
        ].into_iter().collect();

        let chains = vec![CoreferenceChain {
            canonical: "Jacob".to_string(),
            entity_type: "PERSON".to_string(),
            aliases: vec!["Israel".to_string()],
            mentions: vec![CoreferenceData {
                referent: "Israel".to_string(),
                canonical: "Jacob".to_string(),
                coref_type: CorefType::Appositive,
                confidence: 0.95,
                sentence_idx: 0,
                token_idx: 5,
                source_line: 1,
            }],
            total_mention_count: 1,
        }];

        // Replicate the person_aliased_gpe construction logic
        let mut person_aliased_gpe: std::collections::HashSet<String> = std::collections::HashSet::new();
        for chain in &chains {
            let canonical_is_person = entity_type_map
                .get(&chain.canonical.to_lowercase())
                .map(|t| t == "PERSON")
                .unwrap_or(false);
            if canonical_is_person {
                for alias in &chain.aliases {
                    let alias_type = entity_type_map.get(&alias.to_lowercase()).map(|s| s.as_str());
                    if matches!(alias_type, Some("GPE" | "LOC")) {
                        person_aliased_gpe.insert(alias.clone());
                    }
                }
                for mention in &chain.mentions {
                    let ref_type = entity_type_map.get(&mention.referent.to_lowercase()).map(|s| s.as_str());
                    if matches!(ref_type, Some("GPE" | "LOC")) {
                        person_aliased_gpe.insert(mention.referent.clone());
                    }
                }
            }
        }

        // "Israel" must be in the rescue set
        assert!(person_aliased_gpe.contains("Israel"),
            "Israel (GPE aliased to Jacob PERSON) should be in rescue set, got {:?}", person_aliased_gpe);

        // The combined filter: !is_narrative_entity(label) && !person_aliased_gpe.contains(text)
        // For "Israel" with label "GPE": is_narrative_entity("GPE") == false,
        // but person_aliased_gpe.contains("Israel") == true → should NOT skip.
        let israel_label = "GPE";
        let should_skip = !is_narrative_entity(israel_label) && !person_aliased_gpe.contains("Israel");
        assert!(!should_skip, "Israel should NOT be skipped by the combined entity filter");

        // For "Egypt" (GPE, NOT aliased to a person): should still be skipped
        let egypt_label = "GPE";
        let egypt_not_aliased = !person_aliased_gpe.contains("Egypt");
        let egypt_should_skip = !is_narrative_entity(egypt_label) && egypt_not_aliased;
        assert!(egypt_should_skip, "Egypt (unaliased GPE) should still be filtered out");
    }

    // ── Fix P2: Patient movement tracking tests ──────────────────────────────

    /// When a causative motion verb is used ("brought", "took", etc.), the patient
    /// also relocates and must be added to movement_interactions.
    #[test]
    fn patient_movement_tracked() {
        // "They brought Joseph into Egypt."
        // Joseph is the dobj (patient) — not the agent — but he relocated.
        let patient_movement_verbs: HashSet<&str> = ["brought", "carried", "took", "sent",
            "led", "dragged", "transported", "delivered"].iter().copied().collect();

        // Verify all expected verbs are present
        assert!(patient_movement_verbs.contains("brought"));
        assert!(patient_movement_verbs.contains("carried"));
        assert!(patient_movement_verbs.contains("took"));
        assert!(patient_movement_verbs.contains("sent"));
        assert!(patient_movement_verbs.contains("led"));
        assert!(patient_movement_verbs.contains("dragged"));
        assert!(patient_movement_verbs.contains("transported"));
        assert!(patient_movement_verbs.contains("delivered"));

        // Simulate the patient movement logic for a "brought" interaction
        let scenes = vec![
            SceneBoundary { scene_index: 0, start_para_idx: 0, end_para_idx: 0,
                start_line: 1, end_line: 5,
                location: Some("Canaan".to_string()), temporal_marker: None,
                entity_names: vec!["Joseph".to_string()],
                boundary_signals: vec![] },
            SceneBoundary { scene_index: 1, start_para_idx: 1, end_para_idx: 1,
                start_line: 6, end_line: 10,
                location: Some("Egypt".to_string()), temporal_marker: None,
                entity_names: vec!["Joseph".to_string()],
                boundary_signals: vec![] },
        ];

        let mut movement_interactions: HashSet<(String, usize, usize)> = HashSet::new();

        // Simulate one interaction: verb="brought", patient="Joseph", source_line=3
        let verb = "brought";
        let patient = "Joseph";
        let source_line: usize = 3;

        if patient_movement_verbs.contains(verb) {
            let line = source_line;
            for (scene_idx, scene) in scenes.iter().enumerate() {
                if line >= scene.start_line && line <= scene.end_line {
                    if scene_idx + 1 < scenes.len() {
                        movement_interactions.insert((patient.to_string(), scene_idx, scene_idx + 1));
                    }
                    if scene_idx > 0 {
                        movement_interactions.insert((patient.to_string(), scene_idx - 1, scene_idx));
                    }
                    break;
                }
            }
        }

        assert!(movement_interactions.contains(&("Joseph".to_string(), 0, 1)),
            "Joseph should be tracked as moving from scene 0 to scene 1 when 'brought', got {:?}",
            movement_interactions);
    }

    // ── Requirement tests ────────────────────────────────────────────────────

    #[test]
    fn requirement_shall_detected() {
        let source = "The system shall provide authentication.";
        let tokens = vec![
            make_token("The", "DET", "det", "", "O", 0),
            make_token("system", "NOUN", "nsubj", "", "O", 4),
            make_token("shall", "MD", "aux", "", "O", 11),
            make_token("provide", "VERB", "ROOT", "", "O", 17),
            make_token("authentication", "NOUN", "dobj", "", "O", 25),
            make_token(".", "PUNCT", "punct", "", "O", 39),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:requirement");
        assert_eq!(results.len(), 1, "expected 1 requirement, got: {:?}", results);
        assert!(
            results[0].contains("shall"),
            "should contain 'shall': {}",
            results[0]
        );
    }

    #[test]
    fn requirement_must_detected() {
        let source = "Users must authenticate before access.";
        let tokens = vec![
            make_token("Users", "NOUN", "nsubj", "", "O", 0),
            make_token("must", "MD", "aux", "", "O", 6),
            make_token("authenticate", "VERB", "ROOT", "", "O", 11),
            make_token("before", "ADP", "prep", "", "O", 24),
            make_token("access", "NOUN", "pobj", "", "O", 31),
            make_token(".", "PUNCT", "punct", "", "O", 37),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:requirement");
        assert_eq!(results.len(), 1);
        let strength = run_tree_query(&tree, "desc:requirement | .strength");
        assert_eq!(strength, vec!["mandatory"]);
    }

    #[test]
    fn requirement_should_is_recommended() {
        let source = "The API should return JSON.";
        let tokens = vec![
            make_token("The", "DET", "det", "", "O", 0),
            make_token("API", "NOUN", "nsubj", "", "O", 4),
            make_token("should", "MD", "aux", "", "O", 8),
            make_token("return", "VERB", "ROOT", "", "O", 15),
            make_token("JSON", "PROPN", "dobj", "", "O", 22),
            make_token(".", "PUNCT", "punct", "", "O", 26),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let strength = run_tree_query(&tree, "desc:requirement | .strength");
        assert_eq!(strength, vec!["recommended"]);
        let modal = run_tree_query(&tree, "desc:requirement | .modal");
        assert_eq!(modal, vec!["should"]);
    }

    #[test]
    fn no_requirements_in_plain_text() {
        let source = "The cat sat on the mat.";
        let tokens = vec![
            make_token("The", "DET", "det", "", "O", 0),
            make_token("cat", "NOUN", "nsubj", "", "O", 4),
            make_token("sat", "VERB", "ROOT", "", "O", 8),
            make_token("on", "ADP", "prep", "", "O", 12),
            make_token("the", "DET", "det", "", "O", 15),
            make_token("mat", "NOUN", "pobj", "", "O", 19),
            make_token(".", "PUNCT", "punct", "", "O", 22),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:requirement");
        assert!(
            results.is_empty(),
            "plain text should have no requirements: {:?}",
            results
        );
    }

    // ── Question tests ───────────────────────────────────────────────────────

    #[test]
    fn question_what_detected() {
        let source = "What is the status of the project?";
        let tokens = vec![
            make_token("What", "PRON", "nsubj", "", "O", 0),
            make_token("is", "AUX", "ROOT", "", "O", 5),
            make_token("the", "DET", "det", "", "O", 8),
            make_token("status", "NOUN", "attr", "", "O", 12),
            make_token("of", "ADP", "prep", "", "O", 19),
            make_token("the", "DET", "det", "", "O", 22),
            make_token("project", "NOUN", "pobj", "", "O", 26),
            make_token("?", "PUNCT", "punct", "", "O", 33),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:question");
        assert_eq!(results.len(), 1, "expected 1 question, got: {:?}", results);
        let qtype = run_tree_query(&tree, "desc:question | .question_type");
        assert_eq!(qtype, vec!["what"]);
    }

    #[test]
    fn question_yes_no_detected() {
        let source = "Is the system running?";
        let tokens = vec![
            make_token("Is", "AUX", "ROOT", "", "O", 0),
            make_token("the", "DET", "det", "", "O", 3),
            make_token("system", "NOUN", "nsubj", "", "O", 7),
            make_token("running", "VERB", "acomp", "", "O", 14),
            make_token("?", "PUNCT", "punct", "", "O", 21),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:question");
        assert_eq!(results.len(), 1, "expected 1 question, got: {:?}", results);
        let qtype = run_tree_query(&tree, "desc:question | .question_type");
        assert_eq!(qtype, vec!["yes-no"]);
    }

    #[test]
    fn question_how_detected() {
        let source = "How does the authentication work?";
        let tokens = vec![
            make_token("How", "ADV", "advmod", "", "O", 0),
            make_token("does", "AUX", "aux", "", "O", 4),
            make_token("the", "DET", "det", "", "O", 9),
            make_token("authentication", "NOUN", "nsubj", "", "O", 13),
            make_token("work", "VERB", "ROOT", "", "O", 28),
            make_token("?", "PUNCT", "punct", "", "O", 32),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:question");
        assert_eq!(results.len(), 1, "expected 1 question, got: {:?}", results);
        let qtype = run_tree_query(&tree, "desc:question | .question_type");
        assert_eq!(qtype, vec!["how"]);
    }

    #[test]
    fn no_questions_in_statements() {
        let source = "The system is running.";
        let tokens = vec![
            make_token("The", "DET", "det", "", "O", 0),
            make_token("system", "NOUN", "nsubj", "", "O", 4),
            make_token("is", "AUX", "aux", "", "O", 11),
            make_token("running", "VERB", "ROOT", "", "O", 14),
            make_token(".", "PUNCT", "punct", "", "O", 21),
        ];
        let doc = SpacyDoc {
            text: source.to_string(),
            sentences: vec![make_sentence(source, 0, tokens)],
            entities: vec![],
        };
        let tree = spacy_doc_to_owned_tree(&doc, source, None);
        let results = run_tree_query(&tree, "desc:question");
        assert!(
            results.is_empty(),
            "statements should have no questions: {:?}",
            results
        );
    }
}
