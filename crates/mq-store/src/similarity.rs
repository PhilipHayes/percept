use mq_embed::model::SearchResult;

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "Vector dimensions must match");
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Brute-force top-k search over a set of embeddings.
pub fn search_top_k(
    query: &[f32],
    items: &[(String, Vec<f32>, Option<serde_json::Value>)],
    k: usize,
    threshold: f32,
) -> Vec<SearchResult> {
    let mut scored: Vec<SearchResult> = items
        .iter()
        .map(|(key, embedding, metadata)| SearchResult {
            key: key.clone(),
            score: cosine_similarity(query, embedding),
            metadata: metadata.clone(),
        })
        .filter(|r| r.score >= threshold)
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_have_similarity_one() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_have_similarity_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn opposite_vectors_have_negative_similarity() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn search_top_k_respects_threshold() {
        let query = vec![1.0, 0.0, 0.0];
        let items = vec![
            ("a".to_string(), vec![1.0, 0.0, 0.0], None), // sim = 1.0
            ("b".to_string(), vec![0.0, 1.0, 0.0], None), // sim = 0.0
            ("c".to_string(), vec![0.7, 0.7, 0.0], None), // sim ~= 0.707
        ];
        let results = search_top_k(&query, &items, 10, 0.5);
        assert_eq!(results.len(), 2); // a and c
        assert_eq!(results[0].key, "a");
        assert_eq!(results[1].key, "c");
    }

    #[test]
    fn search_top_k_respects_k() {
        let query = vec![1.0, 0.0, 0.0];
        let items = vec![
            ("a".to_string(), vec![1.0, 0.0, 0.0], None),
            ("b".to_string(), vec![0.9, 0.1, 0.0], None),
            ("c".to_string(), vec![0.8, 0.2, 0.0], None),
        ];
        let results = search_top_k(&query, &items, 1, 0.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "a");
    }
}
