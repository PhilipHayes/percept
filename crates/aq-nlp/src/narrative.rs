use crate::discourse::DiscourseRelationData;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoundarySignal {
    LocationChange,
    TemporalMarker,
    EntitySetShift,
    DiscourseBreak,
    ParagraphBreak,
}

impl fmt::Display for BoundarySignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoundarySignal::LocationChange => write!(f, "location_change"),
            BoundarySignal::TemporalMarker => write!(f, "temporal_marker"),
            BoundarySignal::EntitySetShift => write!(f, "entity_set_shift"),
            BoundarySignal::DiscourseBreak => write!(f, "discourse_break"),
            BoundarySignal::ParagraphBreak => write!(f, "paragraph_break"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SceneBoundary {
    pub scene_index: usize,
    pub start_para_idx: usize,
    pub end_para_idx: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub location: Option<String>,
    pub temporal_marker: Option<String>,
    pub entity_names: Vec<String>,
    pub boundary_signals: Vec<BoundarySignal>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ParagraphEntityData {
    pub para_idx: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub entity_names: Vec<String>,
    pub location_entities: Vec<String>,
    pub temporal_entities: Vec<String>,
}

pub(crate) fn entity_set_jaccard(a: &[String], b: &[String]) -> f64 {
    use std::collections::HashSet;
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let set_a: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

pub(crate) fn detect_scene_boundaries(
    paragraphs: &[ParagraphEntityData],
    discourse_relations: &[DiscourseRelationData],
) -> Vec<SceneBoundary> {
    if paragraphs.is_empty() {
        return Vec::new();
    }
    if paragraphs.len() == 1 {
        let p = &paragraphs[0];
        return vec![SceneBoundary {
            scene_index: 0,
            start_para_idx: 0,
            end_para_idx: 0,
            start_line: p.start_line,
            end_line: p.end_line,
            location: dominant_location(&p.location_entities),
            temporal_marker: p.temporal_entities.first().cloned(),
            entity_names: p.entity_names.clone(),
            boundary_signals: vec![],
        }];
    }

    let all_empty = paragraphs.iter().all(|p| p.entity_names.is_empty());
    if all_empty {
        return paragraphs
            .iter()
            .enumerate()
            .map(|(i, p)| SceneBoundary {
                scene_index: i,
                start_para_idx: i,
                end_para_idx: i,
                start_line: p.start_line,
                end_line: p.end_line,
                location: None,
                temporal_marker: None,
                entity_names: vec![],
                boundary_signals: if i == 0 {
                    vec![]
                } else {
                    vec![BoundarySignal::ParagraphBreak]
                },
            })
            .collect();
    }

    // Pass 1: Find boundary points and their signals
    let mut boundary_signals_at: Vec<(usize, Vec<BoundarySignal>)> = Vec::new();

    let mut scene_entities: Vec<String> = paragraphs[0].entity_names.clone();
    let mut scene_locations: Vec<String> = paragraphs[0].location_entities.clone();

    for i in 0..paragraphs.len() - 1 {
        let next = &paragraphs[i + 1];
        let mut signals = Vec::new();

        // Location change
        if !next.location_entities.is_empty()
            && next
                .location_entities
                .iter()
                .any(|l| !scene_locations.contains(l))
        {
            signals.push(BoundarySignal::LocationChange);
        }

        // Temporal marker
        if !next.temporal_entities.is_empty() {
            signals.push(BoundarySignal::TemporalMarker);
        }

        // Entity set shift (Fix 3: lowered threshold from 0.25 to 0.15)
        if (!scene_entities.is_empty() || !next.entity_names.is_empty())
            && entity_set_jaccard(&scene_entities, &next.entity_names) < 0.15
        {
            signals.push(BoundarySignal::EntitySetShift);
        }

        // Discourse break
        for rel in discourse_relations {
            if rel.nucleus_para_idx == i && rel.satellite_para_idx == i + 1 {
                let t = rel.relation.to_string();
                if t == "background" || t == "sequence" {
                    signals.push(BoundarySignal::DiscourseBreak);
                    break;
                }
            }
        }

        // Fix 3: Require 2+ signals to fire, EXCEPT an explicit location change
        // (new location not seen in current scene) fires alone as a strong signal.
        let location_changed = signals.contains(&BoundarySignal::LocationChange);
        let should_fire = location_changed || signals.len() >= 2;

        if should_fire {
            boundary_signals_at.push((i + 1, signals));
            scene_entities = next.entity_names.clone();
            scene_locations = next.location_entities.clone();
        } else {
            for name in &next.entity_names {
                if !scene_entities.contains(name) {
                    scene_entities.push(name.clone());
                }
            }
            for loc in &next.location_entities {
                if !scene_locations.contains(loc) {
                    scene_locations.push(loc.clone());
                }
            }
        }
    }

    // Pass 2: Build scenes from boundaries
    let mut scene_starts: Vec<usize> = vec![0];
    let mut scene_signal_map: std::collections::HashMap<usize, Vec<BoundarySignal>> =
        std::collections::HashMap::new();
    for (para_idx, sigs) in boundary_signals_at {
        scene_starts.push(para_idx);
        scene_signal_map.insert(para_idx, sigs);
    }

    let mut scenes = Vec::new();
    for (s_idx, &start) in scene_starts.iter().enumerate() {
        let end = if s_idx + 1 < scene_starts.len() {
            scene_starts[s_idx + 1] - 1
        } else {
            paragraphs.len() - 1
        };
        let scene_paras: Vec<&ParagraphEntityData> = paragraphs[start..=end].iter().collect();
        let all_entities = collect_scene_entities(&scene_paras);
        let all_locations = collect_scene_locations(&scene_paras);
        scenes.push(SceneBoundary {
            scene_index: s_idx,
            start_para_idx: start,
            end_para_idx: end,
            start_line: paragraphs[start].start_line,
            end_line: paragraphs[end].end_line,
            location: dominant_location(&all_locations),
            temporal_marker: paragraphs[start].temporal_entities.first().cloned(),
            entity_names: all_entities,
            boundary_signals: scene_signal_map.remove(&start).unwrap_or_default(),
        });
    }

    scenes
}

fn collect_scene_entities(paras: &[&ParagraphEntityData]) -> Vec<String> {
    let mut entities = Vec::new();
    for p in paras {
        for name in &p.entity_names {
            if !entities.contains(name) {
                entities.push(name.clone());
            }
        }
    }
    entities
}

fn collect_scene_locations(paras: &[&ParagraphEntityData]) -> Vec<String> {
    let mut locs = Vec::new();
    for p in paras {
        for loc in &p.location_entities {
            if !locs.contains(loc) {
                locs.push(loc.clone());
            }
        }
    }
    locs
}

fn dominant_location(locations: &[String]) -> Option<String> {
    if locations.is_empty() {
        return None;
    }
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for loc in locations {
        *counts.entry(loc.as_str()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(name, _)| name.to_string())
}

// ============================================================
// Part A: Character Arc Computation
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArcShape {
    Rising,
    Falling,
    Flat,
    Transformative,
    Peak,
}

impl fmt::Display for ArcShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArcShape::Rising => write!(f, "rising"),
            ArcShape::Falling => write!(f, "falling"),
            ArcShape::Flat => write!(f, "flat"),
            ArcShape::Transformative => write!(f, "transformative"),
            ArcShape::Peak => write!(f, "peak"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CharacterArc {
    pub entity_name: String,
    pub arc_shape: ArcShape,
    pub total_mentions: usize,
    pub total_interactions: usize,
    pub mention_positions: Vec<f64>,
    pub role_distribution: std::collections::HashMap<String, usize>,
    pub first_mention_position: f64,
    pub last_mention_position: f64,
    pub peak_position: f64,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct EntityInteractionProfile {
    pub entity_name: String,
    pub mention_positions: Vec<f64>,
    pub interaction_positions: Vec<f64>,
    pub role_counts: std::collections::HashMap<String, usize>,
    pub interaction_roles: Vec<(f64, String)>,
}

fn segment_count(positions: &[f64], start: f64, end: f64) -> usize {
    positions.iter().filter(|&&p| p >= start && p < end).count()
}

fn dominant_role_in_range(interactions: &[(f64, String)], start: f64, end: f64) -> Option<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (pos, role) in interactions {
        if *pos >= start && *pos < end {
            *counts.entry(role.as_str()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(r, _)| r.to_string())
}

pub(crate) fn compute_character_arcs(profiles: &[EntityInteractionProfile]) -> Vec<CharacterArc> {
    profiles
        .iter()
        .filter_map(|p| {
            let total_interactions = p.interaction_positions.len();
            let total_mentions = p.mention_positions.len();

            if total_mentions == 0 && total_interactions == 0 {
                return None;
            }

            let first_mention = p.mention_positions.first().copied().unwrap_or(0.0);
            let last_mention = p.mention_positions.last().copied().unwrap_or(1.0);

            // Compute peak as median of interaction positions
            let peak_position = if total_interactions > 0 {
                let mut sorted = p.interaction_positions.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                sorted[sorted.len() / 2]
            } else {
                0.5
            };

            if total_interactions < 2 {
                return Some(CharacterArc {
                    entity_name: p.entity_name.clone(),
                    arc_shape: ArcShape::Flat,
                    total_mentions,
                    total_interactions,
                    mention_positions: p.mention_positions.clone(),
                    role_distribution: p.role_counts.clone(),
                    first_mention_position: first_mention,
                    last_mention_position: last_mention,
                    peak_position,
                    confidence: 0.40,
                });
            }

            let begin = segment_count(&p.interaction_positions, 0.0, 0.33);
            let middle = segment_count(&p.interaction_positions, 0.33, 0.67);
            let end_count = segment_count(&p.interaction_positions, 0.67, 1.01);

            // Check Transformative first (role shift)
            let first_half_role = dominant_role_in_range(&p.interaction_roles, 0.0, 0.5);
            let second_half_role = dominant_role_in_range(&p.interaction_roles, 0.5, 1.01);

            let (arc_shape, confidence) = if first_half_role.is_some()
                && second_half_role.is_some()
                && first_half_role != second_half_role
            {
                (ArcShape::Transformative, 0.75)
            } else if end_count as f64 > begin as f64 * 1.5 && end_count > middle {
                let ratio = if begin > 0 {
                    end_count as f32 / begin as f32
                } else {
                    3.0
                };
                (
                    ArcShape::Rising,
                    (0.6 + (ratio - 1.5).min(2.0) * 0.1).min(0.95),
                )
            } else if begin as f64 > end_count as f64 * 1.5 && begin > middle {
                let ratio = if end_count > 0 {
                    begin as f32 / end_count as f32
                } else {
                    3.0
                };
                (
                    ArcShape::Falling,
                    (0.6 + (ratio - 1.5).min(2.0) * 0.1).min(0.95),
                )
            } else if middle as f64 > begin as f64 * 1.5 && middle as f64 > end_count as f64 * 1.5 {
                (ArcShape::Peak, 0.80)
            } else {
                (ArcShape::Flat, 0.60)
            };

            Some(CharacterArc {
                entity_name: p.entity_name.clone(),
                arc_shape,
                total_mentions,
                total_interactions,
                mention_positions: p.mention_positions.clone(),
                role_distribution: p.role_counts.clone(),
                first_mention_position: first_mention,
                last_mention_position: last_mention,
                peak_position,
                confidence,
            })
        })
        .collect()
}

// ============================================================
// Part B: Conflict Graph Extraction
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictTrend {
    Escalating,
    Resolving,
    Stable,
    Brief,
}

impl fmt::Display for ConflictTrend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConflictTrend::Escalating => write!(f, "escalating"),
            ConflictTrend::Resolving => write!(f, "resolving"),
            ConflictTrend::Stable => write!(f, "stable"),
            ConflictTrend::Brief => write!(f, "brief"),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ConflictEdge {
    pub entity_a: String,
    pub entity_b: String,
    pub interaction_count: usize,
    pub positions: Vec<f64>,
    pub trend: ConflictTrend,
    pub first_position: f64,
    pub last_position: f64,
    pub sample_verbs: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct OpposingInteraction {
    pub agent: String,
    pub patient: String,
    pub verb: String,
    pub position: f64,
}

fn classify_conflict_trend(positions: &[f64]) -> ConflictTrend {
    if positions.len() < 3 {
        return ConflictTrend::Brief;
    }
    let begin = segment_count(positions, 0.0, 0.33);
    let end_count = segment_count(positions, 0.67, 1.01);

    // Resolving: no interactions in final 20%
    let in_final_20 = segment_count(positions, 0.80, 1.01);
    if in_final_20 == 0 {
        return ConflictTrend::Resolving;
    }

    // Escalating: last third > first third * 1.5
    if end_count as f64 > begin as f64 * 1.5 && end_count > begin {
        return ConflictTrend::Escalating;
    }

    ConflictTrend::Stable
}

/// Check if a string is a bare pronoun (not useful for conflict dyads).
pub fn is_bare_pronoun_text(s: &str) -> bool {
    matches!(
        s.to_lowercase().as_str(),
        "i" | "me"
            | "my"
            | "mine"
            | "myself"
            | "you"
            | "your"
            | "yours"
            | "yourself"
            | "yourselves"
            | "he"
            | "him"
            | "his"
            | "himself"
            | "she"
            | "her"
            | "hers"
            | "herself"
            | "it"
            | "its"
            | "itself"
            | "we"
            | "us"
            | "our"
            | "ours"
            | "ourselves"
            | "they"
            | "them"
            | "their"
            | "theirs"
            | "themselves"
    )
}

pub(crate) fn build_conflict_graph(interactions: &[OpposingInteraction]) -> Vec<ConflictEdge> {
    use std::collections::HashMap;

    // Group by canonical pair (sorted alphabetically)
    let mut pairs: HashMap<(String, String), Vec<(f64, String)>> = HashMap::new();
    for inter in interactions {
        // Skip pronoun-only dyads — they produce noise at corpus scale
        if is_bare_pronoun_text(&inter.agent) || is_bare_pronoun_text(&inter.patient) {
            continue;
        }
        let (a, b) = if inter.agent <= inter.patient {
            (inter.agent.clone(), inter.patient.clone())
        } else {
            (inter.patient.clone(), inter.agent.clone())
        };
        pairs
            .entry((a, b))
            .or_default()
            .push((inter.position, inter.verb.clone()));
    }

    let mut edges: Vec<ConflictEdge> = pairs
        .into_iter()
        .map(|((a, b), mut entries)| {
            entries.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            let positions: Vec<f64> = entries.iter().map(|(p, _)| *p).collect();
            let trend = classify_conflict_trend(&positions);

            // Collect up to 5 unique verbs
            let mut sample_verbs = Vec::new();
            for (_, verb) in &entries {
                if !sample_verbs.contains(verb) {
                    sample_verbs.push(verb.clone());
                    if sample_verbs.len() >= 5 {
                        break;
                    }
                }
            }

            ConflictEdge {
                entity_a: a,
                entity_b: b,
                interaction_count: entries.len(),
                first_position: positions.first().copied().unwrap_or(0.0),
                last_position: positions.last().copied().unwrap_or(1.0),
                positions,
                trend,
                sample_verbs,
            }
        })
        .collect();

    // Sort by interaction_count descending
    edges.sort_by(|a, b| b.interaction_count.cmp(&a.interaction_count));
    edges
}

// ============================================================
// Part C: Narrative Issues (Setup/Payoff, Foreshadowing, Consistency)
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NarrativeIssueType {
    SetupWithPayoff,
    SetupWithoutPayoff,
    Foreshadowing,
    ConsistencyViolation,
}

impl fmt::Display for NarrativeIssueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NarrativeIssueType::SetupWithPayoff => write!(f, "setup_with_payoff"),
            NarrativeIssueType::SetupWithoutPayoff => write!(f, "setup_without_payoff"),
            NarrativeIssueType::Foreshadowing => write!(f, "foreshadowing"),
            NarrativeIssueType::ConsistencyViolation => write!(f, "consistency_violation"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NarrativeIssue {
    pub issue_type: NarrativeIssueType,
    pub entity_name: String,
    pub description: String,
    pub confidence: f32,
    pub attribute: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

pub(crate) fn detect_setup_payoff(
    arcs: &[CharacterArc],
    conflict_edges: &[ConflictEdge],
) -> Vec<NarrativeIssue> {
    let mut issues = Vec::new();

    for arc in arcs {
        if arc.first_mention_position >= 0.30 {
            continue; // Only entities introduced early
        }

        // Check if entity participates in conflict with activity in last 30%
        let in_climactic_conflict = conflict_edges.iter().any(|edge| {
            (edge.entity_a == arc.entity_name || edge.entity_b == arc.entity_name)
                && edge.last_position >= 0.70
        });

        // Check if entity has a Rising or Peak arc
        let has_rising_peak = matches!(arc.arc_shape, ArcShape::Rising | ArcShape::Peak);

        if in_climactic_conflict || has_rising_peak {
            let arc_suffix = if has_rising_peak {
                format!(" with {} arc", arc.arc_shape)
            } else {
                String::new()
            };
            issues.push(NarrativeIssue {
                issue_type: NarrativeIssueType::SetupWithPayoff,
                entity_name: arc.entity_name.clone(),
                description: format!(
                    "{} introduced early (pos {:.2}) and pays off{}{}",
                    arc.entity_name,
                    arc.first_mention_position,
                    if in_climactic_conflict {
                        " in climactic conflict"
                    } else {
                        ""
                    },
                    &arc_suffix,
                ),
                confidence: if in_climactic_conflict && has_rising_peak {
                    0.90
                } else {
                    0.75
                },
                attribute: None,
                expected: None,
                actual: None,
            });
        } else if arc.total_interactions > 0 {
            // Entity introduced early but no payoff in last 50%
            let late_interactions = arc.mention_positions.iter().filter(|&&p| p >= 0.50).count();
            if late_interactions < 2 {
                issues.push(NarrativeIssue {
                    issue_type: NarrativeIssueType::SetupWithoutPayoff,
                    entity_name: arc.entity_name.clone(),
                    description: format!(
                        "{} introduced early (pos {:.2}) with {} interactions but only {} in latter half",
                        arc.entity_name,
                        arc.first_mention_position,
                        arc.total_interactions,
                        late_interactions,
                    ),
                    confidence: 0.65,
                    attribute: None,
                    expected: None,
                    actual: None,
                });
            }
        }
    }

    issues
}

pub(crate) fn detect_foreshadowing(arcs: &[CharacterArc]) -> Vec<NarrativeIssue> {
    let mut issues: Vec<NarrativeIssue> = arcs
        .iter()
        .filter_map(|arc| {
            let first_half = arc.mention_positions.iter().filter(|&&p| p < 0.50).count();
            let last_half = arc.mention_positions.iter().filter(|&&p| p >= 0.50).count();

            if first_half <= 2 && last_half >= 4 {
                let ratio = if first_half > 0 {
                    last_half as f32 / first_half as f32
                } else {
                    last_half as f32
                };
                let confidence = (0.5 + (ratio - 2.0).min(3.0) * 0.1).min(0.95);
                Some(NarrativeIssue {
                    issue_type: NarrativeIssueType::Foreshadowing,
                    entity_name: arc.entity_name.clone(),
                    description: format!(
                        "{} foreshadowed: {} mentions in first half, {} in second half",
                        arc.entity_name, first_half, last_half,
                    ),
                    confidence,
                    attribute: None,
                    expected: None,
                    actual: None,
                })
            } else {
                None
            }
        })
        .collect();

    issues.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    issues
}

pub(crate) fn detect_consistency_issues(
    _scenes: &[SceneBoundary],
    entity_scene_locations: &HashMap<String, Vec<(usize, String)>>,
    movement_interactions: &HashSet<(String, usize, usize)>,
) -> Vec<NarrativeIssue> {
    let mut issues = Vec::new();

    for (entity_name, scene_locs) in entity_scene_locations {
        if scene_locs.len() < 2 {
            continue;
        }

        for pair in scene_locs.windows(2) {
            let (scene_a, loc_a) = &pair[0];
            let (scene_b, loc_b) = &pair[1];

            // Skip if location is empty/unknown
            if loc_a.is_empty() || loc_b.is_empty() {
                continue;
            }

            if loc_a != loc_b {
                let has_movement =
                    movement_interactions.contains(&(entity_name.clone(), *scene_a, *scene_b));
                if !has_movement {
                    issues.push(NarrativeIssue {
                        issue_type: NarrativeIssueType::ConsistencyViolation,
                        entity_name: entity_name.clone(),
                        description: format!(
                            "{} in {} (scene {}) \u{2192} {} (scene {}) without movement",
                            entity_name, loc_a, scene_a, loc_b, scene_b,
                        ),
                        confidence: 0.70,
                        attribute: Some("location".to_string()),
                        expected: Some(loc_a.clone()),
                        actual: Some(loc_b.clone()),
                    });
                }
            }
        }
    }

    issues
}

// ============================================================
// Part D: Narrative Summary (aggregation)
// ============================================================

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct NarrativeSummary {
    pub scene_count: usize,
    pub character_count: usize,
    pub central_conflict: Option<(String, String)>,
    pub arc_shape_distribution: HashMap<String, usize>,
    pub conflict_count: usize,
    pub issue_count: usize,
    pub unresolved_conflicts: usize,
    pub setup_payoff_count: usize,
    pub setup_without_payoff_count: usize,
    pub foreshadowing_count: usize,
    pub consistency_violation_count: usize,
}

pub(crate) fn build_narrative_summary(
    scenes: &[SceneBoundary],
    arcs: &[CharacterArc],
    conflicts: &[ConflictEdge],
    issues: &[NarrativeIssue],
) -> NarrativeSummary {
    let mut arc_shape_dist: HashMap<String, usize> = HashMap::new();
    for arc in arcs {
        *arc_shape_dist.entry(arc.arc_shape.to_string()).or_default() += 1;
    }

    let central_conflict = conflicts
        .iter()
        .find(|c| !is_bare_pronoun_text(&c.entity_a) && !is_bare_pronoun_text(&c.entity_b))
        .or_else(|| conflicts.first())
        .map(|c| (c.entity_a.clone(), c.entity_b.clone()));

    let unresolved = conflicts
        .iter()
        .filter(|c| matches!(c.trend, ConflictTrend::Escalating))
        .count();

    let setup_payoff = issues
        .iter()
        .filter(|i| i.issue_type == NarrativeIssueType::SetupWithPayoff)
        .count();
    let setup_without = issues
        .iter()
        .filter(|i| i.issue_type == NarrativeIssueType::SetupWithoutPayoff)
        .count();
    let foreshadowing = issues
        .iter()
        .filter(|i| i.issue_type == NarrativeIssueType::Foreshadowing)
        .count();
    let consistency = issues
        .iter()
        .filter(|i| i.issue_type == NarrativeIssueType::ConsistencyViolation)
        .count();

    NarrativeSummary {
        scene_count: scenes.len(),
        character_count: arcs.len(),
        central_conflict,
        arc_shape_distribution: arc_shape_dist,
        conflict_count: conflicts.len(),
        issue_count: issues.len(),
        unresolved_conflicts: unresolved,
        setup_payoff_count: setup_payoff,
        setup_without_payoff_count: setup_without,
        foreshadowing_count: foreshadowing,
        consistency_violation_count: consistency,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discourse::{DiscourseRelation, DiscourseRelationData};

    fn make_para(
        idx: usize,
        entities: &[&str],
        locations: &[&str],
        temporals: &[&str],
    ) -> ParagraphEntityData {
        ParagraphEntityData {
            para_idx: idx,
            start_line: idx * 5 + 1,
            end_line: idx * 5 + 4,
            entity_names: entities.iter().map(|s| s.to_string()).collect(),
            location_entities: locations.iter().map(|s| s.to_string()).collect(),
            temporal_entities: temporals.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn make_discourse_rel(
        rel: DiscourseRelation,
        nucleus_para: usize,
        satellite_para: usize,
    ) -> DiscourseRelationData {
        DiscourseRelationData {
            relation: rel,
            connective: Some("Previously".to_string()),
            confidence: 0.90,
            nucleus_sentence_idx: 0,
            satellite_sentence_idx: 1,
            nucleus_text: "A.".to_string(),
            satellite_text: "B.".to_string(),
            nucleus_line: 1,
            satellite_line: 2,
            nucleus_para_idx: nucleus_para,
            satellite_para_idx: satellite_para,
        }
    }

    // --- Jaccard tests ---
    #[test]
    fn jaccard_disjoint() {
        let a = vec!["Sarah".to_string(), "Bob".to_string()];
        let b = vec!["Alice".to_string(), "David".to_string()];
        assert_eq!(entity_set_jaccard(&a, &b), 0.0);
    }

    #[test]
    fn jaccard_identical() {
        let a = vec!["Sarah".to_string(), "Bob".to_string()];
        let b = vec!["Sarah".to_string(), "Bob".to_string()];
        assert_eq!(entity_set_jaccard(&a, &b), 1.0);
    }

    #[test]
    fn jaccard_partial() {
        let a = vec!["Sarah".to_string(), "Bob".to_string(), "Alice".to_string()];
        let b = vec!["Sarah".to_string(), "David".to_string()];
        assert_eq!(entity_set_jaccard(&a, &b), 0.25);
    }

    #[test]
    fn jaccard_both_empty() {
        let a: Vec<String> = vec![];
        let b: Vec<String> = vec![];
        assert_eq!(entity_set_jaccard(&a, &b), 1.0);
    }

    // --- Scene detection tests ---
    #[test]
    fn scene_location_change() {
        let paras = vec![
            make_para(0, &["Sarah", "kitchen"], &["kitchen"], &[]),
            make_para(1, &["Sarah", "garden"], &["garden"], &[]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].location.as_deref(), Some("kitchen"));
        assert_eq!(scenes[1].location.as_deref(), Some("garden"));
        assert!(scenes[1]
            .boundary_signals
            .contains(&BoundarySignal::LocationChange));
    }

    #[test]
    fn scene_same_entities_no_break() {
        let paras = vec![
            make_para(0, &["Sarah", "Bob"], &[], &[]),
            make_para(1, &["Sarah", "Bob"], &[], &[]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].start_para_idx, 0);
        assert_eq!(scenes[0].end_para_idx, 1);
    }

    #[test]
    fn scene_temporal_marker() {
        // A temporal marker alone is no longer sufficient to trigger a boundary
        // (Fix 3: require 2+ signals, except explicit location change).
        let paras = vec![
            make_para(0, &["Sarah"], &[], &[]),
            make_para(1, &["Sarah"], &[], &["Monday morning"]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 1);
    }

    #[test]
    fn scene_entity_set_shift() {
        // Entity set shift alone (without a second signal) no longer triggers a boundary
        // (Fix 3: require 2+ signals, except explicit location change).
        let paras = vec![
            make_para(0, &["Sarah", "Bob", "Alice"], &[], &[]),
            make_para(1, &["David", "Eve", "Frank"], &[], &[]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 1);
    }

    #[test]
    fn scene_entity_partial_overlap_no_break() {
        let paras = vec![
            make_para(0, &["Sarah", "Bob", "Alice"], &[], &[]),
            make_para(1, &["Sarah", "Bob", "David"], &[], &[]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 1, "67% overlap should not trigger boundary");
    }

    #[test]
    fn scene_discourse_break() {
        // A discourse break alone is no longer sufficient to trigger a boundary
        // (Fix 3: require 2+ signals, except explicit location change).
        let paras = vec![
            make_para(0, &["Sarah"], &[], &[]),
            make_para(1, &["Sarah"], &[], &[]),
        ];
        let rels = vec![make_discourse_rel(DiscourseRelation::Background, 0, 1)];
        let scenes = detect_scene_boundaries(&paras, &rels);
        assert_eq!(scenes.len(), 1);
    }

    #[test]
    fn scene_single_paragraph() {
        let paras = vec![make_para(0, &["Sarah", "Bob"], &[], &[])];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 1);
        assert!(scenes[0].boundary_signals.is_empty());
    }

    #[test]
    fn scene_no_entities_fallback() {
        let paras = vec![
            make_para(0, &[], &[], &[]),
            make_para(1, &[], &[], &[]),
            make_para(2, &[], &[], &[]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 3);
        assert!(scenes[0].boundary_signals.is_empty());
        assert!(scenes[1]
            .boundary_signals
            .contains(&BoundarySignal::ParagraphBreak));
        assert!(scenes[2]
            .boundary_signals
            .contains(&BoundarySignal::ParagraphBreak));
    }

    #[test]
    fn scene_multiple_signals() {
        let paras = vec![
            make_para(0, &["Sarah"], &["office"], &[]),
            make_para(1, &["David"], &["park"], &["next day"]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 2);
        let sigs = &scenes[1].boundary_signals;
        assert!(sigs.contains(&BoundarySignal::LocationChange));
        assert!(sigs.contains(&BoundarySignal::EntitySetShift));
        assert!(sigs.contains(&BoundarySignal::TemporalMarker));
    }

    #[test]
    fn scene_empty_input() {
        let scenes = detect_scene_boundaries(&[], &[]);
        assert!(scenes.is_empty());
    }

    #[test]
    fn scene_entity_names_collected() {
        let paras = vec![
            make_para(0, &["Sarah", "Bob"], &[], &[]),
            make_para(1, &["Sarah", "Alice"], &[], &[]),
        ];
        let scenes = detect_scene_boundaries(&paras, &[]);
        assert_eq!(scenes.len(), 1); // 67% overlap = no break
        assert!(scenes[0].entity_names.contains(&"Sarah".to_string()));
        assert!(scenes[0].entity_names.contains(&"Bob".to_string()));
        assert!(scenes[0].entity_names.contains(&"Alice".to_string()));
    }

    // === Arc tests ===
    use std::collections::HashMap;

    fn make_profile(
        name: &str,
        interactions: &[f64],
        roles: &[(&str, usize)],
        int_roles: &[(f64, &str)],
    ) -> EntityInteractionProfile {
        let role_counts: HashMap<String, usize> =
            roles.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let interaction_roles: Vec<(f64, String)> =
            int_roles.iter().map(|(p, r)| (*p, r.to_string())).collect();
        EntityInteractionProfile {
            entity_name: name.to_string(),
            mention_positions: interactions.to_vec(),
            interaction_positions: interactions.to_vec(),
            role_counts,
            interaction_roles,
        }
    }

    #[test]
    fn arc_rising() {
        let p = make_profile("Sarah", &[0.1, 0.5, 0.7, 0.8, 0.85, 0.9], &[], &[]);
        let arcs = compute_character_arcs(&[p]);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].arc_shape, ArcShape::Rising);
    }

    #[test]
    fn arc_falling() {
        let p = make_profile("Bob", &[0.05, 0.1, 0.15, 0.2, 0.8], &[], &[]);
        let arcs = compute_character_arcs(&[p]);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].arc_shape, ArcShape::Falling);
    }

    #[test]
    fn arc_flat() {
        let p = make_profile("Alice", &[0.1, 0.3, 0.5, 0.7, 0.9], &[], &[]);
        let arcs = compute_character_arcs(&[p]);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].arc_shape, ArcShape::Flat);
    }

    #[test]
    fn arc_peak() {
        let p = make_profile("David", &[0.1, 0.4, 0.45, 0.5, 0.55, 0.6, 0.9], &[], &[]);
        let arcs = compute_character_arcs(&[p]);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].arc_shape, ArcShape::Peak);
    }

    #[test]
    fn arc_transformative() {
        let p = EntityInteractionProfile {
            entity_name: "Eve".to_string(),
            mention_positions: vec![0.1, 0.2, 0.3, 0.7, 0.8, 0.9],
            interaction_positions: vec![0.1, 0.2, 0.3, 0.7, 0.8, 0.9],
            role_counts: [("patient".to_string(), 3), ("agent".to_string(), 3)]
                .into_iter()
                .collect(),
            interaction_roles: vec![
                (0.1, "patient".to_string()),
                (0.2, "patient".to_string()),
                (0.3, "patient".to_string()),
                (0.7, "agent".to_string()),
                (0.8, "agent".to_string()),
                (0.9, "agent".to_string()),
            ],
        };
        let arcs = compute_character_arcs(&[p]);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].arc_shape, ArcShape::Transformative);
    }

    #[test]
    fn arc_insufficient_data() {
        let p = make_profile("Minor", &[0.5], &[], &[]);
        let arcs = compute_character_arcs(&[p]);
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].arc_shape, ArcShape::Flat);
        assert!((arcs[0].confidence - 0.40).abs() < 0.01);
    }

    #[test]
    fn arc_shape_display() {
        assert_eq!(ArcShape::Rising.to_string(), "rising");
        assert_eq!(ArcShape::Transformative.to_string(), "transformative");
        assert_eq!(ArcShape::Peak.to_string(), "peak");
    }

    #[test]
    fn segment_count_begin() {
        let positions = vec![0.1, 0.2, 0.4, 0.8];
        assert_eq!(segment_count(&positions, 0.0, 0.33), 2);
    }

    #[test]
    fn segment_count_empty_range() {
        let positions = vec![0.1, 0.2];
        assert_eq!(segment_count(&positions, 0.5, 1.0), 0);
    }

    #[test]
    fn arc_peak_position() {
        let p = make_profile("Sarah", &[0.1, 0.4, 0.45, 0.5, 0.55, 0.9], &[], &[]);
        let arcs = compute_character_arcs(&[p]);
        assert!((arcs[0].peak_position - 0.475).abs() < 0.05);
    }

    // === Conflict tests ===
    fn make_opposing(agent: &str, patient: &str, verb: &str, pos: f64) -> OpposingInteraction {
        OpposingInteraction {
            agent: agent.to_string(),
            patient: patient.to_string(),
            verb: verb.to_string(),
            position: pos,
        }
    }

    #[test]
    fn conflict_opposing_pair() {
        let ints = vec![
            make_opposing("Sarah", "Bob", "attacked", 0.3),
            make_opposing("Bob", "Sarah", "retaliated", 0.6),
        ];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].interaction_count, 2);
        assert!(edges[0].sample_verbs.contains(&"attacked".to_string()));
        assert!(edges[0].sample_verbs.contains(&"retaliated".to_string()));
    }

    #[test]
    fn conflict_escalating() {
        let ints = vec![
            make_opposing("Sarah", "David", "confronted", 0.1),
            make_opposing("David", "Sarah", "challenged", 0.7),
            make_opposing("Sarah", "David", "fought", 0.8),
            make_opposing("David", "Sarah", "attacked", 0.9),
        ];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges[0].trend, ConflictTrend::Escalating);
    }

    #[test]
    fn conflict_resolving() {
        let ints = vec![
            make_opposing("A", "B", "argued", 0.2),
            make_opposing("B", "A", "fought", 0.4),
            make_opposing("A", "B", "confronted", 0.6),
        ];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges[0].trend, ConflictTrend::Resolving);
    }

    #[test]
    fn conflict_stable() {
        let ints = vec![
            make_opposing("A", "B", "argued", 0.1),
            make_opposing("B", "A", "debated", 0.5),
            make_opposing("A", "B", "disputed", 0.9),
        ];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges[0].trend, ConflictTrend::Stable);
    }

    #[test]
    fn conflict_brief() {
        let ints = vec![make_opposing("A", "B", "pushed", 0.5)];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges[0].trend, ConflictTrend::Brief);
    }

    #[test]
    fn conflict_trend_display() {
        assert_eq!(ConflictTrend::Escalating.to_string(), "escalating");
        assert_eq!(ConflictTrend::Resolving.to_string(), "resolving");
    }

    #[test]
    fn conflict_pair_canonicalization() {
        let ints = vec![
            make_opposing("Bob", "Alice", "hit", 0.3),
            make_opposing("Alice", "Bob", "hit", 0.5),
        ];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].entity_a, "Alice"); // alphabetical
        assert_eq!(edges[0].entity_b, "Bob");
        assert_eq!(edges[0].interaction_count, 2);
    }

    #[test]
    fn conflict_multiple_pairs_sorted() {
        let ints = vec![
            make_opposing("A", "B", "hit", 0.2),
            make_opposing("C", "D", "attacked", 0.5),
            make_opposing("A", "B", "kicked", 0.8),
        ];
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].interaction_count, 2); // (A,B) first
        assert_eq!(edges[1].interaction_count, 1); // (C,D) second
    }

    #[test]
    fn conflict_no_interactions() {
        let edges = build_conflict_graph(&[]);
        assert!(edges.is_empty());
    }

    #[test]
    fn conflict_sample_verbs_capped() {
        let ints: Vec<OpposingInteraction> = (0..7)
            .map(|i| make_opposing("A", "B", &format!("v{}", i + 1), i as f64 * 0.1 + 0.1))
            .collect();
        let edges = build_conflict_graph(&ints);
        assert_eq!(edges[0].sample_verbs.len(), 5);
    }

    // ── Phase 5: Setup/Payoff tests ──

    #[test]
    fn setup_with_payoff_rising_arc() {
        let arcs = vec![CharacterArc {
            entity_name: "gun".to_string(),
            arc_shape: ArcShape::Rising,
            total_mentions: 5,
            total_interactions: 4,
            mention_positions: vec![0.05, 0.2, 0.5, 0.7, 0.9],
            role_distribution: HashMap::new(),
            first_mention_position: 0.05,
            last_mention_position: 0.9,
            peak_position: 0.7,
            confidence: 0.80,
        }];
        let edges = vec![ConflictEdge {
            entity_a: "gun".to_string(),
            entity_b: "villain".to_string(),
            interaction_count: 2,
            positions: vec![0.3, 0.9],
            trend: ConflictTrend::Escalating,
            first_position: 0.3,
            last_position: 0.9,
            sample_verbs: vec!["fired".to_string()],
        }];
        let issues = detect_setup_payoff(&arcs, &edges);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, NarrativeIssueType::SetupWithPayoff);
        assert_eq!(issues[0].entity_name, "gun");
        assert!(issues[0].confidence >= 0.85);
    }

    #[test]
    fn setup_without_payoff() {
        let arcs = vec![CharacterArc {
            entity_name: "locket".to_string(),
            arc_shape: ArcShape::Flat,
            total_mentions: 3,
            total_interactions: 1,
            mention_positions: vec![0.1, 0.15, 0.2],
            role_distribution: HashMap::new(),
            first_mention_position: 0.1,
            last_mention_position: 0.2,
            peak_position: 0.15,
            confidence: 0.40,
        }];
        let edges = vec![];
        let issues = detect_setup_payoff(&arcs, &edges);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, NarrativeIssueType::SetupWithoutPayoff);
        assert_eq!(issues[0].entity_name, "locket");
    }

    #[test]
    fn setup_payoff_late_entity_ignored() {
        let arcs = vec![CharacterArc {
            entity_name: "newcomer".to_string(),
            arc_shape: ArcShape::Rising,
            total_mentions: 5,
            total_interactions: 3,
            mention_positions: vec![0.5, 0.6, 0.7, 0.8, 0.9],
            role_distribution: HashMap::new(),
            first_mention_position: 0.5,
            last_mention_position: 0.9,
            peak_position: 0.8,
            confidence: 0.80,
        }];
        let issues = detect_setup_payoff(&arcs, &[]);
        assert!(
            issues.is_empty(),
            "Entity introduced at 0.5 should be ignored (>= 0.30)"
        );
    }

    #[test]
    fn foreshadowing_detected() {
        let arcs = vec![CharacterArc {
            entity_name: "stranger".to_string(),
            arc_shape: ArcShape::Rising,
            total_mentions: 9,
            total_interactions: 4,
            mention_positions: vec![0.1, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9],
            role_distribution: HashMap::new(),
            first_mention_position: 0.1,
            last_mention_position: 0.9,
            peak_position: 0.7,
            confidence: 0.80,
        }];
        let issues = detect_foreshadowing(&arcs);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, NarrativeIssueType::Foreshadowing);
        assert_eq!(issues[0].entity_name, "stranger");
    }

    #[test]
    fn foreshadowing_not_triggered_balanced() {
        let arcs = vec![CharacterArc {
            entity_name: "Sarah".to_string(),
            arc_shape: ArcShape::Flat,
            total_mentions: 10,
            total_interactions: 5,
            mention_positions: vec![0.1, 0.2, 0.3, 0.4, 0.45, 0.55, 0.6, 0.7, 0.8, 0.9],
            role_distribution: HashMap::new(),
            first_mention_position: 0.1,
            last_mention_position: 0.9,
            peak_position: 0.5,
            confidence: 0.60,
        }];
        let issues = detect_foreshadowing(&arcs);
        assert!(
            issues.is_empty(),
            "Balanced mentions should not trigger foreshadowing"
        );
    }

    #[test]
    fn consistency_location_violation() {
        let mut entity_scene_locations: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        entity_scene_locations.insert(
            "Sarah".to_string(),
            vec![
                (0, "kitchen".to_string()),
                (1, "garden".to_string()),
                (2, "kitchen".to_string()),
            ],
        );
        let movement: HashSet<(String, usize, usize)> = HashSet::new();
        let issues = detect_consistency_issues(&[], &entity_scene_locations, &movement);
        assert_eq!(issues.len(), 2); // kitchen→garden, garden→kitchen
        assert!(issues
            .iter()
            .all(|i| i.issue_type == NarrativeIssueType::ConsistencyViolation));
        assert!(issues.iter().all(|i| i.entity_name == "Sarah"));
    }

    #[test]
    fn consistency_location_with_movement() {
        let mut entity_scene_locations: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        entity_scene_locations.insert(
            "Sarah".to_string(),
            vec![(0, "kitchen".to_string()), (1, "garden".to_string())],
        );
        let mut movement: HashSet<(String, usize, usize)> = HashSet::new();
        movement.insert(("Sarah".to_string(), 0, 1));
        let issues = detect_consistency_issues(&[], &entity_scene_locations, &movement);
        assert!(
            issues.is_empty(),
            "Movement interaction accounts for location change"
        );
    }

    #[test]
    fn consistency_no_location_data() {
        let mut entity_scene_locations: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        entity_scene_locations.insert(
            "Sarah".to_string(),
            vec![(0, "".to_string()), (1, "".to_string())],
        );
        let movement: HashSet<(String, usize, usize)> = HashSet::new();
        let issues = detect_consistency_issues(&[], &entity_scene_locations, &movement);
        assert!(
            issues.is_empty(),
            "No issue when no location data to compare"
        );
    }

    #[test]
    fn issue_type_display() {
        assert_eq!(
            NarrativeIssueType::SetupWithPayoff.to_string(),
            "setup_with_payoff"
        );
        assert_eq!(
            NarrativeIssueType::SetupWithoutPayoff.to_string(),
            "setup_without_payoff"
        );
        assert_eq!(
            NarrativeIssueType::Foreshadowing.to_string(),
            "foreshadowing"
        );
        assert_eq!(
            NarrativeIssueType::ConsistencyViolation.to_string(),
            "consistency_violation"
        );
    }

    #[test]
    fn narrative_summary_aggregation() {
        let scenes = vec![
            SceneBoundary {
                scene_index: 0,
                start_para_idx: 0,
                end_para_idx: 1,
                start_line: 1,
                end_line: 5,
                location: Some("kitchen".into()),
                temporal_marker: None,
                entity_names: vec!["Sarah".into()],
                boundary_signals: vec![],
            },
            SceneBoundary {
                scene_index: 1,
                start_para_idx: 2,
                end_para_idx: 3,
                start_line: 6,
                end_line: 10,
                location: Some("garden".into()),
                temporal_marker: None,
                entity_names: vec!["Bob".into()],
                boundary_signals: vec![BoundarySignal::LocationChange],
            },
        ];
        let arcs = vec![
            CharacterArc {
                entity_name: "Sarah".into(),
                arc_shape: ArcShape::Rising,
                total_mentions: 5,
                total_interactions: 3,
                mention_positions: vec![0.1, 0.3, 0.5, 0.7, 0.9],
                role_distribution: HashMap::new(),
                first_mention_position: 0.1,
                last_mention_position: 0.9,
                peak_position: 0.7,
                confidence: 0.80,
            },
            CharacterArc {
                entity_name: "Bob".into(),
                arc_shape: ArcShape::Flat,
                total_mentions: 3,
                total_interactions: 1,
                mention_positions: vec![0.3, 0.5, 0.7],
                role_distribution: HashMap::new(),
                first_mention_position: 0.3,
                last_mention_position: 0.7,
                peak_position: 0.5,
                confidence: 0.40,
            },
        ];
        let conflicts = vec![ConflictEdge {
            entity_a: "Bob".into(),
            entity_b: "Sarah".into(),
            interaction_count: 3,
            positions: vec![0.3, 0.5, 0.8],
            trend: ConflictTrend::Escalating,
            first_position: 0.3,
            last_position: 0.8,
            sample_verbs: vec!["argued".into()],
        }];
        let issues = vec![NarrativeIssue {
            issue_type: NarrativeIssueType::SetupWithPayoff,
            entity_name: "Sarah".into(),
            description: "test".into(),
            confidence: 0.90,
            attribute: None,
            expected: None,
            actual: None,
        }];
        let summary = build_narrative_summary(&scenes, &arcs, &conflicts, &issues);
        assert_eq!(summary.scene_count, 2);
        assert_eq!(summary.character_count, 2);
        assert_eq!(
            summary.central_conflict,
            Some(("Bob".into(), "Sarah".into()))
        );
        assert_eq!(summary.conflict_count, 1);
        assert_eq!(summary.unresolved_conflicts, 1);
        assert_eq!(summary.issue_count, 1);
        assert_eq!(summary.setup_payoff_count, 1);
        assert_eq!(*summary.arc_shape_distribution.get("rising").unwrap(), 1);
        assert_eq!(*summary.arc_shape_distribution.get("flat").unwrap(), 1);
    }
}
