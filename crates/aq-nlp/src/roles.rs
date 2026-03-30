use std::fmt;

use crate::spacy::SpacyToken as SpacyTokenData;
use crate::tree::{collect_span_text, normalize_dep, InteractionData};

// ── Enums ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ThematicRole {
    Agent,       // Deliberate actor
    Patient,     // Entity changed or affected
    Experiencer, // Entity perceiving or feeling
    Theme,       // Entity described or moved without change
    Instrument,  // Means by which action occurs
    Beneficiary, // Entity benefiting
    Location,    // Spatial reference
    Goal,        // Destination of motion/change
    Source,      // Origin of motion/change
    Recipient,   // Entity receiving (ditransitives)
    Cause,       // Non-volitional causer ("The storm destroyed the house")
    Unknown,     // Verb not in lookup table
}

impl fmt::Display for ThematicRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ThematicRole::Agent => "agent",
            ThematicRole::Patient => "patient",
            ThematicRole::Experiencer => "experiencer",
            ThematicRole::Theme => "theme",
            ThematicRole::Instrument => "instrument",
            ThematicRole::Beneficiary => "beneficiary",
            ThematicRole::Location => "location",
            ThematicRole::Goal => "goal",
            ThematicRole::Source => "source",
            ThematicRole::Recipient => "recipient",
            ThematicRole::Cause => "cause",
            ThematicRole::Unknown => "unknown",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum VerbClass {
    Action,        // hit, kick, break, push, throw
    Perception,    // see, hear, feel, smell, taste
    Cognition,     // realize, understand, know, believe, think
    Emotion,       // fear, love, hate, want, need
    Motion,        // go, come, run, walk, fall, rise
    Transfer,      // give, send, tell, show, teach
    Creation,      // make, create, bake, paint, design
    Communication, // say, ask, answer, explain, declare
    ChangeOfState, // melt, open, close, freeze, burn, heal
    Stative,       // be, have, seem, appear, belong, exist
    Consumption,   // eat, drink, consume, devour
}

// ── Verb lookup table ─────────────────────────────────────────────────────────

/// Returns the primary VerbClass for a given verb lemma, or None if unknown.
pub(crate) fn verb_class(lemma: &str) -> Option<VerbClass> {
    match lemma {
        // Action (~45)
        "hit" | "kick" | "break" | "push" | "pull" | "throw" | "catch" | "grab" | "strike"
        | "punch" | "smash" | "crush" | "tear" | "rip" | "cut" | "chop" | "stab" | "shoot"
        | "kill" | "murder" | "attack" | "fight" | "battle" | "destroy" | "demolish" | "build"
        | "write" | "draw" | "carve" | "dig" | "lift" | "carry" | "drag" | "drop" | "place"
        | "put" | "set" | "hold" | "touch" | "bite" | "chew" => Some(VerbClass::Action),

        // Perception (~15)
        "see" | "hear" | "feel" | "smell" | "taste" | "notice" | "observe" | "watch" | "detect"
        | "sense" | "perceive" | "spot" | "glimpse" | "witness" | "overhear" => {
            Some(VerbClass::Perception)
        }

        // Cognition (~20)
        "realize" | "recognize" | "understand" | "know" | "believe" | "think" | "consider"
        | "remember" | "forget" | "recall" | "imagine" | "suppose" | "assume" | "conclude"
        | "decide" | "determine" | "discover" | "learn" | "wonder" | "doubt" => {
            Some(VerbClass::Cognition)
        }

        // Emotion (~24)
        "fear" | "love" | "hate" | "want" | "need" | "desire" | "enjoy" | "like" | "dislike"
        | "prefer" | "dread" | "worry" | "hope" | "wish" | "admire" | "respect" | "despise"
        | "loathe" | "envy" | "pity" | "miss" | "regret" | "appreciate" | "trust" => {
            Some(VerbClass::Emotion)
        }

        // Motion (~35)
        "go" | "come" | "run" | "walk" | "fall" | "rise" | "move" | "travel" | "arrive"
        | "depart" | "leave" | "enter" | "exit" | "return" | "fly" | "swim" | "crawl" | "climb"
        | "jump" | "leap" | "roll" | "slide" | "drift" | "float" | "flow" | "rush" | "hurry"
        | "wander" | "roam" | "flee" | "escape" | "chase" | "follow" | "approach" | "retreat" => {
            Some(VerbClass::Motion)
        }

        // Transfer (~23) — "return" goes to Motion above; "tell", "bring", "take" → Transfer
        "give" | "send" | "tell" | "show" | "teach" | "offer" | "lend" | "bring" | "take"
        | "pass" | "hand" | "deliver" | "provide" | "supply" | "grant" | "award" | "present"
        | "sell" | "buy" | "pay" | "owe" | "trade" => Some(VerbClass::Transfer),

        // Creation (~20)
        "make" | "create" | "compose" | "cook" | "bake" | "paint" | "design" | "produce"
        | "generate" | "manufacture" | "construct" | "assemble" | "craft" | "forge" | "brew"
        | "invent" | "develop" | "prepare" | "establish" | "found" => Some(VerbClass::Creation),

        // Communication (~27)
        "say" | "speak" | "talk" | "ask" | "answer" | "reply" | "respond" | "explain"
        | "describe" | "announce" | "declare" | "claim" | "argue" | "suggest" | "propose"
        | "state" | "report" | "mention" | "discuss" | "shout" | "whisper" | "murmur" | "sing"
        | "chant" | "read" | "call" => Some(VerbClass::Communication),

        // ChangeOfState (~43)
        "melt" | "freeze" | "boil" | "evaporate" | "dissolve" | "harden" | "soften" | "dry"
        | "wet" | "warm" | "cool" | "heat" | "burn" | "ignite" | "extinguish" | "open"
        | "close" | "shut" | "lock" | "unlock" | "tie" | "untie" | "fold" | "unfold" | "bend"
        | "straighten" | "stretch" | "shrink" | "grow" | "expand" | "contract" | "fill"
        | "empty" | "clean" | "stain" | "fix" | "repair" | "damage" | "crack" | "shatter"
        | "split" | "heal" | "cure" | "wake" | "awaken" => Some(VerbClass::ChangeOfState),

        // Stative (~30)
        "be" | "have" | "seem" | "appear" | "belong" | "contain" | "exist" | "remain" | "stay"
        | "last" | "endure" | "persist" | "consist" | "comprise" | "include" | "involve"
        | "resemble" | "equal" | "lack" | "own" | "possess" | "deserve" | "matter" | "mean"
        | "signify" | "represent" | "constitute" | "form" => Some(VerbClass::Stative),

        // Consumption (~12)
        "eat" | "drink" | "consume" | "devour" | "swallow" | "gulp" | "sip" | "nibble"
        | "feast" | "dine" | "gorge" | "inhale" => Some(VerbClass::Consumption),

        _ => None,
    }
}

// ── Role frame mapping ────────────────────────────────────────────────────────

/// Maps a (VerbClass, normalized dependency label, is_passive) triple to a ThematicRole.
///
/// The `dep` string is expected to already be normalized by `tree::normalize_dep`:
///   "nsubj:pass" → "nsubjpass", "obj" → "dobj", etc.
pub(crate) fn role_frame(class: VerbClass, dep: &str, is_passive: bool) -> ThematicRole {
    match class {
        VerbClass::Action => match dep {
            "nsubj" if !is_passive => ThematicRole::Agent,
            "nsubj" if is_passive => ThematicRole::Patient,
            "dobj" => ThematicRole::Patient,
            "iobj" | "dative" => ThematicRole::Recipient,
            "agent" => ThematicRole::Agent,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Perception => match dep {
            "nsubj" => ThematicRole::Experiencer,
            "dobj" => ThematicRole::Theme,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Cognition => match dep {
            "nsubj" => ThematicRole::Experiencer,
            "dobj" => ThematicRole::Theme,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Emotion => match dep {
            "nsubj" => ThematicRole::Experiencer,
            "dobj" => ThematicRole::Theme,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Motion => match dep {
            "nsubj" => ThematicRole::Theme,
            "dobj" => ThematicRole::Goal,
            "iobj" | "dative" => ThematicRole::Goal,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Transfer => match dep {
            "nsubj" if !is_passive => ThematicRole::Agent,
            "nsubj" if is_passive => ThematicRole::Theme,
            "dobj" => ThematicRole::Theme,
            "iobj" | "dative" => ThematicRole::Recipient,
            "agent" => ThematicRole::Agent,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Creation => match dep {
            "nsubj" => ThematicRole::Agent,
            "dobj" => ThematicRole::Theme,
            "iobj" | "dative" => ThematicRole::Beneficiary,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Communication => match dep {
            "nsubj" => ThematicRole::Agent,
            "dobj" => ThematicRole::Theme,
            "iobj" | "dative" => ThematicRole::Recipient,
            _ => ThematicRole::Unknown,
        },

        VerbClass::ChangeOfState => match dep {
            "nsubj" if !is_passive => ThematicRole::Agent,
            "nsubj" if is_passive => ThematicRole::Patient,
            "dobj" => ThematicRole::Patient,
            "agent" => ThematicRole::Cause,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Stative => match dep {
            "nsubj" => ThematicRole::Experiencer,
            "dobj" => ThematicRole::Theme,
            _ => ThematicRole::Unknown,
        },

        VerbClass::Consumption => match dep {
            "nsubj" => ThematicRole::Agent,
            "dobj" => ThematicRole::Patient,
            _ => ThematicRole::Unknown,
        },
    }
}

// ── Ergative detection ────────────────────────────────────────────────────────

/// Returns true for ergative uses: ChangeOfState verbs where the nsubj
/// syntactic subject is actually the Patient, not an Agent (spontaneous change).
///
/// Example: "The ice melted" — "ice" is Patient, not Agent.
pub(crate) fn is_ergative_use(verb_class: VerbClass, has_agent: bool, has_patient: bool) -> bool {
    verb_class == VerbClass::ChangeOfState && has_agent && !has_patient
}

// ── RoleAnnotation ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RoleAnnotation {
    pub participant: String,
    pub dep_relation: String,
    pub syntactic_role: String,
    pub thematic_role: ThematicRole,
    pub verb_class: Option<VerbClass>,
    pub confidence: f32,
}

// ── classify_roles ────────────────────────────────────────────────────────────

/// Assigns thematic roles to all participants of an interaction.
pub(crate) fn classify_roles(
    interaction: &InteractionData,
    tokens: &[SpacyTokenData],
) -> Vec<RoleAnnotation> {
    let mut annotations: Vec<RoleAnnotation> = Vec::new();
    let is_passive = interaction.is_passive;
    let has_agent = interaction.agent.is_some();
    let has_patient = interaction.patient.is_some();
    let vc = verb_class(&interaction.verb_lemma);

    const LIGHT_VERB_LEMMAS: &[&str] = &["take", "make", "have", "give", "do"];
    const LIGHT_VERB_COMPLEMENTS: &[&str] = &[
        "walk", "decision", "look", "attempt", "turn", "break", "step", "run", "bath", "nap",
        "rest", "bite", "sip", "ride", "trip",
    ];
    let is_light_verb = LIGHT_VERB_LEMMAS.contains(&interaction.verb_lemma.as_str())
        && interaction.patient.as_ref().is_some_and(|p| {
            let p_lower = p.to_lowercase();
            p_lower
                .split_whitespace()
                .any(|w| LIGHT_VERB_COMPLEMENTS.contains(&w))
        });

    const COPULAR_VERBS: &[&str] = &["be", "become", "seem", "appear", "remain"];
    let is_copular = COPULAR_VERBS.contains(&interaction.verb_lemma.as_str());

    if is_light_verb {
        if let Some(ref agent) = interaction.agent {
            annotations.push(RoleAnnotation {
                participant: agent.clone(),
                dep_relation: "nsubj".to_string(),
                syntactic_role: "agent".to_string(),
                thematic_role: ThematicRole::Agent,
                verb_class: vc,
                confidence: 0.6,
            });
        }
        // Skip patient — part of the light verb construction
    } else if is_copular {
        if let Some(ref agent) = interaction.agent {
            annotations.push(RoleAnnotation {
                participant: agent.clone(),
                dep_relation: "nsubj".to_string(),
                syntactic_role: "agent".to_string(),
                thematic_role: ThematicRole::Theme,
                verb_class: vc,
                confidence: 0.8,
            });
        }
        // Skip patient — it's a predicate complement, not a Patient
    } else if let Some(class) = vc {
        if let Some(ref agent) = interaction.agent {
            let agent_dep = if is_passive { "agent" } else { "nsubj" };
            let mut thematic = role_frame(class, agent_dep, is_passive);
            let mut conf: f32 = 1.0;
            if is_ergative_use(class, has_agent, has_patient) {
                thematic = ThematicRole::Patient;
                conf = 0.9;
            }
            annotations.push(RoleAnnotation {
                participant: agent.clone(),
                dep_relation: "nsubj".to_string(),
                syntactic_role: "agent".to_string(),
                thematic_role: thematic,
                verb_class: Some(class),
                confidence: conf,
            });
        }
        if let Some(ref patient) = interaction.patient {
            let patient_dep = if is_passive { "nsubj" } else { "dobj" };
            let thematic = role_frame(class, patient_dep, is_passive);
            let dep_rel = if is_passive { "nsubjpass" } else { "dobj" };
            annotations.push(RoleAnnotation {
                participant: patient.clone(),
                dep_relation: dep_rel.to_string(),
                syntactic_role: "patient".to_string(),
                thematic_role: thematic,
                verb_class: Some(class),
                confidence: 1.0,
            });
        }
        if let Some(ref instrument) = interaction.instrument {
            annotations.push(RoleAnnotation {
                participant: instrument.clone(),
                dep_relation: "prep_with".to_string(),
                syntactic_role: "instrument".to_string(),
                thematic_role: ThematicRole::Instrument,
                verb_class: Some(class),
                confidence: 0.95,
            });
        }
        if let Some(ref recipient) = interaction.recipient {
            let thematic = role_frame(class, "iobj", false);
            annotations.push(RoleAnnotation {
                participant: recipient.clone(),
                dep_relation: "iobj".to_string(),
                syntactic_role: "recipient".to_string(),
                thematic_role: thematic,
                verb_class: Some(class),
                confidence: 1.0,
            });
        }
    } else {
        // Unknown verb class
        if let Some(ref agent) = interaction.agent {
            annotations.push(RoleAnnotation {
                participant: agent.clone(),
                dep_relation: "nsubj".to_string(),
                syntactic_role: "agent".to_string(),
                thematic_role: ThematicRole::Unknown,
                verb_class: None,
                confidence: 0.5,
            });
        }
        if let Some(ref patient) = interaction.patient {
            annotations.push(RoleAnnotation {
                participant: patient.clone(),
                dep_relation: "dobj".to_string(),
                syntactic_role: "patient".to_string(),
                thematic_role: ThematicRole::Unknown,
                verb_class: None,
                confidence: 0.5,
            });
        }
        if let Some(ref instrument) = interaction.instrument {
            annotations.push(RoleAnnotation {
                participant: instrument.clone(),
                dep_relation: "prep_with".to_string(),
                syntactic_role: "instrument".to_string(),
                thematic_role: ThematicRole::Unknown,
                verb_class: None,
                confidence: 0.5,
            });
        }
        if let Some(ref recipient) = interaction.recipient {
            annotations.push(RoleAnnotation {
                participant: recipient.clone(),
                dep_relation: "iobj".to_string(),
                syntactic_role: "recipient".to_string(),
                thematic_role: ThematicRole::Unknown,
                verb_class: None,
                confidence: 0.5,
            });
        }
    }

    // Prepositional role scan
    for (tok_idx, token) in tokens.iter().enumerate() {
        if token.head != interaction.verb_idx || tok_idx == interaction.verb_idx {
            continue;
        }
        if normalize_dep(&token.dep) != "prep" {
            continue;
        }
        let lemma_lower = token.lemma.to_lowercase();
        // Skip instrument ("with") and passive agent ("by")
        if lemma_lower == "with" || lemma_lower == "by" {
            continue;
        }
        let pobj_idx = tokens.iter().enumerate().find_map(|(i, t)| {
            if t.head == tok_idx && normalize_dep(&t.dep) == "pobj" {
                Some(i)
            } else {
                None
            }
        });
        let Some(pobj_i) = pobj_idx else { continue };
        let pobj_text = collect_span_text(pobj_i, tokens);
        let (thematic, conf) = match lemma_lower.as_str() {
            "for" => (ThematicRole::Beneficiary, 0.9_f32),
            "to" => (ThematicRole::Goal, 0.9_f32),
            "from" => (ThematicRole::Source, 0.9_f32),
            "at" | "in" | "on" | "near" => (ThematicRole::Location, 0.85_f32),
            _ => continue,
        };
        annotations.push(RoleAnnotation {
            participant: pobj_text,
            dep_relation: format!("prep_{}", lemma_lower),
            syntactic_role: format!("prep_{}", lemma_lower),
            thematic_role: thematic,
            verb_class: vc,
            confidence: conf,
        });
    }

    annotations
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ThematicRole::*;
    use VerbClass::*;

    // ── verb_class spot tests ─────────────────────────────────────────────────

    #[test]
    fn verb_class_action() {
        assert_eq!(verb_class("kick"), Some(Action));
    }

    #[test]
    fn verb_class_perception() {
        assert_eq!(verb_class("hear"), Some(Perception));
    }

    #[test]
    fn verb_class_cognition() {
        assert_eq!(verb_class("understand"), Some(Cognition));
    }

    #[test]
    fn verb_class_emotion() {
        assert_eq!(verb_class("fear"), Some(Emotion));
    }

    #[test]
    fn verb_class_motion() {
        assert_eq!(verb_class("run"), Some(Motion));
    }

    #[test]
    fn verb_class_transfer() {
        assert_eq!(verb_class("give"), Some(Transfer));
    }

    #[test]
    fn verb_class_creation() {
        assert_eq!(verb_class("bake"), Some(Creation));
    }

    #[test]
    fn verb_class_communication() {
        assert_eq!(verb_class("say"), Some(Communication));
    }

    #[test]
    fn verb_class_change_of_state() {
        assert_eq!(verb_class("freeze"), Some(ChangeOfState));
    }

    #[test]
    fn verb_class_stative() {
        assert_eq!(verb_class("exist"), Some(Stative));
    }

    #[test]
    fn verb_class_consumption() {
        assert_eq!(verb_class("drink"), Some(Consumption));
    }

    #[test]
    fn verb_class_unknown() {
        assert_eq!(verb_class("xyzzy"), None);
    }

    #[test]
    fn verb_class_coverage_spot_check() {
        // Action — 10+
        for v in &[
            "hit", "push", "pull", "throw", "catch", "smash", "cut", "shoot", "destroy", "lift",
        ] {
            assert_eq!(verb_class(v), Some(Action), "Action: {v}");
        }
        // Perception — 10+
        for v in &[
            "see", "feel", "smell", "taste", "notice", "observe", "watch", "detect", "sense",
            "perceive",
        ] {
            assert_eq!(verb_class(v), Some(Perception), "Perception: {v}");
        }
        // Cognition — 10+
        for v in &[
            "realize",
            "recognize",
            "know",
            "believe",
            "think",
            "consider",
            "remember",
            "forget",
            "recall",
            "imagine",
        ] {
            assert_eq!(verb_class(v), Some(Cognition), "Cognition: {v}");
        }
        // Emotion — 10+
        for v in &[
            "love", "hate", "want", "need", "desire", "enjoy", "like", "dislike", "prefer", "dread",
        ] {
            assert_eq!(verb_class(v), Some(Emotion), "Emotion: {v}");
        }
        // Motion — 10+
        for v in &[
            "go", "come", "walk", "fall", "rise", "move", "travel", "arrive", "depart", "fly",
        ] {
            assert_eq!(verb_class(v), Some(Motion), "Motion: {v}");
        }
        // Transfer — 10+
        for v in &[
            "send", "tell", "show", "teach", "offer", "lend", "bring", "take", "pass", "hand",
        ] {
            assert_eq!(verb_class(v), Some(Transfer), "Transfer: {v}");
        }
        // Creation — 10+
        for v in &[
            "make",
            "create",
            "compose",
            "cook",
            "paint",
            "design",
            "produce",
            "generate",
            "manufacture",
            "construct",
        ] {
            assert_eq!(verb_class(v), Some(Creation), "Creation: {v}");
        }
        // Communication — 10+
        for v in &[
            "speak", "talk", "ask", "answer", "reply", "respond", "explain", "describe",
            "announce", "declare",
        ] {
            assert_eq!(verb_class(v), Some(Communication), "Communication: {v}");
        }
        // ChangeOfState — 10+
        for v in &[
            "melt",
            "boil",
            "evaporate",
            "dissolve",
            "harden",
            "soften",
            "dry",
            "burn",
            "open",
            "close",
        ] {
            assert_eq!(verb_class(v), Some(ChangeOfState), "ChangeOfState: {v}");
        }
        // Stative — 10+
        for v in &[
            "be", "have", "seem", "appear", "belong", "contain", "remain", "stay", "last", "endure",
        ] {
            assert_eq!(verb_class(v), Some(Stative), "Stative: {v}");
        }
        // Consumption — all 12
        for v in &[
            "eat", "consume", "devour", "swallow", "gulp", "sip", "nibble", "feast", "dine",
            "gorge", "inhale",
        ] {
            assert_eq!(verb_class(v), Some(Consumption), "Consumption: {v}");
        }
    }

    // ── role_frame tests ──────────────────────────────────────────────────────

    #[test]
    fn role_frame_action_active() {
        assert_eq!(role_frame(Action, "nsubj", false), Agent);
    }

    #[test]
    fn role_frame_action_dobj() {
        assert_eq!(role_frame(Action, "dobj", false), Patient);
    }

    #[test]
    fn role_frame_action_passive_subj() {
        assert_eq!(role_frame(Action, "nsubj", true), Patient);
    }

    #[test]
    fn role_frame_perception_nsubj() {
        assert_eq!(role_frame(Perception, "nsubj", false), Experiencer);
    }

    #[test]
    fn role_frame_motion_nsubj() {
        assert_eq!(role_frame(Motion, "nsubj", false), Theme);
    }

    #[test]
    fn role_frame_transfer_iobj() {
        assert_eq!(role_frame(Transfer, "iobj", false), Recipient);
    }

    #[test]
    fn role_frame_creation_dobj() {
        // dobj of a creation verb is Theme, NOT Patient
        assert_eq!(role_frame(Creation, "dobj", false), Theme);
        assert_ne!(role_frame(Creation, "dobj", false), Patient);
    }

    #[test]
    fn role_frame_change_of_state_passive() {
        assert_eq!(role_frame(ChangeOfState, "nsubj", true), Patient);
    }

    #[test]
    fn role_frame_stative_nsubj() {
        assert_eq!(role_frame(Stative, "nsubj", false), Experiencer);
    }

    #[test]
    fn role_frame_unknown_dep() {
        assert_eq!(role_frame(Action, "advmod", false), Unknown);
    }

    // ── ergative detection tests ──────────────────────────────────────────────

    #[test]
    fn ergative_detection() {
        // ChangeOfState + has_agent + no patient → ergative
        assert!(is_ergative_use(ChangeOfState, true, false));
        // Action class is never ergative
        assert!(!is_ergative_use(Action, true, false));
        // ChangeOfState with both agent AND patient → not ergative (transitive use)
        assert!(!is_ergative_use(ChangeOfState, true, true));
        // ChangeOfState with no agent → not ergative
        assert!(!is_ergative_use(ChangeOfState, false, false));
    }

    // ── Display impl tests ────────────────────────────────────────────────────

    #[test]
    fn thematic_role_display() {
        assert_eq!(Agent.to_string(), "agent");
        assert_eq!(Patient.to_string(), "patient");
        assert_eq!(Experiencer.to_string(), "experiencer");
        assert_eq!(Theme.to_string(), "theme");
        assert_eq!(Instrument.to_string(), "instrument");
        assert_eq!(Beneficiary.to_string(), "beneficiary");
        assert_eq!(Location.to_string(), "location");
        assert_eq!(Goal.to_string(), "goal");
        assert_eq!(Source.to_string(), "source");
        assert_eq!(Recipient.to_string(), "recipient");
        assert_eq!(Cause.to_string(), "cause");
        assert_eq!(Unknown.to_string(), "unknown");
    }

    // ── classify_roles tests ──────────────────────────────────────────────────

    use crate::spacy::SpacyToken as SpacyTokenData;
    use crate::tree::InteractionData;

    fn make_interaction(
        verb_lemma: &str,
        agent: Option<&str>,
        patient: Option<&str>,
        instrument: Option<&str>,
        recipient: Option<&str>,
        is_passive: bool,
    ) -> InteractionData {
        InteractionData {
            verb: verb_lemma.to_string(),
            verb_lemma: verb_lemma.to_string(),
            verb_idx: 1,
            agent: agent.map(str::to_string),
            patient: patient.map(str::to_string),
            instrument: instrument.map(str::to_string),
            recipient: recipient.map(str::to_string),
            is_passive,
            source_line: 0,
            ..InteractionData::default()
        }
    }

    fn make_token(
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
            tag: String::new(),
            dep: dep.to_string(),
            head,
            idx,
            ent_type: String::new(),
            ent_iob: String::new(),
        }
    }

    #[test]
    fn classify_action_verb_kick() {
        let i = make_interaction("kick", Some("Sarah"), Some("the ball"), None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Agent);
        assert!((ann[0].confidence - 1.0).abs() < 1e-6);
        assert_eq!(ann[1].participant, "the ball");
        assert_eq!(ann[1].thematic_role, Patient);
        assert!((ann[1].confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn classify_perception_verb_hear() {
        let i = make_interaction("hear", Some("Sarah"), Some("the noise"), None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0].thematic_role, Experiencer);
        assert_eq!(ann[1].thematic_role, Theme);
    }

    #[test]
    fn classify_motion_verb_run() {
        let i = make_interaction("run", Some("Sarah"), None, None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 1);
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Theme);
        assert!((ann[0].confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn classify_transfer_verb_give() {
        let i = make_interaction(
            "give",
            Some("Sarah"),
            Some("the book"),
            None,
            Some("Tom"),
            false,
        );
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 3);
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Agent);
        assert_eq!(ann[1].participant, "the book");
        assert_eq!(ann[1].thematic_role, Theme);
        assert_eq!(ann[2].participant, "Tom");
        assert_eq!(ann[2].thematic_role, Recipient);
    }

    #[test]
    fn classify_creation_verb_bake() {
        let i = make_interaction("bake", Some("Sarah"), Some("a cake"), None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0].thematic_role, Agent);
        assert_eq!(ann[1].thematic_role, Theme);
    }

    #[test]
    fn classify_passive_voice() {
        let i = make_interaction("kick", Some("Sarah"), Some("The ball"), None, None, true);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Agent);
        assert!((ann[0].confidence - 1.0).abs() < 1e-6);
        assert_eq!(ann[1].participant, "The ball");
        assert_eq!(ann[1].thematic_role, Patient);
        assert!((ann[1].confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn classify_ergative_door_opened() {
        let i = make_interaction("open", Some("The door"), None, None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 1);
        assert_eq!(ann[0].participant, "The door");
        assert_eq!(ann[0].thematic_role, Patient);
        assert!((ann[0].confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn classify_ergative_ice_melted() {
        let i = make_interaction("melt", Some("The ice"), None, None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 1);
        assert_eq!(ann[0].thematic_role, Patient);
        assert!((ann[0].confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn classify_change_of_state_with_agent() {
        let i = make_interaction(
            "break",
            Some("Sarah"),
            Some("the window"),
            None,
            None,
            false,
        );
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Agent);
        assert!((ann[0].confidence - 1.0).abs() < 1e-6);
        assert_eq!(ann[1].participant, "the window");
        assert_eq!(ann[1].thematic_role, Patient);
    }

    #[test]
    fn classify_instrument_with_hammer() {
        let i = make_interaction(
            "break",
            Some("Sarah"),
            Some("the window"),
            Some("a hammer"),
            None,
            false,
        );
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 3);
        assert_eq!(ann[2].participant, "a hammer");
        assert_eq!(ann[2].thematic_role, Instrument);
        assert!((ann[2].confidence - 0.95).abs() < 1e-6);
    }

    #[test]
    fn classify_unknown_verb() {
        let i = make_interaction(
            "defenestrate",
            Some("Sarah"),
            Some("the villain"),
            None,
            None,
            false,
        );
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(ann.len(), 2);
        assert_eq!(ann[0].thematic_role, Unknown);
        assert!((ann[0].confidence - 0.5).abs() < 1e-6);
        assert_eq!(ann[1].thematic_role, Unknown);
        assert!((ann[1].confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn classify_copular_be() {
        let i = make_interaction("be", Some("Sarah"), Some("a detective"), None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(
            ann.len(),
            1,
            "No Patient annotation expected for copular complement"
        );
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Theme);
        assert!((ann[0].confidence - 0.8).abs() < 1e-6);
    }

    #[test]
    fn classify_light_verb_take_walk() {
        let i = make_interaction("take", Some("Sarah"), Some("a walk"), None, None, false);
        let ann = super::classify_roles(&i, &[]);
        assert_eq!(
            ann.len(),
            1,
            "No Patient annotation expected for light verb complement"
        );
        assert_eq!(ann[0].participant, "Sarah");
        assert_eq!(ann[0].thematic_role, Agent);
        assert!((ann[0].confidence - 0.6).abs() < 1e-6);
    }

    #[test]
    fn classify_prep_to_goal() {
        // "Sarah ran to the store"
        // [0]=Sarah nsubj h=1, [1]=ran ROOT h=1, [2]=to prep h=1, [3]=the det h=4, [4]=store pobj h=2
        let tokens = vec![
            make_token("Sarah", "Sarah", "PROPN", "nsubj", 1, 0),
            make_token("ran", "run", "VERB", "ROOT", 1, 1),
            make_token("to", "to", "ADP", "prep", 1, 2),
            make_token("the", "the", "DET", "det", 4, 3),
            make_token("store", "store", "NOUN", "pobj", 2, 4),
        ];
        let i = InteractionData {
            verb: "ran".to_string(),
            verb_lemma: "run".to_string(),
            verb_idx: 1,
            agent: Some("Sarah".to_string()),
            patient: None,
            instrument: None,
            recipient: None,
            is_passive: false,
            source_line: 0,
            ..InteractionData::default()
        };
        let ann = super::classify_roles(&i, &tokens);
        let goal = ann
            .iter()
            .find(|a| a.thematic_role == Goal)
            .expect("Expected a Goal annotation");
        assert_eq!(goal.participant, "the store");
        assert!((goal.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn classify_prep_from_source() {
        // "Sarah fled from the city"
        let tokens = vec![
            make_token("Sarah", "Sarah", "PROPN", "nsubj", 1, 0),
            make_token("fled", "flee", "VERB", "ROOT", 1, 1),
            make_token("from", "from", "ADP", "prep", 1, 2),
            make_token("the", "the", "DET", "det", 4, 3),
            make_token("city", "city", "NOUN", "pobj", 2, 4),
        ];
        let i = InteractionData {
            verb: "fled".to_string(),
            verb_lemma: "flee".to_string(),
            verb_idx: 1,
            agent: Some("Sarah".to_string()),
            patient: None,
            instrument: None,
            recipient: None,
            is_passive: false,
            source_line: 0,
            ..InteractionData::default()
        };
        let ann = super::classify_roles(&i, &tokens);
        let src = ann
            .iter()
            .find(|a| a.thematic_role == Source)
            .expect("Expected a Source annotation");
        assert_eq!(src.participant, "the city");
        assert!((src.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn classify_prep_at_location() {
        // "Sarah waited at the park"
        let tokens = vec![
            make_token("Sarah", "Sarah", "PROPN", "nsubj", 1, 0),
            make_token("waited", "wait", "VERB", "ROOT", 1, 1),
            make_token("at", "at", "ADP", "prep", 1, 2),
            make_token("the", "the", "DET", "det", 4, 3),
            make_token("park", "park", "NOUN", "pobj", 2, 4),
        ];
        let i = InteractionData {
            verb: "waited".to_string(),
            verb_lemma: "wait".to_string(),
            verb_idx: 1,
            agent: Some("Sarah".to_string()),
            patient: None,
            instrument: None,
            recipient: None,
            is_passive: false,
            source_line: 0,
            ..InteractionData::default()
        };
        let ann = super::classify_roles(&i, &tokens);
        let loc = ann
            .iter()
            .find(|a| a.thematic_role == Location)
            .expect("Expected a Location annotation");
        assert_eq!(loc.participant, "the park");
        assert!((loc.confidence - 0.85).abs() < 1e-6);
    }
}
