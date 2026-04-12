//! Thin cache adapter wrapping [`oq::Cache`] for nq domain needs.
//!
//! Embeds pipeline version in the oq mode string so callers never pass it.
//! Stores a version marker alongside each entry for stale-pipeline detection.

use aq_core::node::OwnedNode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::corpus::CorpusMetadata;

/// Returns the current NLP pipeline version string.
pub fn pipeline_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Cache adapter for nq, backed by [`oq::Cache`].
pub struct NqCache {
    inner: oq::Cache,
    version: String,
}

/// Per-file entry in the corpus manifest.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ManifestEntry {
    pub path: String,
    pub content_hash: String,
    pub word_count: usize,
    pub indexed_at: String,
}

/// Manifest tracking indexed corpus state.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CorpusManifest {
    pub files: Vec<ManifestEntry>,
    pub pipeline_version: String,
    pub total_files: usize,
    pub total_words: usize,
    pub indexed_at: String,
}

impl CorpusManifest {
    /// Build a manifest from index results.
    pub fn from_entries(entries: Vec<ManifestEntry>, pipeline_version: &str) -> Self {
        let total_files = entries.len();
        let total_words: usize = entries.iter().map(|e| e.word_count).sum();
        let indexed_at = now_iso8601();
        CorpusManifest {
            files: entries,
            pipeline_version: pipeline_version.to_string(),
            total_files,
            total_words,
            indexed_at,
        }
    }

    /// Write manifest to the cache directory.
    pub fn write(&self, cache_dir: &Path) -> anyhow::Result<()> {
        let path = cache_dir.join("nq-corpus-manifest.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Read manifest from the cache directory.
    pub fn read(cache_dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = cache_dir.join("nq-corpus-manifest.json");
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&path)?;
        let manifest: Self = serde_json::from_str(&data)?;
        Ok(Some(manifest))
    }
}

/// Result of a cache status check.
#[derive(Debug)]
pub enum NqCacheStatus {
    /// Exact match (content hash + pipeline version).
    Hit,
    /// Content matches but pipeline version differs.
    StalePipeline(String),
    /// No entry for this content hash.
    Miss,
}

/// A cached merge result (Phase 2).
#[derive(Serialize, Deserialize)]
struct MergeCacheEntry {
    tree: OwnedNode,
    metadata: CorpusMetadata,
}

impl NqCache {
    /// Open the default cache (`~/.cache/oq/`).
    pub fn open() -> anyhow::Result<Self> {
        Ok(Self {
            inner: oq::Cache::open()?,
            version: pipeline_version().to_string(),
        })
    }

    /// Open a cache at a specific directory (for testing).
    pub fn open_at(dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            inner: oq::Cache::open_at(dir)?,
            version: pipeline_version().to_string(),
        })
    }

    /// Open a cache with an explicit version (for testing stale-pipeline).
    pub fn open_at_with_version(dir: PathBuf, version: &str) -> anyhow::Result<Self> {
        Ok(Self {
            inner: oq::Cache::open_at(dir)?,
            version: version.to_string(),
        })
    }

    /// SHA-256 of a content string, returned as 64-char lowercase hex.
    pub fn content_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Current pipeline version.
    pub fn pipeline_version(&self) -> &str {
        &self.version
    }

    /// The oq mode string with pipeline version baked in.
    fn mode(&self) -> String {
        format!("nq:{}", self.version)
    }

    /// Mode string for the version marker (version-agnostic key).
    fn marker_mode() -> String {
        "nq:__v__".to_string()
    }

    /// Look up a cached OwnedNode tree. Returns `None` on miss.
    pub fn get(&self, file: &Path, content_hash: &str) -> anyhow::Result<Option<OwnedNode>> {
        let mode = self.mode();
        match self.inner.get_by_hash(file, content_hash, &mode)? {
            Some(value) => {
                let node: OwnedNode = serde_json::from_value(value)?;
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    /// Write an OwnedNode tree to cache.
    pub fn put(&self, file: &Path, content_hash: &str, tree: &OwnedNode) -> anyhow::Result<()> {
        let mode = self.mode();
        let value = serde_json::to_value(tree)?;
        self.inner.put_by_hash(file, content_hash, &mode, value)?;
        // Store version marker for stale-pipeline detection.
        let marker = Self::marker_mode();
        let ver = serde_json::Value::String(self.version.clone());
        self.inner.put_by_hash(file, content_hash, &marker, ver)?;
        Ok(())
    }

    /// Check cache status without returning the tree.
    pub fn get_status(&self, file: &Path, content_hash: &str) -> anyhow::Result<NqCacheStatus> {
        let mode = self.mode();
        if self.inner.get_by_hash(file, content_hash, &mode)?.is_some() {
            return Ok(NqCacheStatus::Hit);
        }
        // Check version marker — same content hash, different pipeline version.
        let marker = Self::marker_mode();
        if let Some(v) = self.inner.get_by_hash(file, content_hash, &marker)? {
            if let Some(ver) = v.as_str() {
                return Ok(NqCacheStatus::StalePipeline(ver.to_string()));
            }
        }
        Ok(NqCacheStatus::Miss)
    }

    // ── Merge cache (Phase 2) ───────────────────────────────────────────

    /// Compute a deterministic merge key from file hashes + pipeline version.
    fn merge_key(&self, file_hashes: &[(&str, &str)]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.version.as_bytes());
        hasher.update(b"\n");
        let mut sorted: Vec<(&str, &str)> = file_hashes.to_vec();
        sorted.sort_by_key(|(path, _)| *path);
        for (path, hash) in &sorted {
            hasher.update(format!("{path}:{hash}\n").as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// Retrieve a cached Phase 2 merge result.
    pub fn get_merged(
        &self,
        file_hashes: &[(&str, &str)],
    ) -> anyhow::Result<Option<(OwnedNode, CorpusMetadata)>> {
        let key = self.merge_key(file_hashes);
        let merge_mode = format!("nq-merge:{}", self.version);
        let path = Path::new("__corpus_merge__");
        match self.inner.get_by_hash(path, &key, &merge_mode)? {
            Some(value) => {
                let entry: MergeCacheEntry = serde_json::from_value(value)?;
                Ok(Some((entry.tree, entry.metadata)))
            }
            None => Ok(None),
        }
    }

    /// Store a Phase 2 merge result.
    pub fn put_merged(
        &self,
        file_hashes: &[(&str, &str)],
        tree: &OwnedNode,
        metadata: &CorpusMetadata,
    ) -> anyhow::Result<()> {
        let key = self.merge_key(file_hashes);
        let merge_mode = format!("nq-merge:{}", self.version);
        let path = Path::new("__corpus_merge__");
        let entry = MergeCacheEntry {
            tree: tree.clone(),
            metadata: metadata.clone(),
        };
        let value = serde_json::to_value(&entry)?;
        self.inner.put_by_hash(path, &key, &merge_mode, value)?;
        Ok(())
    }

    // ── Helpers used by index.rs ────────────────────────────────────────

    /// Total cache size in bytes (delegates to oq stats).
    pub fn total_size_bytes(&self) -> u64 {
        self.inner.stats().map(|s| s.total_bytes).unwrap_or(0)
    }

    /// Cache directory path.
    pub fn cache_dir(&self) -> &Path {
        self.inner.dir()
    }
}

fn now_iso8601() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn temp_cache() -> (NqCache, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let cache = NqCache::open_at(tmp.path().to_path_buf()).expect("open cache");
        (cache, tmp)
    }

    fn sample_tree() -> OwnedNode {
        OwnedNode {
            node_type: "document".into(),
            text: None,
            subtree_text: Some("Joseph dreamed a dream.".into()),
            field_indices: HashMap::from([("entities".into(), vec![0])]),
            children: vec![OwnedNode::leaf("entity", "Joseph", 1)],
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 0,
            source_file: Some("genesis.txt".into()),
        }
    }

    fn sample_metadata() -> CorpusMetadata {
        CorpusMetadata {
            files: vec!["a.txt".into(), "b.txt".into()],
            file_boundaries: vec![(0, 2), (3, 5)],
        }
    }

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = NqCache::content_hash("Joseph dreamed a dream.");
        let h2 = NqCache::content_hash("Joseph dreamed a dream.");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_content_hash_different() {
        let h1 = NqCache::content_hash("Joseph dreamed a dream.");
        let h2 = NqCache::content_hash("Jacob sent Joseph to Shechem.");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_roundtrip() {
        let (cache, _tmp) = temp_cache();
        let tree = sample_tree();
        let hash = NqCache::content_hash("content");
        let path = Path::new("genesis.txt");

        cache.put(path, &hash, &tree).expect("put");
        let restored = cache.get(path, &hash).expect("get");
        assert_eq!(restored, Some(tree));
    }

    #[test]
    fn test_miss() {
        let (cache, _tmp) = temp_cache();
        let result = cache
            .get(Path::new("missing.txt"), "deadbeef00")
            .expect("get");
        assert_eq!(result, None);
    }

    #[test]
    fn test_invalidation_on_content_change() {
        let (cache, _tmp) = temp_cache();
        let tree = sample_tree();
        let path = Path::new("genesis.txt");
        let hash_a = NqCache::content_hash("version A");
        let hash_b = NqCache::content_hash("version B");

        cache.put(path, &hash_a, &tree).expect("put");
        assert_eq!(cache.get(path, &hash_b).expect("get"), None);
        assert_eq!(cache.get(path, &hash_a).expect("get"), Some(tree));
    }

    #[test]
    fn test_status_hit() {
        let (cache, _tmp) = temp_cache();
        let tree = sample_tree();
        let path = Path::new("genesis.txt");
        let hash = NqCache::content_hash("content");

        cache.put(path, &hash, &tree).expect("put");
        let status = cache.get_status(path, &hash).expect("status");
        assert!(matches!(status, NqCacheStatus::Hit));
    }

    #[test]
    fn test_status_miss() {
        let (cache, _tmp) = temp_cache();
        let status = cache
            .get_status(Path::new("missing.txt"), "deadbeef")
            .expect("status");
        assert!(matches!(status, NqCacheStatus::Miss));
    }

    #[test]
    fn test_pipeline_version_not_empty() {
        assert!(!pipeline_version().is_empty());
    }

    #[test]
    fn test_pipeline_version_stable() {
        assert_eq!(pipeline_version(), pipeline_version());
    }

    #[test]
    fn test_merge_roundtrip() {
        let (cache, _tmp) = temp_cache();
        let tree = sample_tree();
        let meta = sample_metadata();
        let pairs = [("a.txt", "aaa"), ("b.txt", "bbb")];

        cache.put_merged(&pairs, &tree, &meta).expect("put_merged");
        let result = cache.get_merged(&pairs).expect("get_merged");
        assert!(result.is_some());
        let (t, m) = result.unwrap();
        assert_eq!(t, tree);
        assert_eq!(m.files, meta.files);
    }

    #[test]
    fn test_merge_miss_different_files() {
        let (cache, _tmp) = temp_cache();
        let tree = sample_tree();
        let meta = sample_metadata();

        cache
            .put_merged(&[("a.txt", "aaa"), ("b.txt", "bbb")], &tree, &meta)
            .expect("put_merged");
        let result = cache
            .get_merged(&[("a.txt", "aaa"), ("c.txt", "ccc")])
            .expect("get_merged");
        assert!(result.is_none());
    }
}
