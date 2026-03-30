use anyhow::Result;
use clap::Parser;
use std::io::{self, Read};

use tq_parse::{detect_format, parse_output};
use tq_core::diff::diff_runs;
use tq_core::flaky::detect_flaky;

#[derive(Parser)]
#[command(name = "tq", about = "Test Results Query — structured test output for agents")]
struct Cli {
    /// Show summary (total/passed/failed/skipped + failures)
    #[arg(long)]
    summary: bool,

    /// Force test output format
    #[arg(long)]
    format: Option<String>,

    /// Budget in approximate tokens (~4 chars/token)
    #[arg(long)]
    budget: Option<usize>,

    /// Save parsed TestRun as JSON to this path
    #[arg(long)]
    save: Option<String>,

    /// Diff two saved TestRun JSON files: --diff before.json after.json
    #[arg(long, num_args = 2)]
    diff: Option<Vec<String>>,

    /// Detect flaky tests across multiple saved TestRun JSON files
    #[arg(long, num_args = 1..)]
    flaky: Option<Vec<String>>,

    /// Input file(s). Reads stdin if omitted.
    files: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // --diff mode: compare two saved TestRun JSON files
    if let Some(ref diff_files) = cli.diff {
        let before: tq_parse::TestRun =
            serde_json::from_str(&std::fs::read_to_string(&diff_files[0])?)?;
        let after: tq_parse::TestRun =
            serde_json::from_str(&std::fs::read_to_string(&diff_files[1])?)?;
        let result = diff_runs(&before, &after);
        let output = serde_json::to_string_pretty(&result)?;
        println!("{}", truncate_output(&output, &cli.budget));
        return Ok(());
    }

    // --flaky mode: detect flaky tests across multiple saved runs
    if let Some(ref flaky_files) = cli.flaky {
        let mut runs = Vec::new();
        for path in flaky_files {
            let run: tq_parse::TestRun =
                serde_json::from_str(&std::fs::read_to_string(path)?)?;
            runs.push(run);
        }
        let report = detect_flaky(&runs);
        let output = serde_json::to_string_pretty(&report)?;
        println!("{}", truncate_output(&output, &cli.budget));
        return Ok(());
    }

    let input = if cli.files.is_empty() {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        let mut buf = String::new();
        for path in &cli.files {
            buf.push_str(&std::fs::read_to_string(path)?);
            buf.push('\n');
        }
        buf
    };

    if input.trim().is_empty() {
        eprintln!("tq: no input");
        std::process::exit(1);
    }

    let lines: Vec<&str> = input.lines().collect();
    let format = resolve_format(&cli.format, &lines);
    let run = parse_output(&input, format);

    // --save: persist parsed TestRun as JSON
    if let Some(ref save_path) = cli.save {
        let json = serde_json::to_string_pretty(&run)?;
        std::fs::write(save_path, &json)?;
        eprintln!("tq: saved to {}", save_path);
    }

    if cli.summary {
        let summary = serde_json::json!({
            "total": run.total,
            "passed": run.passed,
            "failed": run.failed,
            "skipped": run.skipped,
            "errored": run.errored,
            "duration_ms": run.duration_ms,
            "format": run.format,
            "runner": run.runner,
            "failures": run.failures().iter().map(|t| {
                serde_json::json!({
                    "test": t.name,
                    "message": t.message,
                })
            }).collect::<Vec<_>>(),
        });
        let output = serde_json::to_string_pretty(&summary)?;
        println!("{}", truncate_output(&output, &cli.budget));
    } else {
        let output = serde_json::to_string_pretty(&run)?;
        println!("{}", truncate_output(&output, &cli.budget));
    }

    Ok(())
}

fn truncate_output(output: &str, budget: &Option<usize>) -> String {
    if let Some(budget) = budget {
        let max_chars = budget * 4;
        if output.len() > max_chars {
            return format!(
                "{}\n... _truncated (use --budget {} for more)",
                &output[..max_chars],
                budget * 2
            );
        }
    }
    output.to_string()
}

fn resolve_format(user_format: &Option<String>, sample: &[&str]) -> tq_parse::Format {
    use tq_parse::Format;
    match user_format {
        Some(f) => match f.as_str() {
            "libtest" | "cargo" => Format::Libtest,
            "libtest-json" | "cargo-json" => Format::LibtestJson,
            "pytest" => Format::Pytest,
            "junit" | "junit-xml" => Format::Junit,
            "jest" => Format::Jest,
            "go" | "gotest" => Format::GoTest,
            "go-json" | "gotest-json" => Format::GoTestJson,
            "tap" => Format::Tap,
            "flutter" => Format::Flutter,
            _ => {
                eprintln!("tq: unknown format '{}', auto-detecting", f);
                detect_format(sample)
            }
        },
        None => detect_format(sample),
    }
}
