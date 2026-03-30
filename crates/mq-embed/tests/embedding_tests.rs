// Embedding tests require sequential execution — fastembed uses file locks
// for model download/cache that conflict under parallel test runners.
// Use `cargo test -p mq-embed -- --test-threads=1` if running in isolation.

#[cfg(test)]
mod tests {
    use mq_embed::engine::EmbedEngine;
    use mq_embed::model::ModelKind;
    use std::sync::Mutex;

    // Serialize access to the model to avoid lock contention
    static ENGINE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn bge_small_produces_correct_dimensions() {
        let _guard = ENGINE_LOCK.lock().unwrap();
        let mut engine = EmbedEngine::new(ModelKind::BgeSmall).unwrap();
        let embedding = engine.embed_one("hello world").unwrap();
        assert_eq!(embedding.len(), 384);
    }

    #[test]
    fn embeddings_are_deterministic() {
        let _guard = ENGINE_LOCK.lock().unwrap();
        let mut engine = EmbedEngine::new(ModelKind::BgeSmall).unwrap();
        let a = engine.embed_one("test determinism").unwrap();
        let b = engine.embed_one("test determinism").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn similar_texts_have_high_cosine_similarity() {
        let _guard = ENGINE_LOCK.lock().unwrap();
        let mut engine = EmbedEngine::new(ModelKind::BgeSmall).unwrap();
        let a = engine.embed_one("the cat sat on the mat").unwrap();
        let b = engine.embed_one("a cat is sitting on a mat").unwrap();
        let c = engine
            .embed_one("quantum chromodynamics in lattice gauge theory")
            .unwrap();

        let sim_ab = cosine(&a, &b);
        let sim_ac = cosine(&a, &c);

        // Similar sentences should score higher than unrelated
        assert!(
            sim_ab > sim_ac,
            "sim(cat-cat)={} should be > sim(cat-physics)={}",
            sim_ab,
            sim_ac
        );
        assert!(
            sim_ab > 0.7,
            "similar sentences should have high similarity: {}",
            sim_ab
        );
    }

    #[test]
    fn batch_embed_matches_individual() {
        let _guard = ENGINE_LOCK.lock().unwrap();
        let mut engine = EmbedEngine::new(ModelKind::BgeSmall).unwrap();
        let texts = vec!["hello", "world"];
        let batch = engine.embed_batch(&texts).unwrap();
        let individual_0 = engine.embed_one("hello").unwrap();
        let individual_1 = engine.embed_one("world").unwrap();

        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0], individual_0);
        assert_eq!(batch[1], individual_1);
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm_a * norm_b)
    }
}
