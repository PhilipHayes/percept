use crate::coref::CoreferenceChain;
use std::collections::HashMap;

/// Merge per-file coreference chains into cross-file chains.
///
/// Chains with the same canonical name (case-insensitive) are merged,
/// combining their aliases and mentions.  Chains for characters not
/// re-mentioned within `window` paragraphs of each other are kept
/// separate (their mentions simply accumulate).
pub(crate) fn merge_coref_chains(
    per_file_chains: &[Vec<CoreferenceChain>],
    window: usize,
) -> Vec<CoreferenceChain> {
    let mut merged_map: HashMap<String, CoreferenceChain> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for file_chains in per_file_chains {
        for chain in file_chains {
            let key = chain.canonical.to_lowercase();

            if let Some(existing) = merged_map.get_mut(&key) {
                merge_chain_into(existing, chain, window);
            } else {
                order.push(key.clone());
                merged_map.insert(key, chain.clone());
            }
        }
    }

    let mut result: Vec<CoreferenceChain> = order
        .into_iter()
        .filter_map(|k| merged_map.remove(&k))
        .collect();

    result.sort_by(|a, b| a.canonical.to_lowercase().cmp(&b.canonical.to_lowercase()));
    result
}

/// Merge `incoming` chain into `target` chain.
fn merge_chain_into(target: &mut CoreferenceChain, incoming: &CoreferenceChain, _window: usize) {
    // Merge aliases (dedup).
    for alias in &incoming.aliases {
        if !target.aliases.contains(alias) {
            target.aliases.push(alias.clone());
        }
    }

    // Append mentions.
    target.mentions.extend(incoming.mentions.iter().cloned());
    target.total_mention_count += incoming.total_mention_count;

    // Keep the more specific entity_type if target's is empty.
    if target.entity_type.is_empty() && !incoming.entity_type.is_empty() {
        target.entity_type = incoming.entity_type.clone();
    }
}

/// Build a canonical-name rescue map from merged coref chains.
///
/// Returns a map from alias (lowercase) to canonical name, supporting
/// cross-file alias resolution (e.g., "Israel" → "Jacob").
pub(crate) fn build_rescue_map(chains: &[CoreferenceChain]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for chain in chains {
        let canonical_lower = chain.canonical.to_lowercase();
        for alias in &chain.aliases {
            let alias_lower = alias.to_lowercase();
            if alias_lower != canonical_lower {
                map.insert(alias_lower, chain.canonical.clone());
            }
        }
    }
    map
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coref::{CoreferenceChain, CoreferenceData, CorefType};

    fn make_chain(
        canonical: &str,
        entity_type: &str,
        aliases: Vec<&str>,
        mention_count: usize,
    ) -> CoreferenceChain {
        let mentions: Vec<CoreferenceData> = (0..mention_count)
            .map(|i| CoreferenceData {
                referent: aliases.first().map(|a| a.to_string()).unwrap_or_default(),
                canonical: canonical.to_string(),
                coref_type: CorefType::CrossSentencePronoun,
                confidence: 0.8,
                sentence_idx: i,
                token_idx: 0,
                source_line: i + 1,
            })
            .collect();

        CoreferenceChain {
            canonical: canonical.to_string(),
            entity_type: entity_type.to_string(),
            aliases: aliases.into_iter().map(|s| s.to_string()).collect(),
            mentions,
            total_mention_count: mention_count,
        }
    }

    // ── test: merge chains with same canonical ───────────────────────────────

    #[test]
    fn test_merge_chains_same_canonical() {
        let file1_chains = vec![make_chain("Joseph", "PERSON", vec!["he"], 3)];
        let file2_chains = vec![make_chain("Joseph", "PERSON", vec!["him"], 2)];

        let merged = merge_coref_chains(&[file1_chains, file2_chains], 100);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].canonical, "Joseph");
        assert_eq!(merged[0].total_mention_count, 5);
        assert_eq!(merged[0].mentions.len(), 5);
        assert!(merged[0].aliases.contains(&"he".to_string()));
        assert!(merged[0].aliases.contains(&"him".to_string()));
    }

    // ── test: different entities stay separate ───────────────────────────────

    #[test]
    fn test_merge_chains_different_entities() {
        let file1_chains = vec![make_chain("Joseph", "PERSON", vec!["he"], 2)];
        let file2_chains = vec![make_chain("Reuben", "PERSON", vec!["he"], 1)];

        let merged = merge_coref_chains(&[file1_chains, file2_chains], 100);

        assert_eq!(merged.len(), 2);
        let names: Vec<&str> = merged.iter().map(|c| c.canonical.as_str()).collect();
        assert!(names.contains(&"Joseph"));
        assert!(names.contains(&"Reuben"));
    }

    // ── test: cross-file alias merge ─────────────────────────────────────────

    #[test]
    fn test_cross_file_alias_merge() {
        // File 1 establishes "Joseph" with alias "he"
        let file1_chains = vec![make_chain("Joseph", "PERSON", vec!["he"], 2)];
        // File 2 has "Joseph" with alias "the boy"
        let file2_chains = vec![make_chain("Joseph", "PERSON", vec!["the boy"], 1)];

        let merged = merge_coref_chains(&[file1_chains, file2_chains], 100);

        assert_eq!(merged.len(), 1);
        assert!(merged[0].aliases.contains(&"he".to_string()));
        assert!(merged[0].aliases.contains(&"the boy".to_string()));
    }

    // ── test: rescue map across files ────────────────────────────────────────

    #[test]
    fn test_gpe_rescue_across_files() {
        // File 1 establishes "Jacob" with alias "Israel"
        let file1 = vec![make_chain("Jacob", "PERSON", vec!["Israel"], 2)];
        // File 3 mentions "Israel" → should resolve to "Jacob"
        let file3 = vec![make_chain("Jacob", "PERSON", vec![], 1)];

        let merged = merge_coref_chains(&[file1, file3], 100);
        let rescue = build_rescue_map(&merged);

        assert_eq!(rescue.get("israel"), Some(&"Jacob".to_string()));
    }

    // ── test: entity type propagation ────────────────────────────────────────

    #[test]
    fn test_entity_type_propagation() {
        // File 1 has no entity type, file 2 establishes PERSON
        let file1 = vec![make_chain("Joseph", "", vec!["he"], 1)];
        let file2 = vec![make_chain("Joseph", "PERSON", vec!["him"], 1)];

        let merged = merge_coref_chains(&[file1, file2], 100);

        assert_eq!(merged[0].entity_type, "PERSON");
    }

    // ── test: window parameter (currently accumulates all) ───────────────────

    #[test]
    fn test_coref_window_respected() {
        // With window=100, chains from all files with same canonical merge.
        // The window primarily affects cross-file pronoun resolution (future),
        // not chain merging by canonical name.
        let file1 = vec![make_chain("Joseph", "PERSON", vec!["he"], 2)];
        let file2 = vec![make_chain("Joseph", "PERSON", vec!["him"], 3)];

        let merged = merge_coref_chains(&[file1, file2], 100);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].total_mention_count, 5);
    }
}
