use aq_nlp::index::{self, IndexOptions, IndexStatus};
use aq_nlp::nq_cache::NqCache;
use std::fs;

fn spacy_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import spacy; spacy.load('en_core_web_sm')"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn make_options(cache_dir: &std::path::Path) -> IndexOptions {
    IndexOptions {
        cache: NqCache::open_at(cache_dir.to_path_buf()).unwrap(),
        dry_run: false,
        force: true,
    }
}

#[test]
fn test_index_single_file() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let file = tmp.path().join("story.txt");
    fs::write(&file, "Joseph dreamed a dream and told it to his brothers.").unwrap();

    let options = make_options(cache_dir.path());
    let results = index::index_files(&[file.clone()], &options);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, IndexStatus::Indexed);
    assert!(results[0].words > 0);
    assert!(results[0].error.is_none());

    // Verify artifact is in cache
    let content = fs::read_to_string(&file).unwrap();
    let hash = NqCache::content_hash(&content);
    let cached = options.cache.get(&file, &hash).unwrap();
    assert!(cached.is_some(), "Artifact should be cached after indexing");
}

#[test]
fn test_index_directory() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("a.txt"), "Jacob sent Joseph to Shechem.").unwrap();
    fs::write(
        tmp.path().join("b.txt"),
        "The brothers cast Joseph into a pit.",
    )
    .unwrap();
    fs::write(tmp.path().join("c.txt"), "Reuben returned to the pit.").unwrap();

    let files = index::discover_files(tmp.path());
    let options = make_options(cache_dir.path());
    let results = index::index_files(&files, &options);

    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.status == IndexStatus::Indexed));
}

#[test]
fn test_index_skips_cached() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("story.txt"),
        "Joseph interpreted Pharaoh's dream.",
    )
    .unwrap();

    let files = index::discover_files(tmp.path());

    // First run: indexes
    let options = IndexOptions {
        cache: NqCache::open_at(cache_dir.path().to_path_buf()).unwrap(),
        dry_run: false,
        force: false,
    };
    let results = index::index_files(&files, &options);
    assert_eq!(results[0].status, IndexStatus::Indexed);

    // Second run with incremental: should be cached
    let options2 = IndexOptions {
        cache: NqCache::open_at(cache_dir.path().to_path_buf()).unwrap(),
        dry_run: false,
        force: false,
    };
    let results2 = index::index_files(&files, &options2);
    assert_eq!(results2[0].status, IndexStatus::Cached);
}

#[test]
fn test_index_md_preprocessing() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let file = tmp.path().join("note.md");
    fs::write(
        &file,
        "---\ntags: [genesis]\n---\n# Joseph's Journey\n\n**Joseph** went to [[Egypt]].",
    )
    .unwrap();

    let options = make_options(cache_dir.path());
    let results = index::index_files(&[file.clone()], &options);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, IndexStatus::Indexed);

    // Verify the cached tree doesn't contain frontmatter artifacts
    let content = fs::read_to_string(&file).unwrap();
    let hash = NqCache::content_hash(&content);
    let cached = options.cache.get(&file, &hash).unwrap().unwrap();

    // Walk tree text to ensure no YAML keys leaked through
    fn collect_text(node: &aq_core::node::OwnedNode, out: &mut Vec<String>) {
        if let Some(t) = &node.text {
            out.push(t.clone());
        }
        for child in &node.children {
            collect_text(child, out);
        }
    }
    let mut texts = Vec::new();
    collect_text(&cached, &mut texts);
    let all_text = texts.join(" ");
    assert!(
        !all_text.contains("tags:"),
        "Frontmatter key should not appear in NLP tree"
    );
}

#[test]
fn test_index_error_handling() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("good.txt"), "Joseph went to Egypt.").unwrap();
    // Create a non-existent file reference
    let nonexistent = tmp.path().join("missing.txt");

    let files = vec![tmp.path().join("good.txt"), nonexistent];
    let options = make_options(cache_dir.path());
    let results = index::index_files(&files, &options);

    assert_eq!(results.len(), 2);
    // Good file succeeds
    assert_eq!(results[0].status, IndexStatus::Indexed);
    // Missing file errors but doesn't crash
    assert_eq!(results[1].status, IndexStatus::Error);
    assert!(results[1].error.is_some());
}

#[test]
fn test_index_result_structure() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("test.txt"), "Joseph dreamed a dream.").unwrap();

    let files = index::discover_files(tmp.path());
    let options = make_options(cache_dir.path());
    let results = index::index_files(&files, &options);

    let r = &results[0];
    assert!(r.file.contains("test.txt"));
    assert_eq!(r.status, IndexStatus::Indexed);
    assert_eq!(r.words, 4); // "Joseph dreamed a dream."
    assert!(r.error.is_none());
}

// ---- S07: Incremental Indexing Tests ----

#[test]
fn test_incremental_skips_unchanged() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("story.txt"),
        "Joseph interpreted Pharaoh's dream.",
    )
    .unwrap();

    let files = index::discover_files(tmp.path());

    // First run: indexes
    let opts1 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: false,
    };
    let r1 = index::index_files(&files, &opts1);
    assert_eq!(r1[0].status, IndexStatus::Indexed);

    // Second run with incremental: all cached, 0 files parsed
    let opts2 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: false,
    };
    let r2 = index::index_files(&files, &opts2);
    assert_eq!(r2[0].status, IndexStatus::Cached);

    let (indexed, cached, _stale, errors) = index::summarize(&r2);
    assert_eq!(indexed, 0);
    assert_eq!(cached, 1);
    assert_eq!(errors, 0);
}

#[test]
fn test_incremental_detects_change() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let file_a = tmp.path().join("a.txt");
    let file_b = tmp.path().join("b.txt");
    fs::write(&file_a, "Joseph went to Egypt.").unwrap();
    fs::write(&file_b, "Jacob mourned for days.").unwrap();

    let files = index::discover_files(tmp.path());

    // First run
    let opts1 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: false,
    };
    let r1 = index::index_files(&files, &opts1);
    assert!(r1.iter().all(|r| r.status == IndexStatus::Indexed));

    // Modify one file
    fs::write(&file_b, "Jacob mourned for many days and nights.").unwrap();

    // Second run: a.txt cached, b.txt re-indexed
    let opts2 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: false,
    };
    let r2 = index::index_files(&files, &opts2);
    let a_result = r2.iter().find(|r| r.file.contains("a.txt")).unwrap();
    let b_result = r2.iter().find(|r| r.file.contains("b.txt")).unwrap();
    assert_eq!(a_result.status, IndexStatus::Cached);
    assert_eq!(b_result.status, IndexStatus::Indexed);
}

#[test]
fn test_incremental_detects_pipeline_change() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("story.txt"), "Joseph dreamed a dream.").unwrap();

    let files = index::discover_files(tmp.path());

    // Index with version A
    let opts1 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: false,
    };
    let r1 = index::index_files(&files, &opts1);
    assert_eq!(r1[0].status, IndexStatus::Indexed);

    // Re-index with version B (incremental): stale-pipeline
    let opts2 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.2.0").unwrap(),
        dry_run: false,
        force: false,
    };
    let r2 = index::index_files(&files, &opts2);
    assert_eq!(r2[0].status, IndexStatus::StalePipeline);

    let (_indexed, _cached, stale, _errors) = index::summarize(&r2);
    assert_eq!(stale, 1);
}

#[test]
fn test_non_incremental_always_reindexes() {
    if !spacy_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("story.txt"),
        "Jacob sent Joseph to his brothers.",
    )
    .unwrap();

    let files = index::discover_files(tmp.path());

    // First run: indexes (non-incremental)
    let opts1 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: true,
    };
    let r1 = index::index_files(&files, &opts1);
    assert_eq!(r1[0].status, IndexStatus::Indexed);

    // Second run without incremental: re-indexes everything
    let opts2 = IndexOptions {
        cache: NqCache::open_at_with_version(cache_dir.path().to_path_buf(), "test-0.1.0").unwrap(),
        dry_run: false,
        force: true,
    };
    let r2 = index::index_files(&files, &opts2);
    assert_eq!(r2[0].status, IndexStatus::Indexed);
}
