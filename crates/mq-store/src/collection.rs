use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use mq_embed::model::{CollectionMeta, EmbeddedItem, ModelKind};
use serde::{Deserialize, Serialize};

/// A vector collection stored on disk.
#[derive(Debug, Serialize, Deserialize)]
pub struct Collection {
    pub meta: CollectionMeta,
    pub items: Vec<EmbeddedItem>,
}

impl Collection {
    /// Create a new empty collection.
    pub fn new(name: &str, model: ModelKind) -> Self {
        Self {
            meta: CollectionMeta {
                name: name.to_string(),
                model,
                dims: model.dims(),
                item_count: 0,
            },
            items: Vec::new(),
        }
    }

    /// Add an item to the collection.
    pub fn add(&mut self, item: EmbeddedItem) {
        self.items.push(item);
        self.meta.item_count = self.items.len();
    }

    /// Remove items by key (for invalidation).
    pub fn remove_by_keys(&mut self, keys: &[String]) {
        self.items.retain(|item| !keys.contains(&item.key));
        self.meta.item_count = self.items.len();
    }

    /// Get the default storage directory.
    pub fn storage_dir() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home).join(".mq").join("collections"))
    }

    /// Path for a specific collection.
    pub fn collection_path(name: &str) -> Result<PathBuf> {
        Ok(Self::storage_dir()?.join(format!("{}.json", name)))
    }

    /// Save collection to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::collection_path(&self.meta.name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec(self)?;
        fs::write(&path, data).context("Failed to write collection")?;
        Ok(())
    }

    /// Load collection from disk.
    pub fn load(name: &str) -> Result<Self> {
        let path = Self::collection_path(name)?;
        let data = fs::read(&path).with_context(|| format!("Collection '{}' not found", name))?;
        serde_json::from_slice(&data).context("Failed to parse collection")
    }

    /// Check if a collection exists on disk.
    pub fn exists(name: &str) -> Result<bool> {
        Ok(Self::collection_path(name)?.exists())
    }
}
