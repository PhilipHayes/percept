use std::path::{Path, PathBuf};

/// Resolve which `.agents/wq.db` a given directory belongs to.
///
/// Walks up from `start_dir` looking for an existing `.agents/wq.db` file.
/// If one is found, its path is returned. If none is found by the time the
/// filesystem root is reached, `start_dir.join(".agents/wq.db")` is returned
/// as the path a caller should create.
///
/// This is pure filesystem inspection — no DB is opened here (D-wq-1-5).
pub fn resolve_project_db_path(start_dir: &Path) -> PathBuf {
    for ancestor in start_dir.ancestors() {
        let candidate = ancestor.join(".agents").join("wq.db");
        if candidate.is_file() {
            return candidate;
        }
    }
    start_dir.join(".agents").join("wq.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn finds_wq_db_planted_in_a_grandparent_dir() {
        let root = TempDir::new().unwrap();
        let grandparent = root.path();
        let agents_dir = grandparent.join(".agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        let db_path = agents_dir.join("wq.db");
        std::fs::write(&db_path, b"").unwrap();

        let child = grandparent.join("child").join("grandchild");
        std::fs::create_dir_all(&child).unwrap();

        let resolved = resolve_project_db_path(&child);
        assert_eq!(resolved, db_path);
    }

    #[test]
    fn falls_back_to_start_dir_when_nothing_found() {
        // Environmental assumption: no real .agents/wq.db exists in any
        // ancestor of the OS temp directory (e.g. /tmp, /) on the test
        // runner's machine. If that assumption is ever violated, this test
        // would need a different isolation strategy (e.g. chrooting or
        // stubbing the filesystem walk).
        let start = TempDir::new().unwrap();

        let resolved = resolve_project_db_path(start.path());
        assert_eq!(resolved, start.path().join(".agents").join("wq.db"));
    }
}
