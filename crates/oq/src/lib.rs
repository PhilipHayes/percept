use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Metadata stored alongside each cache entry.
#[derive(Serialize, Deserialize)]
pub struct CacheEntry {
    /// Absolute path to the source file at cache time.
    pub source_path: String,
    /// Git blob OID that was current when cached.
    pub blob_hash: String,
    /// Which aq mode produced this: "skeleton" or "signatures".
    pub mode: String,
    /// The cached aq JSON output.
    pub data: serde_json::Value,
}

/// SHA-256 of a file's raw bytes, returned as a 64-char lowercase hex string.
pub fn content_hash(file: &Path) -> Result<String> {
    let bytes = fs::read(file).with_context(|| format!("cannot read '{}'", file.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Returns the git blob hash for `file` when inside a repo, otherwise falls
/// back to `content_hash`.
pub fn file_hash(file: &Path) -> Result<String> {
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir()?.join(file)
    };

    if let Ok(repo) = Repository::discover(abs.parent().unwrap_or(&abs)) {
        if let Ok(hash) = Cache::blob_hash(&repo, &abs) {
            return Ok(hash);
        }
    }

    content_hash(&abs)
}

/// Cache key derived from file path + pre-computed hash + mode (no git repo needed).
fn cache_key_by_hash(file_path: &str, hash: &str, mode: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(file_path.as_bytes());
    hasher.update(b"|");
    hasher.update(hash.as_bytes());
    hasher.update(b"|");
    hasher.update(mode.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// The observation cache.
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    /// Open (or create) the cache directory.
    /// Respects OQ_CACHE_DIR env var, falls back to ~/.cache/oq.
    pub fn open() -> Result<Self> {
        let dir = if let Ok(custom) = std::env::var("OQ_CACHE_DIR") {
            PathBuf::from(custom)
        } else {
            dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("oq")
        };
        fs::create_dir_all(&dir).context("cannot create cache directory")?;
        Ok(Cache { dir })
    }

    /// Open with a custom directory (for testing).
    pub fn open_at(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).context("cannot create cache directory")?;
        Ok(Cache { dir })
    }

    /// Returns the cache directory path.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Get the current git blob hash for a file in a repo.
    pub fn blob_hash(repo: &Repository, file: &Path) -> Result<String> {
        let workdir = repo.workdir().context("bare repository")?;
        let rel = file
            .strip_prefix(workdir)
            .or_else(|_| {
                // file might already be relative
                Ok::<&Path, std::path::StripPrefixError>(file)
            })
            .unwrap();

        let head = repo.head()?.peel_to_tree()?;
        let entry = head
            .get_path(rel)
            .with_context(|| format!("'{}' not found in HEAD", rel.display()))?;
        Ok(entry.id().to_string())
    }

    /// Compute the cache key from repo root + relative path + blob hash.
    fn cache_key(repo_root: &str, rel_path: &str, blob_hash: &str, mode: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(repo_root.as_bytes());
        hasher.update(b"|");
        hasher.update(rel_path.as_bytes());
        hasher.update(b"|");
        hasher.update(blob_hash.as_bytes());
        hasher.update(b"|");
        hasher.update(mode.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Look up a cached entry. Returns None if not cached or blob hash changed.
    pub fn get(
        &self,
        repo: &Repository,
        file: &Path,
        mode: &str,
    ) -> Result<Option<serde_json::Value>> {
        let blob = Self::blob_hash(repo, file)?;
        let repo_root = repo
            .workdir()
            .context("bare repo")?
            .to_string_lossy()
            .to_string();
        let rel = file
            .strip_prefix(repo.workdir().unwrap())
            .unwrap_or(file);
        let key = Self::cache_key(&repo_root, &rel.to_string_lossy(), &blob, mode);
        let path = self.dir.join(&key);

        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let entry: CacheEntry = serde_json::from_str(&content)?;
            if entry.blob_hash == blob {
                return Ok(Some(entry.data));
            }
        }
        Ok(None)
    }

    /// Store an aq result in the cache.
    pub fn put(
        &self,
        repo: &Repository,
        file: &Path,
        mode: &str,
        data: serde_json::Value,
    ) -> Result<()> {
        let blob = Self::blob_hash(repo, file)?;
        let repo_root = repo
            .workdir()
            .context("bare repo")?
            .to_string_lossy()
            .to_string();
        let rel = file
            .strip_prefix(repo.workdir().unwrap())
            .unwrap_or(file);
        let key = Self::cache_key(&repo_root, &rel.to_string_lossy(), &blob, mode);

        let entry = CacheEntry {
            source_path: file.to_string_lossy().into_owned(),
            blob_hash: blob,
            mode: mode.to_string(),
            data,
        };

        let path = self.dir.join(&key);
        fs::write(&path, serde_json::to_string(&entry)?)?;
        Ok(())
    }

    /// Run aq or nq on a file and return its JSON output.
    pub fn run_aq(file: &Path, mode: &str) -> Result<serde_json::Value> {
        let (cmd, args): (&str, Vec<&str>) = match mode {
            "skeleton" => ("aq", vec!["--skeleton"]),
            "signatures" => ("aq", vec!["--signatures"]),
            "nq-skeleton" => ("nq", vec!["--skeleton"]),
            "nq-entities" => ("nq", vec!["desc:entity", "--format", "compact"]),
            "nq-interactions" => ("nq", vec!["desc:interaction", "--format", "compact"]),
            "nq-coreference" => ("nq", vec!["desc:entity", "--format", "compact"]),
            "nq-thematic-roles" => ("nq", vec!["desc:interaction", "--format", "compact"]),
            "nq-discourse" => ("nq", vec!["desc:discourse", "--format", "compact"]),
            "nq-narrative" => ("nq", vec!["--skeleton"]),
            _ => anyhow::bail!("unknown mode: {mode}"),
        };

        let output = Command::new(cmd)
            .args(&args)
            .arg(file)
            .output()
            .with_context(|| format!("failed to run {cmd} — is it installed?"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("{cmd} failed: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Try parsing as a single JSON value first (skeleton/signatures outputs)
        let json: serde_json::Value = serde_json::from_str(stdout.trim())
            .or_else(|_| {
                // Fall back to NDJSON (one compact JSON object per line)
                let items: Result<Vec<serde_json::Value>, _> = stdout
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| serde_json::from_str(l))
                    .collect();
                items.map(serde_json::Value::Array)
            })
            .with_context(|| format!("{cmd} output is not valid JSON"))?;
        Ok(json)
    }

    /// Get cached or fresh aq output for a file.
    pub fn get_or_compute(
        &self,
        repo: &Repository,
        file: &Path,
        mode: &str,
    ) -> Result<(serde_json::Value, bool)> {
        // Check cache first
        if let Some(cached) = self.get(repo, file, mode)? {
            return Ok((cached, true));
        }

        // Cache miss — run aq
        let data = Self::run_aq(file, mode)?;
        self.put(repo, file, mode, data.clone())?;
        Ok((data, false))
    }

    /// Warm the cache for multiple files.
    pub fn warm(
        &self,
        repo: &Repository,
        files: &[PathBuf],
        mode: &str,
    ) -> Result<WarmResult> {
        let mut result = WarmResult::default();
        for file in files {
            let abs = if file.is_absolute() {
                file.clone()
            } else {
                repo.workdir().unwrap().join(file)
            };

            if !abs.exists() {
                result.skipped += 1;
                continue;
            }

            match self.get_or_compute(repo, &abs, mode) {
                Ok((_, true)) => result.cached += 1,
                Ok((_, false)) => result.computed += 1,
                Err(_) => result.errors += 1,
            }
        }
        Ok(result)
    }

    /// Invalidate cache entries for files that changed since a ref/date.
    /// Returns paths of files that were invalidated.
    pub fn invalidate_changed(
        &self,
        repo: &Repository,
        since: &str,
    ) -> Result<Vec<String>> {
        // Use gq to find changed files
        let output = Command::new("gq")
            .arg("--changed-since")
            .arg(since)
            .arg("-C")
            .arg(repo.workdir().context("bare repo")?)
            .output()
            .context("failed to run gq — is it installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gq failed: {stderr}");
        }

        let entries: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout)?;

        let mut invalidated = Vec::new();
        for entry in &entries {
            if let Some(path) = entry["path"].as_str() {
                invalidated.push(path.to_string());
            }
        }

        // We don't need to delete cache files — the blob hash mismatch
        // means old entries are automatically stale. But we can report
        // which files need re-warming.
        Ok(invalidated)
    }

    /// Count entries in the cache.
    pub fn stats(&self) -> Result<CacheStats> {
        let mut count = 0;
        let mut total_bytes = 0;
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                count += 1;
                total_bytes += entry.metadata()?.len();
            }
        }
        Ok(CacheStats {
            entries: count,
            total_bytes,
            dir: self.dir.clone(),
        })
    }

    /// Clear all cache entries.
    pub fn clear(&self) -> Result<usize> {
        let mut count = 0;
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::remove_file(entry.path())?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Look up a cache entry using a pre-computed hash (no git repo needed).
    pub fn get_by_hash(
        &self,
        file: &Path,
        hash: &str,
        mode: &str,
    ) -> Result<Option<serde_json::Value>> {
        let key = cache_key_by_hash(&file.to_string_lossy(), hash, mode);
        let path = self.dir.join(&key);
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let entry: CacheEntry = serde_json::from_str(&content)?;
            if entry.blob_hash == hash {
                return Ok(Some(entry.data));
            }
        }
        Ok(None)
    }

    /// Store a cache entry using a pre-computed hash (no git repo needed).
    pub fn put_by_hash(
        &self,
        file: &Path,
        hash: &str,
        mode: &str,
        data: serde_json::Value,
    ) -> Result<()> {
        let key = cache_key_by_hash(&file.to_string_lossy(), hash, mode);
        let entry = CacheEntry {
            source_path: file.to_string_lossy().into_owned(),
            blob_hash: hash.to_string(),
            mode: mode.to_string(),
            data,
        };
        let path = self.dir.join(&key);
        fs::write(&path, serde_json::to_string(&entry)?)?;
        Ok(())
    }
}

#[derive(Default, Serialize)]
pub struct WarmResult {
    pub cached: usize,
    pub computed: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Serialize)]
pub struct CacheStats {
    pub entries: usize,
    pub total_bytes: u64,
    pub dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn make_tmp_file(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, content).unwrap();
        (dir, file)
    }

    fn run_git(dir: &Path, args: &[&str]) {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
    }

    fn setup_git_repo_with_file() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        run_git(path, &["init"]);
        run_git(path, &["config", "user.email", "test@test.com"]);
        run_git(path, &["config", "user.name", "Test"]);
        let file = path.join("sample.txt");
        std::fs::write(&file, "hello from git\n").unwrap();
        run_git(path, &["add", "."]);
        run_git(path, &["commit", "-m", "init"]);
        (dir, file)
    }

    // ── S01: content_hash() ──────────────────────────────────────────────────

    #[test]
    fn test_content_hash_deterministic() {
        let (_dir, file) = make_tmp_file("deterministic content");
        let h1 = content_hash(&file).unwrap();
        let h2 = content_hash(&file).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_content_hash_different_content() {
        let (_dir1, file1) = make_tmp_file("content alpha");
        let (_dir2, file2) = make_tmp_file("content beta");
        let h1 = content_hash(&file1).unwrap();
        let h2 = content_hash(&file2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_content_hash_is_64_hex_chars() {
        let (_dir, file) = make_tmp_file("some data");
        let hash = content_hash(&file).unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── S01: file_hash() ────────────────────────────────────────────────────

    #[test]
    fn test_file_hash_non_git() {
        // /tmp is not a git repo — must fall back to content_hash without panicking
        let file = std::path::PathBuf::from("/tmp/oq_test_non_git.txt");
        std::fs::write(&file, "not in git").unwrap();
        let result = file_hash(&file);
        let _ = std::fs::remove_file(&file);
        let hash = result.unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_file_hash_in_git_repo() {
        let (_dir, file) = setup_git_repo_with_file();
        // Should return some hash (blob or content) — not an error
        let hash = file_hash(&file).unwrap();
        assert!(!hash.is_empty());
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_file_hash_nonexistent() {
        let path = std::path::PathBuf::from("/tmp/oq_does_not_exist_xyz.txt");
        let result = file_hash(&path);
        assert!(result.is_err());
    }

    // ── S02: get_by_hash() + put_by_hash() ───────────────────────────────────

    #[test]
    fn test_get_by_hash_miss() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::open_at(dir.path().to_path_buf()).unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        let result = cache.get_by_hash(&file, "abc123", "skeleton").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_by_hash_get_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::open_at(dir.path().to_path_buf()).unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "hello").unwrap();
        let data = serde_json::json!({"key": "value"});
        cache.put_by_hash(&file, "deadbeef", "skeleton", data.clone()).unwrap();
        let got = cache.get_by_hash(&file, "deadbeef", "skeleton").unwrap();
        assert_eq!(got.unwrap(), data);
    }

    #[test]
    fn test_get_by_hash_different_hash() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::open_at(dir.path().to_path_buf()).unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "hello").unwrap();
        cache
            .put_by_hash(&file, "hash_aaa", "skeleton", serde_json::json!({"a": 1}))
            .unwrap();
        let result = cache.get_by_hash(&file, "hash_bbb", "skeleton").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_by_hash_different_mode() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::open_at(dir.path().to_path_buf()).unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "hello").unwrap();
        cache
            .put_by_hash(&file, "hash_x", "skeleton", serde_json::json!({"a": 1}))
            .unwrap();
        let result = cache.get_by_hash(&file, "hash_x", "signatures").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_stats_counts_by_hash_entries() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::open_at(dir.path().to_path_buf()).unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "data").unwrap();
        cache
            .put_by_hash(&file, "h1", "skeleton", serde_json::json!({}))
            .unwrap();
        cache
            .put_by_hash(&file, "h2", "signatures", serde_json::json!({}))
            .unwrap();
        let stats = cache.stats().unwrap();
        assert!(stats.entries >= 2);
    }

    #[test]
    fn test_clear_removes_by_hash_entries() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::open_at(dir.path().to_path_buf()).unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "data").unwrap();
        cache
            .put_by_hash(&file, "hhhh", "skeleton", serde_json::json!({"z": 99}))
            .unwrap();
        cache.clear().unwrap();
        let result = cache.get_by_hash(&file, "hhhh", "skeleton").unwrap();
        assert!(result.is_none());
    }
}
