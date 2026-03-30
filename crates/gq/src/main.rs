use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "gq",
    about = "Git Query — structured JSON output from git history",
    version
)]
struct Cli {
    /// File content at a git ref (pipe to aq)
    #[arg(long)]
    at: Option<String>,

    /// Files changed since date or ref
    #[arg(long)]
    changed_since: Option<String>,

    /// Commit log as JSON
    #[arg(long)]
    log: bool,

    /// Structured diff as JSON
    #[arg(long)]
    diff: Option<String>,

    /// Per-line attribution as JSON
    #[arg(long)]
    blame: Option<String>,

    /// Change frequency per file
    #[arg(long)]
    churn: bool,

    /// Number of commits for --log
    #[arg(short = 'n', default_value = "20")]
    count: usize,

    /// Only show file names in diff (no stats)
    #[arg(long)]
    files_only: bool,

    /// Filter churn/changed-since by date
    #[arg(long)]
    since: Option<String>,

    /// Repository path (default: current directory)
    #[arg(long, short = 'C')]
    repo: Option<PathBuf>,

    /// Remaining positional args (paths, file for --at)
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let repo_path = cli.repo.as_deref();
    let repo = gq::open_repo(repo_path)?;

    if let Some(ref rev) = cli.at {
        let file = cli
            .args
            .first()
            .ok_or_else(|| anyhow::anyhow!("Usage: gq --at <ref> <file>"))?;
        let content = gq::cmd_at(&repo, rev, file)?;
        print!("{content}");
    } else if let Some(ref since) = cli.changed_since {
        let entries = gq::cmd_changed_since(&repo, since, &cli.args)?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if cli.log {
        let entries = gq::cmd_log(&repo, cli.count, &cli.args)?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if let Some(ref range) = cli.diff {
        let result = gq::cmd_diff(&repo, range, cli.files_only, &cli.args)?;
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(ref file) = cli.blame {
        let lines = gq::cmd_blame(&repo, file)?;
        println!("{}", serde_json::to_string_pretty(&lines)?);
    } else if cli.churn {
        let entries = gq::cmd_churn(&repo, cli.since.as_deref(), &cli.args)?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        Cli::parse_from(["gq", "--help"]);
    }

    Ok(())
}
