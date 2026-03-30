/// CLI integration tests for the nq binary alias.
///
/// Validates that:
/// - The `nq` binary exists and routes to the NLP backend (via argv[0] detection)
/// - When spaCy is available, nq parses text and returns a valid document tree
/// - When spaCy is unavailable, nq exits with a non-zero status
use std::io::Write;
use std::process::Command;

fn nq_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nq"))
}

fn write_temp_file(name: &str, content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(name)
        .tempfile()
        .unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f
}

fn spacy_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import spacy; spacy.load('en_core_web_sm')"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn nq_skeleton_returns_nlp_not_implemented_error() {
    let f = write_temp_file(".rs", "fn main() {}");
    let output = nq_bin()
        .arg("--skeleton")
        .arg(f.path())
        .output()
        .expect("failed to run nq");

    if spacy_available() {
        assert!(
            output.status.success(),
            "nq should succeed when spaCy is available; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        assert!(
            !output.status.success(),
            "nq should exit with an error when spaCy is unavailable"
        );
    }
}

#[test]
fn nq_signatures_returns_nlp_not_implemented_error() {
    let f = write_temp_file(".rs", "fn hello() {}");
    let output = nq_bin()
        .arg("--signatures")
        .arg(f.path())
        .output()
        .expect("failed to run nq");

    if spacy_available() {
        assert!(
            output.status.success(),
            "nq should succeed when spaCy is available; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        assert!(
            !output.status.success(),
            "nq should exit with an error when spaCy is unavailable"
        );
    }
}

// ---------------------------------------------------------------------------
// nq index subcommand tests (S05)
// ---------------------------------------------------------------------------

fn aq_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aq"))
}

#[test]
fn nq_index_help() {
    let output = nq_bin()
        .args(["index", "--help"])
        .output()
        .expect("failed to run nq index --help");
    assert!(output.status.success(), "nq index --help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("index") || stdout.contains("Index"),
        "help output should mention 'index': {stdout}");
}

#[test]
fn nq_index_no_args_errors() {
    let output = nq_bin()
        .arg("index")
        .output()
        .expect("failed to run nq index");
    assert!(!output.status.success(), "nq index with no paths should exit non-zero");
}

#[test]
fn nq_index_accepts_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let output = nq_bin()
        .args(["index", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index <dir>");
    // Should not crash — stub implementation succeeds
    assert!(output.status.success(),
        "nq index <dir> should not crash; stderr: {}",
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn nq_existing_query_unchanged() {
    if !spacy_available() { return; }
    let f = write_temp_file(".txt", "Joseph went to Egypt.");
    let output = nq_bin()
        .args(["--format", "json", "desc:entity", f.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq query");
    assert!(output.status.success(),
        "existing nq query should still work; stderr: {}",
        String::from_utf8_lossy(&output.stderr));
}

#[test]
fn aq_does_not_have_index() {
    let output = aq_bin()
        .args(["index", "/tmp"])
        .output()
        .expect("failed to run aq index");
    // aq treats "index" as a query expression, not a subcommand
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("nq index"),
        "aq should not dispatch to nq index; stderr: {stderr}");
}

#[test]
fn nq_index_real_directory() {
    if !spacy_available() { return; }
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "Joseph went to Egypt.").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "Jacob mourned for days.").unwrap();

    let output = nq_bin()
        .args(["index", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index <dir>");

    assert!(output.status.success(),
        "nq index should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"status\": \"indexed\"") || stdout.contains("\"status\":\"indexed\""),
        "output should contain indexed status; stdout: {stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("2 files") || stderr.contains("found 2"),
        "stderr should report file count; stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// nq index --status / --dry-run / --prune tests (S08)
// ---------------------------------------------------------------------------

#[test]
fn nq_index_status_empty() {
    let tmp = tempfile::tempdir().unwrap();
    // Empty dir — no indexable files
    let output = nq_bin()
        .args(["index", "--status", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index --status");

    assert!(output.status.success(),
        "nq index --status should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no indexable files"),
        "stderr should say no files; stderr: {stderr}");
}

#[test]
fn nq_index_status_json() {
    if !spacy_available() { return; }
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "Joseph went to Egypt.").unwrap();

    // First index
    nq_bin()
        .args(["index", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index");

    // Then check status
    let output = nq_bin()
        .args(["index", "--status", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index --status");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"files\""), "status should have files field: {stdout}");
    assert!(stdout.contains("\"pipeline_current\""), "status should have pipeline_current: {stdout}");
}

#[test]
fn nq_index_dry_run_json() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "Joseph went to Egypt.").unwrap();

    let output = nq_bin()
        .args(["index", "--dry-run", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index --dry-run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"files_to_index\""), "dry-run should have files_to_index: {stdout}");
    assert!(stdout.contains("\"estimated_words\""), "dry-run should have estimated_words: {stdout}");
}

#[test]
fn nq_index_prune() {
    if !spacy_available() { return; }
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "Joseph went to Egypt.").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "Jacob mourned for days.").unwrap();

    // Index both files
    nq_bin()
        .args(["index", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index");

    // Delete one source file
    std::fs::remove_file(tmp.path().join("a.txt")).unwrap();

    // Prune
    let output = nq_bin()
        .args(["index", "--prune", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run nq index --prune");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"pruned\""), "prune output should contain pruned count: {stdout}");
}

// ── S16: --corpus CLI tests ─────────────────────────────────────────────────

#[test]
fn nq_corpus_flag_accepted() {
    if !spacy_available() {
        return;
    }
    let f1 = write_temp_file(".txt", "Joseph went to Egypt.\n");
    let f2 = write_temp_file(".txt", "Joseph served Pharaoh.\n");
    let output = nq_bin()
        .args(["--corpus", "--skeleton"])
        .arg(f1.path())
        .arg(f2.path())
        .output()
        .expect("failed to run nq --corpus --skeleton");

    assert!(
        output.status.success(),
        "nq --corpus --skeleton should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "skeleton output should not be empty");
}

#[test]
fn nq_corpus_without_flag_independent() {
    if !spacy_available() {
        return;
    }
    let f1 = write_temp_file(".txt", "Joseph went to Egypt.\n");
    let f2 = write_temp_file(".txt", "Joseph served Pharaoh.\n");

    // Without --corpus: each file is independent → get per-file output.
    let output = nq_bin()
        .args(["--skeleton"])
        .arg(f1.path())
        .arg(f2.path())
        .output()
        .expect("failed to run nq --skeleton");

    assert!(
        output.status.success(),
        "nq --skeleton (no --corpus) should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nq_corpus_skeleton_has_corpus_fields() {
    if !spacy_available() {
        return;
    }
    let f1 = write_temp_file(".txt", "Joseph went to Egypt. He traveled far.\n");
    let f2 = write_temp_file(".txt", "Joseph served Pharaoh faithfully.\n");
    let output = nq_bin()
        .args(["--corpus", "--skeleton"])
        .arg(f1.path())
        .arg(f2.path())
        .output()
        .expect("failed to run nq --corpus --skeleton");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Invalid JSON: {e}\nstdout: {stdout}"));

    assert_eq!(json["mode"], "corpus");
    assert!(json["files"].is_array(), "should have files array");
    assert!(json["file_count"].as_u64().unwrap() >= 2, "should have >= 2 files");
    assert!(json["total_paragraphs"].is_number(), "should have total_paragraphs");
    assert!(json["characters"].is_array(), "should have characters array");
    assert!(json["arc_distribution"].is_object(), "should have arc_distribution");
    assert!(json.get("central_conflict").is_some(), "should have central_conflict");
    assert!(json.get("scenes").is_some(), "should have scenes");
}
