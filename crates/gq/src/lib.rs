use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use git2::{Diff, DiffOptions, Repository, Sort};
use serde::Serialize;

/// Open the git repository that contains the given path (or cwd).
pub fn open_repo(path: Option<&Path>) -> Result<Repository> {
    let start = path.unwrap_or_else(|| Path::new("."));
    Repository::discover(start).context("not inside a git repository")
}

// ---------------------------------------------------------------------------
// --at: file content at a ref
// ---------------------------------------------------------------------------

pub fn cmd_at(repo: &Repository, rev: &str, file: &str) -> Result<String> {
    let obj = repo
        .revparse_single(&format!("{rev}:{file}"))
        .with_context(|| format!("cannot find '{file}' at ref '{rev}'"))?;
    let blob = obj
        .as_blob()
        .with_context(|| format!("'{file}' at '{rev}' is not a file"))?;
    let content = std::str::from_utf8(blob.content())
        .with_context(|| format!("'{file}' at '{rev}' is not valid UTF-8"))?;
    Ok(content.to_string())
}

// ---------------------------------------------------------------------------
// --log: commit log as JSON
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LogEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files: Vec<FileStatus>,
}

#[derive(Serialize)]
pub struct FileStatus {
    pub status: String,
    pub path: String,
}

pub fn cmd_log(repo: &Repository, count: usize, paths: &[String]) -> Result<Vec<LogEntry>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TIME)?;
    revwalk.push_head()?;

    let mut entries = Vec::new();

    for oid in revwalk {
        if entries.len() >= count {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;

        // Path filtering: skip commits that don't touch any of the requested paths
        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        if !paths.is_empty() {
            let mut diff_opts = DiffOptions::new();
            for p in paths {
                diff_opts.pathspec(p);
            }
            let diff = repo.diff_tree_to_tree(
                parent_tree.as_ref(),
                Some(&tree),
                Some(&mut diff_opts),
            )?;
            if diff.deltas().count() == 0 {
                continue;
            }
        }

        // Collect changed files
        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let files: Vec<FileStatus> = diff
            .deltas()
            .map(|delta| {
                let status_char = match delta.status() {
                    git2::Delta::Added => "A",
                    git2::Delta::Deleted => "D",
                    git2::Delta::Modified => "M",
                    git2::Delta::Renamed => "R",
                    git2::Delta::Copied => "C",
                    _ => "?",
                };
                FileStatus {
                    status: status_char.to_string(),
                    path: delta
                        .new_file()
                        .path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                }
            })
            .collect();

        let author = commit.author();
        let time = commit.time();
        let dt = chrono::DateTime::from_timestamp(time.seconds(), 0)
            .unwrap_or_default()
            .to_rfc3339();

        entries.push(LogEntry {
            hash: format!("{:.8}", oid),
            author: author.name().unwrap_or("").to_string(),
            date: dt,
            message: commit.summary().unwrap_or("").to_string(),
            files,
        });
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// --diff: structured diff as JSON
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct DiffEntry {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Serialize)]
pub struct DiffFileOnly {
    pub status: String,
    pub path: String,
}

pub fn cmd_diff(
    repo: &Repository,
    range: &str,
    files_only: bool,
    paths: &[String],
) -> Result<serde_json::Value> {
    let (from_obj, to_obj) = parse_range(repo, range)?;
    let from_tree = from_obj.peel_to_tree()?;
    let to_tree = to_obj.peel_to_tree()?;

    let mut diff_opts = DiffOptions::new();
    for p in paths {
        diff_opts.pathspec(p);
    }

    let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut diff_opts))?;

    if files_only {
        let entries: Vec<DiffFileOnly> = diff
            .deltas()
            .map(|delta| {
                let status_char = match delta.status() {
                    git2::Delta::Added => "A",
                    git2::Delta::Deleted => "D",
                    git2::Delta::Modified => "M",
                    git2::Delta::Renamed => "R",
                    git2::Delta::Copied => "C",
                    _ => "?",
                };
                DiffFileOnly {
                    status: status_char.to_string(),
                    path: delta
                        .new_file()
                        .path()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                }
            })
            .collect();
        return Ok(serde_json::to_value(entries)?);
    }

    let file_stats = diff_numstat(&diff)?;
    Ok(serde_json::to_value(file_stats)?)
}

fn parse_range<'a>(
    repo: &'a Repository,
    range: &str,
) -> Result<(git2::Object<'a>, git2::Object<'a>)> {
    if let Some((from, to)) = range.split_once("..") {
        let from_obj = repo.revparse_single(from)?;
        let to_obj = repo.revparse_single(to)?;
        Ok((from_obj, to_obj))
    } else {
        // Treat as "range..HEAD" (e.g., "HEAD~3" means "HEAD~3..HEAD")
        let from_obj = repo.revparse_single(range)?;
        let to_obj = repo.revparse_single("HEAD")?;
        Ok((from_obj, to_obj))
    }
}

/// Extract per-file numstat (insertions/deletions) from a Diff using Patch API.
fn diff_numstat(diff: &Diff<'_>) -> Result<Vec<DiffEntry>> {
    let num_deltas = diff.deltas().len();
    let mut entries = Vec::with_capacity(num_deltas);

    for idx in 0..num_deltas {
        if let Some(patch) = git2::Patch::from_diff(diff, idx)? {
            let (_, insertions, deletions) = patch.line_stats()?;
            let path = patch
                .delta()
                .new_file()
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            entries.push(DiffEntry {
                path,
                insertions,
                deletions,
            });
        }
    }

    entries.sort_by(|a, b| (b.insertions + b.deletions).cmp(&(a.insertions + a.deletions)));
    Ok(entries)
}

// ---------------------------------------------------------------------------
// --blame: per-line attribution as JSON
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct BlameLine {
    pub line: usize,
    pub hash: String,
    pub author: String,
    pub timestamp: i64,
    pub content: String,
}

pub fn cmd_blame(repo: &Repository, file: &str) -> Result<Vec<BlameLine>> {
    let blame = repo.blame_file(Path::new(file), None)?;

    // Read current file content for line text
    let workdir = repo.workdir().context("bare repository")?;
    let file_path = workdir.join(file);
    let content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("cannot read '{file}'"))?;
    let lines: Vec<&str> = content.lines().collect();

    let mut result = Vec::new();
    for (i, line_text) in lines.iter().enumerate() {
        let line_num = i + 1;
        if let Some(hunk) = blame.get_line(line_num) {
            let sig = hunk.final_signature();
            result.push(BlameLine {
                line: line_num,
                hash: format!("{:.8}", hunk.final_commit_id()),
                author: sig.name().unwrap_or("").to_string(),
                timestamp: sig.when().seconds(),
                content: line_text.to_string(),
            });
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// --churn: change frequency per file
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ChurnEntry {
    pub path: String,
    pub commits: usize,
    pub insertions: usize,
    pub deletions: usize,
}

pub fn cmd_churn(
    repo: &Repository,
    since: Option<&str>,
    paths: &[String],
) -> Result<Vec<ChurnEntry>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TIME)?;
    revwalk.push_head()?;

    let since_ts = since.and_then(|s| parse_date_to_timestamp(s));

    let mut file_stats: HashMap<String, (usize, usize, usize)> = HashMap::new();

    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;

        // Filter by date
        if let Some(ts) = since_ts {
            if commit.time().seconds() < ts {
                break; // commits are sorted by time, so we can stop
            }
        }

        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let mut diff_opts = DiffOptions::new();
        for p in paths {
            diff_opts.pathspec(p);
        }

        let diff = repo.diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&tree),
            Some(&mut diff_opts),
        )?;

        // Use Patch API to get per-file stats without borrow conflicts
        let num_deltas = diff.deltas().len();
        for idx in 0..num_deltas {
            if let Some(patch) = git2::Patch::from_diff(&diff, idx)? {
                let (_, ins, del) = patch.line_stats()?;
                let path = patch
                    .delta()
                    .new_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let entry = file_stats.entry(path).or_insert((0, 0, 0));
                entry.0 += 1; // commits
                entry.1 += ins;
                entry.2 += del;
            }
        }
    }

    let mut entries: Vec<ChurnEntry> = file_stats
        .into_iter()
        .map(|(path, (commits, ins, del))| ChurnEntry {
            path,
            commits,
            insertions: ins,
            deletions: del,
        })
        .collect();

    entries.sort_by(|a, b| b.commits.cmp(&a.commits));
    Ok(entries)
}

// ---------------------------------------------------------------------------
// --changed-since: files changed since date/ref
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub change_count: usize,
}

pub fn cmd_changed_since(
    repo: &Repository,
    since: &str,
    paths: &[String],
) -> Result<Vec<ChangedFile>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TIME)?;
    revwalk.push_head()?;

    // Try as a rev first, then as a date
    let filter = if let Ok(obj) = repo.revparse_single(since) {
        RevOrDate::Rev(obj.id())
    } else {
        RevOrDate::Date(
            parse_date_to_timestamp(since)
                .context("cannot parse as git ref or date")?,
        )
    };

    let mut file_counts: HashMap<String, usize> = HashMap::new();

    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;

        match &filter {
            RevOrDate::Rev(stop_oid) => {
                if oid == *stop_oid {
                    break;
                }
            }
            RevOrDate::Date(ts) => {
                if commit.time().seconds() < *ts {
                    break;
                }
            }
        }

        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let mut diff_opts = DiffOptions::new();
        for p in paths {
            diff_opts.pathspec(p);
        }

        let diff = repo.diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&tree),
            Some(&mut diff_opts),
        )?;

        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path() {
                *file_counts
                    .entry(path.to_string_lossy().into_owned())
                    .or_default() += 1;
            }
        }
    }

    let mut entries: Vec<ChangedFile> = file_counts
        .into_iter()
        .map(|(path, count)| ChangedFile {
            path,
            change_count: count,
        })
        .collect();

    entries.sort_by(|a, b| b.change_count.cmp(&a.change_count));
    Ok(entries)
}

enum RevOrDate {
    Rev(git2::Oid),
    Date(i64),
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a date string like "2025-01-01" or "3 months ago" to a Unix timestamp.
fn parse_date_to_timestamp(date_str: &str) -> Option<i64> {
    // Try ISO date first
    if let Ok(naive) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let dt = naive.and_hms_opt(0, 0, 0)?;
        return Some(dt.and_utc().timestamp());
    }
    // Try full datetime
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date_str) {
        return Some(dt.timestamp());
    }
    None
}
