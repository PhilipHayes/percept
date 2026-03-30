/// Per-file indexing pipeline for `nq index`.
///
/// Discovers text files in directories, parses each through the NLP pipeline,
/// and caches the resulting OwnedNode trees for corpus-mode queries.
use aq_core::node::OwnedNode;
use std::path::{Path, PathBuf};

use crate::markdown;
use crate::nq_cache::{CorpusManifest, ManifestEntry, NqCache, NqCacheStatus};
use crate::spacy;
use crate::tree;

use serde::Serialize;

/// Supported file extensions for indexing.
const INDEXABLE_EXTENSIONS: &[&str] = &["txt", "md", "rst", "adoc"];

/// Result of indexing a single file.
#[derive(Debug, Clone)]
pub struct IndexResult {
    pub file: String,
    pub status: IndexStatus,
    pub words: usize,
    pub error: Option<String>,
}

/// Status of a single file indexing operation.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexStatus {
    Indexed,
    Cached,
    StalePipeline,
    Error,
}

/// Options controlling indexing behavior.
pub struct IndexOptions {
    pub cache: NqCache,
    pub dry_run: bool,
    pub force: bool,
}

/// Discover indexable files in a directory (recursive).
pub fn discover_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    discover_files_recursive(dir, &mut files);
    files.sort();
    files
}

fn discover_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_files_recursive(&path, out);
        } else if is_indexable(&path) {
            out.push(path);
        }
    }
}

fn is_indexable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| INDEXABLE_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}

/// Expand glob patterns into file paths.
pub fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for pattern in patterns {
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            let entries =
                glob::glob(pattern).map_err(|e| format!("Invalid glob '{}': {}", pattern, e))?;
            for entry in entries {
                let path = entry.map_err(|e| format!("Glob error: {}", e))?;
                if path.is_file() && is_indexable(&path) {
                    files.push(path);
                }
            }
        } else {
            let path = PathBuf::from(pattern);
            if path.is_dir() {
                files.extend(discover_files(&path));
            } else if path.is_file() && is_indexable(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Index a set of files through the NLP pipeline.
pub fn index_files(files: &[PathBuf], options: &IndexOptions) -> Vec<IndexResult> {
    let mut results = Vec::with_capacity(files.len());
    let total = files.len();

    for (i, file) in files.iter().enumerate() {
        let result = index_single_file(file, options);
        eprint!("\rIndexed {}/{} files...", i + 1, total);
        results.push(result);
    }
    if total > 0 {
        eprintln!();
    }
    results
}

/// Index a single file: read → preprocess → hash → cache check → parse → cache write.
fn index_single_file(path: &Path, options: &IndexOptions) -> IndexResult {
    let file_str = path.display().to_string();

    // Read file content
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return IndexResult {
                file: file_str,
                status: IndexStatus::Error,
                words: 0,
                error: Some(format!("read error: {}", e)),
            };
        }
    };

    let word_count = content.split_whitespace().count();
    let content_hash = NqCache::content_hash(&content);

    // Check cache unless force re-index is requested
    let mut is_stale_pipeline = false;
    if !options.force {
        match options.cache.get_status(path, &content_hash) {
            Ok(NqCacheStatus::Hit) => {
                return IndexResult {
                    file: file_str,
                    status: IndexStatus::Cached,
                    words: word_count,
                    error: None,
                };
            }
            Ok(NqCacheStatus::StalePipeline(_)) => {
                is_stale_pipeline = true;
            }
            Ok(NqCacheStatus::Miss) => {}
            Err(_) => {} // Cache error — proceed with indexing
        }
    }

    if options.dry_run {
        return IndexResult {
            file: file_str,
            status: IndexStatus::Indexed,
            words: word_count,
            error: None,
        };
    }

    // Preprocess markdown files
    let parse_text = if is_markdown(path) {
        let (normalized, _raw) = markdown::preprocess_markdown(&content);
        normalized
    } else {
        content.clone()
    };

    // Parse through spaCy → OwnedNode tree
    let tree = match parse_and_build_tree(&parse_text, path) {
        Ok(t) => t,
        Err(e) => {
            return IndexResult {
                file: file_str,
                status: IndexStatus::Error,
                words: word_count,
                error: Some(e),
            };
        }
    };

    // Write to cache
    if let Err(e) = options.cache.put(path, &content_hash, &tree) {
        return IndexResult {
            file: file_str,
            status: IndexStatus::Error,
            words: word_count,
            error: Some(format!("cache write error: {}", e)),
        };
    }

    let status = if is_stale_pipeline {
        IndexStatus::StalePipeline
    } else {
        IndexStatus::Indexed
    };

    IndexResult {
        file: file_str,
        status,
        words: word_count,
        error: None,
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "md")
        .unwrap_or(false)
}

fn parse_and_build_tree(text: &str, file_path: &Path) -> Result<OwnedNode, String> {
    let doc = spacy::parse_with_spacy(text).map_err(|e| format!("spaCy error: {}", e))?;
    Ok(tree::spacy_doc_to_owned_tree(
        &doc,
        text,
        Some(&file_path.display().to_string()),
    ))
}

/// Print index results as JSON lines to stdout.
pub fn print_results(results: &[IndexResult]) {
    for r in results {
        let status_str = match r.status {
            IndexStatus::Indexed => "indexed",
            IndexStatus::Cached => "cached",
            IndexStatus::StalePipeline => "stale-pipeline",
            IndexStatus::Error => "error",
        };
        let error_json = match &r.error {
            Some(e) => format!(", \"error\": \"{}\"", e.replace('"', "\\\"")),
            None => String::new(),
        };
        println!(
            "{{\"file\": \"{}\", \"status\": \"{}\", \"words\": {}{}}}",
            r.file.replace('"', "\\\""),
            status_str,
            r.words,
            error_json
        );
    }
}

/// Summary of indexing results.
pub fn summarize(results: &[IndexResult]) -> (usize, usize, usize, usize) {
    let indexed = results
        .iter()
        .filter(|r| r.status == IndexStatus::Indexed)
        .count();
    let cached = results
        .iter()
        .filter(|r| r.status == IndexStatus::Cached)
        .count();
    let stale = results
        .iter()
        .filter(|r| r.status == IndexStatus::StalePipeline)
        .count();
    let errors = results
        .iter()
        .filter(|r| r.status == IndexStatus::Error)
        .count();
    (indexed, cached, stale, errors)
}

/// Status report for an indexed corpus directory.
#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub files: usize,
    pub indexed: usize,
    pub stale: usize,
    pub total_words: usize,
    pub index_size_mb: f64,
    pub pipeline_version: String,
    pub pipeline_current: bool,
}

/// Dry-run report estimating indexing cost.
#[derive(Debug, Serialize)]
pub struct DryRunReport {
    pub files_to_index: usize,
    pub estimated_words: usize,
    pub estimated_time_seconds: f64,
}

/// Compute index status for a directory without parsing any files.
pub fn status(files: &[PathBuf], cache: &NqCache) -> StatusReport {
    let mut indexed = 0usize;
    let mut stale = 0usize;
    let mut total_words = 0usize;

    for file in files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let words = content.split_whitespace().count();
        total_words += words;
        let hash = NqCache::content_hash(&content);

        match cache.get_status(file, &hash) {
            Ok(NqCacheStatus::Hit) => {
                indexed += 1;
            }
            Ok(NqCacheStatus::StalePipeline(_)) => {
                stale += 1;
            }
            Ok(NqCacheStatus::Miss) | Err(_) => {}
        }
    }

    let size_bytes = cache.total_size_bytes();

    StatusReport {
        files: files.len(),
        indexed,
        stale,
        total_words,
        index_size_mb: size_bytes as f64 / (1024.0 * 1024.0),
        pipeline_version: cache.pipeline_version().to_string(),
        pipeline_current: stale == 0 && indexed == files.len(),
    }
}

/// Estimate indexing cost without parsing any files.
pub fn dry_run(files: &[PathBuf], cache: &NqCache) -> DryRunReport {
    let mut files_to_index = 0usize;
    let mut estimated_words = 0usize;

    for file in files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                files_to_index += 1;
                continue;
            }
        };
        let words = content.split_whitespace().count();
        let hash = NqCache::content_hash(&content);

        match cache.get_status(file, &hash) {
            Ok(NqCacheStatus::Hit) => {} // Skip — already indexed
            _ => {
                files_to_index += 1;
                estimated_words += words;
            }
        }
    }

    // Heuristic: ~10K words/second/worker
    let estimated_time = estimated_words as f64 / 10_000.0;

    DryRunReport {
        files_to_index,
        estimated_words,
        estimated_time_seconds: estimated_time,
    }
}

/// Remove cache entries for files that no longer exist.
/// Returns the number of entries pruned.
///
/// NOTE: Simplified after oq migration. Full prune requires oq to expose
/// entry enumeration. Currently returns 0 (deferred).
pub fn prune(_files: &[PathBuf], _cache: &NqCache) -> usize {
    // TODO: Implement once oq exposes entry enumeration API.
    0
}

/// Build and write a corpus manifest from index results.
pub fn write_manifest(results: &[IndexResult], files: &[PathBuf], cache: &NqCache) {
    let entries: Vec<ManifestEntry> = results
        .iter()
        .filter(|r| r.status != IndexStatus::Error)
        .map(|r| {
            let path_buf = files.iter().find(|p| p.display().to_string() == r.file);
            let content_hash = path_buf
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|c| NqCache::content_hash(&c))
                .unwrap_or_default();
            ManifestEntry {
                path: r.file.clone(),
                content_hash,
                word_count: r.words,
                indexed_at: String::new(),
            }
        })
        .collect();

    let manifest = CorpusManifest::from_entries(entries, cache.pipeline_version());
    if let Err(e) = manifest.write(cache.cache_dir()) {
        eprintln!("warning: failed to write corpus manifest: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_test_dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("file1.txt"), "Joseph went to Egypt.").unwrap();
        fs::write(tmp.path().join("file2.txt"), "Jacob mourned for days.").unwrap();
        fs::write(tmp.path().join("skip.rs"), "fn main() {}").unwrap();
        tmp
    }

    #[test]
    fn test_discover_files_txt_only() {
        let tmp = make_test_dir();
        let files = discover_files(tmp.path());
        let names: Vec<&str> = files
            .iter()
            .filter_map(|f| f.file_name()?.to_str())
            .collect();
        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"file2.txt"));
        assert!(!names.contains(&"skip.rs"));
    }

    #[test]
    fn test_discover_files_md() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("notes.md"), "# Title\nContent.").unwrap();
        let files = discover_files(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("notes.md"));
    }

    #[test]
    fn test_discover_files_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub").join("deep");
        fs::create_dir_all(&sub).unwrap();
        fs::write(tmp.path().join("top.txt"), "top").unwrap();
        fs::write(sub.join("deep.txt"), "deep").unwrap();
        let files = discover_files(tmp.path());
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_discover_files_glob() {
        let tmp = make_test_dir();
        let pattern = format!("{}/*.txt", tmp.path().display());
        let files = expand_globs(&[pattern]).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_expand_globs_directory() {
        let tmp = make_test_dir();
        let files = expand_globs(&[tmp.path().display().to_string()]).unwrap();
        assert_eq!(files.len(), 2); // only .txt, not .rs
    }

    fn temp_cache() -> (NqCache, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cache = NqCache::open_at(tmp.path().to_path_buf()).unwrap();
        (cache, tmp)
    }

    #[test]
    fn test_status_empty_dir() {
        let (cache, _tmp) = temp_cache();
        let report = status(&[], &cache);
        assert_eq!(report.files, 0);
        assert_eq!(report.indexed, 0);
        assert_eq!(report.stale, 0);
        assert_eq!(report.total_words, 0);
        assert!(report.pipeline_current); // vacuously true
    }

    #[test]
    fn test_status_no_cache() {
        let (cache, _tmp) = temp_cache();
        let dir = make_test_dir();
        let files = discover_files(dir.path());
        let report = status(&files, &cache);
        assert_eq!(report.files, 2);
        assert_eq!(report.indexed, 0);
        assert_eq!(report.stale, 0);
        assert!(!report.pipeline_current);
    }

    #[test]
    fn test_status_all_indexed() {
        let (cache, _tmp) = temp_cache();
        let dir = make_test_dir();
        let files = discover_files(dir.path());

        // Manually put entries in cache
        for f in &files {
            let content = fs::read_to_string(f).unwrap();
            let hash = NqCache::content_hash(&content);
            let tree = aq_core::node::OwnedNode::leaf("document", &content, 1);
            cache.put(f, &hash, &tree).unwrap();
        }

        let report = status(&files, &cache);
        assert_eq!(report.files, 2);
        assert_eq!(report.indexed, 2);
        assert_eq!(report.stale, 0);
        assert!(report.pipeline_current);
    }

    #[test]
    fn test_status_some_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().to_path_buf();

        let dir = make_test_dir();
        let files = discover_files(dir.path());

        // Index both with version 0.1.0
        let cache_old = NqCache::open_at_with_version(cache_dir.clone(), "0.1.0").unwrap();
        for f in &files {
            let content = fs::read_to_string(f).unwrap();
            let hash = NqCache::content_hash(&content);
            let tree = aq_core::node::OwnedNode::leaf("document", &content, 1);
            cache_old.put(f, &hash, &tree).unwrap();
        }

        // Check with version 0.2.0 → both stale
        let cache_new = NqCache::open_at_with_version(cache_dir, "0.2.0").unwrap();
        let report = status(&files, &cache_new);
        assert_eq!(report.stale, 2);
        assert!(!report.pipeline_current);
    }

    #[test]
    fn test_dry_run_estimates() {
        let (cache, _tmp) = temp_cache();
        let dir = make_test_dir();
        let files = discover_files(dir.path());

        let report = dry_run(&files, &cache);
        assert_eq!(report.files_to_index, 2);
        assert!(report.estimated_words > 0);
        assert!(report.estimated_time_seconds > 0.0);
    }

    #[test]
    fn test_dry_run_skips_cached() {
        let (cache, _tmp) = temp_cache();
        let dir = make_test_dir();
        let files = discover_files(dir.path());

        // Cache one file
        let content = fs::read_to_string(&files[0]).unwrap();
        let hash = NqCache::content_hash(&content);
        let tree = aq_core::node::OwnedNode::leaf("document", &content, 1);
        cache.put(&files[0], &hash, &tree).unwrap();

        let report = dry_run(&files, &cache);
        assert_eq!(report.files_to_index, 1); // only the uncached file
    }

    #[test]
    fn test_prune_deferred() {
        let (cache, _tmp) = temp_cache();
        let pruned = prune(&[], &cache);
        assert_eq!(pruned, 0);
    }
}
