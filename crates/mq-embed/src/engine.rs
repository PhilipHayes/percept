use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use crate::model::ModelKind;

/// Wrapper around fastembed for local embedding inference.
pub struct EmbedEngine {
    model: TextEmbedding,
    kind: ModelKind,
}

impl EmbedEngine {
    /// Create a new embedding engine with the specified model.
    pub fn new(kind: ModelKind) -> Result<Self> {
        let fastembed_model = match kind {
            ModelKind::BgeSmall => EmbeddingModel::BGESmallENV15,
            ModelKind::NomicCode => EmbeddingModel::NomicEmbedTextV15,
        };

        let model = TextEmbedding::try_new(
            InitOptions::new(fastembed_model).with_show_download_progress(true),
        )
        .context("Failed to initialize embedding model")?;

        Ok(Self { model, kind })
    }

    /// Embed a single text string.
    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>> {
        let results = self
            .model
            .embed(vec![text], None)
            .context("Embedding inference failed")?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }

    /// Embed a batch of text strings.
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.model
            .embed(texts.to_vec(), None)
            .context("Batch embedding inference failed")
    }

    /// The model kind this engine uses.
    pub fn model_kind(&self) -> ModelKind {
        self.kind
    }

    /// Embedding dimensionality.
    pub fn dims(&self) -> usize {
        self.kind.dims()
    }
}
