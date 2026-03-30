use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use git2::Repository;

#[derive(Parser, Debug)]
#[command(
    name = "oq",
    about = "Observation Query — cached aq results with git-hash invalidation",
    version
)]
struct Cli {
    /// Get cached skeleton/signatures for a file (runs aq on miss)
    #[arg(long)]
    get: Option<PathBuf>,

    /// Warm cache for files (accepts multiple paths)
    #[arg(long)]
    warm: bool,

    /// Show which files need re-caching since a ref/date
    #[arg(long)]
    invalidate: Option<String>,

    /// Show cache statistics
    #[arg(long)]
    stats: bool,

    /// Clear all cache entries
    #[arg(long)]
    clear: bool,

    /// Mode: skeleton, signatures, nq-skeleton, nq-entities, nq-interactions
    #[arg(long, short, default_value = "skeleton")]
    mode: String,

    /// Repository path (default: current directory)
    #[arg(long, short = 'C')]
    repo: Option<PathBuf>,

    /// Files for --warm
    #[arg(trailing_var_arg = true)]
    files: Vec<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cache = oq::Cache::open()?;

    if cli.stats {
        let stats = cache.stats()?;
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(());
    }

    if cli.clear {
        let count = cache.clear()?;
        eprintln!("cleared {count} cache entries");
        return Ok(());
    }

    let repo_path = cli
        .repo
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("."));
    let repo = Repository::discover(repo_path).context("not inside a git repository")?;
    let workdir = repo.workdir().context("bare repository")?.to_path_buf();

    if let Some(ref file) = cli.get {
        let abs = if file.is_absolute() {
            file.clone()
        } else {
            workdir.join(file)
        };
        let (data, was_cached) = cache.get_or_compute(&repo, &abs, &cli.mode)?;
        if was_cached {
            eprintln!("cache hit");
        } else {
            eprintln!("cache miss — computed and stored");
        }
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else if cli.warm {
        let files: Vec<PathBuf> = if cli.files.is_empty() {
            // Warm all tracked files
            let head = repo.head()?.peel_to_tree()?;
            let mut paths = Vec::new();
            head.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
                if entry.kind() == Some(git2::ObjectType::Blob) {
                    let path = if dir.is_empty() {
                        entry.name().unwrap_or("").to_string()
                    } else {
                        format!("{}{}", dir, entry.name().unwrap_or(""))
                    };
                    // Only warm files this mode can process
                    if is_parseable(&path, &cli.mode) {
                        paths.push(PathBuf::from(path));
                    }
                }
                git2::TreeWalkResult::Ok
            })?;
            paths
        } else {
            cli.files
        };

        let result = cache.warm(&repo, &files, &cli.mode)?;
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(ref since) = cli.invalidate {
        let changed = cache.invalidate_changed(&repo, since)?;
        println!("{}", serde_json::to_string_pretty(&changed)?);
        eprintln!(
            "{} files changed — re-warm with: oq --warm {}",
            changed.len(),
            changed
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    } else {
        Cli::parse_from(["oq", "--help"]);
    }

    Ok(())
}

/// Check if a file has an extension the given mode can process.
fn is_parseable(path: &str, mode: &str) -> bool {
    let code = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".kt", ".c", ".cpp", ".h",
        ".hpp", ".swift", ".dart", ".rb", ".cs",
    ];
    let prose = [".md", ".txt", ".yaml", ".yml"];
    if mode.starts_with("nq-") {
        prose.iter().any(|ext| path.ends_with(ext))
    } else {
        code.iter().any(|ext| path.ends_with(ext))
    }
}
