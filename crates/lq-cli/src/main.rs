use std::io::{self, BufRead, BufReader, IsTerminal, Read, Seek, SeekFrom};
use std::time::Duration;

use clap::Parser;

use lq_core::{execute_pipeline, parse_pipeline, Stage};
use lq_parse::{detect_format, parse_line, Format};

#[derive(Parser, Debug)]
#[command(name = "lq", about = "Log Query — jq for log files")]
struct Cli {
    /// The lq filter expression (e.g. `level:error | count by source`)
    query: Option<String>,

    /// Log files to query
    #[arg(trailing_var_arg = true)]
    files: Vec<String>,

    /// Force a specific format: json, logfmt, bracket
    #[arg(long)]
    format: Option<String>,

    /// Summary mode — structural overview (~100 tokens)
    #[arg(long)]
    summary: bool,

    /// Budget: cap output at roughly N tokens (progressive disclosure)
    #[arg(long)]
    budget: Option<usize>,

    /// Follow mode: tail -f with live filtering
    #[arg(long, short = 'f')]
    follow: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Build actual file list
    let mut files: Vec<&str> = cli.files.iter().map(|s| s.as_str()).collect();
    let query_is_file = cli
        .query
        .as_ref()
        .map_or(false, |q| std::path::Path::new(q).exists());
    if query_is_file {
        files.insert(0, cli.query.as_ref().unwrap());
    }

    // Parse pipeline from query (if it's not a file path)
    let pipeline = if let Some(ref q) = cli.query {
        if !query_is_file {
            parse_pipeline(q)
        } else {
            parse_pipeline("")
        }
    } else {
        parse_pipeline("")
    };

    // Determine if we need buffered (aggregation/context) or streaming mode
    let needs_buffer = pipeline.stages.iter().any(|s| {
        matches!(
            s,
            Stage::CountBy(_)
                | Stage::Rate(_)
                | Stage::Context(_)
                | Stage::Patterns
                | Stage::Timeline
        )
    });

    // --follow mode: tail -f a single file
    if cli.follow {
        if files.is_empty() {
            anyhow::bail!("lq: --follow requires at least one file argument");
        }
        return follow_file(&files[0], &pipeline, cli.budget);
    }

    // --budget mode wraps output with progressive disclosure
    let budget = cli.budget;

    if files.is_empty() {
        // Read from stdin
        if io::stdin().is_terminal() {
            anyhow::bail!("lq: no input. Provide file arguments or pipe data to stdin.");
        }

        if needs_buffer || cli.summary {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            let lines: Vec<&str> = buf.lines().collect();
            let format = resolve_format(&cli.format, &lines);

            if cli.summary {
                print_summary("<stdin>", &lines, format)?;
            } else {
                let results = process_buffered_results(&lines, format, &pipeline);
                output_with_budget(&results, budget)?;
            }
        } else {
            // Streaming: parse and filter line by line
            let stdin = io::stdin();
            let reader = BufReader::new(stdin.lock());
            let mut format_detected = false;
            let mut format = Format::Unknown;

            // Buffer first 20 lines for detection, then stream
            let mut detect_buf: Vec<String> = Vec::new();

            for line_result in reader.lines() {
                let line = line_result?;

                if !format_detected {
                    detect_buf.push(line);
                    if detect_buf.len() >= 20 {
                        let refs: Vec<&str> = detect_buf.iter().map(|s| s.as_str()).collect();
                        format = resolve_format(&cli.format, &refs);
                        format_detected = true;
                        // Process buffered detection lines
                        for l in &detect_buf {
                            process_streaming_line(l, format, &pipeline)?;
                        }
                        detect_buf.clear();
                    }
                } else {
                    process_streaming_line(&line, format, &pipeline)?;
                }
            }

            // If we never hit 20 lines, detect from what we have
            if !format_detected && !detect_buf.is_empty() {
                let refs: Vec<&str> = detect_buf.iter().map(|s| s.as_str()).collect();
                format = resolve_format(&cli.format, &refs);
                for l in &detect_buf {
                    process_streaming_line(l, format, &pipeline)?;
                }
            }
        }
    } else {
        // File mode — check for multi-file timeline
        let has_timeline = pipeline.stages.iter().any(|s| matches!(s, Stage::Timeline));

        if has_timeline && files.len() > 1 {
            // Multi-file timeline: merge all entries, sort by timestamp
            let mut all_entries = Vec::new();
            for path in &files {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("lq: {}: {}", path, e))?;
                let lines: Vec<&str> = content.lines().collect();
                if lines.is_empty() {
                    continue;
                }
                let format = resolve_format(&cli.format, &lines);
                for line in &lines {
                    let mut entry = parse_line(line, format);
                    // Tag source with filename if not already set
                    if entry.source.is_none() {
                        entry.source = Some(path.to_string());
                    }
                    all_entries.push(entry);
                }
            }
            let results = execute_pipeline(&all_entries, &pipeline);
            output_with_budget(&results, budget)?;
        } else {
            for path in &files {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("lq: {}: {}", path, e))?;
                let lines: Vec<&str> = content.lines().collect();
                if lines.is_empty() {
                    continue;
                }

                let format = resolve_format(&cli.format, &lines);

                if cli.summary {
                    print_summary(path, &lines, format)?;
                } else if needs_buffer {
                    let results = process_buffered_results(&lines, format, &pipeline);
                    output_with_budget(&results, budget)?;
                } else {
                    // Stream from file
                    for line in &lines {
                        process_streaming_line(line, format, &pipeline)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn resolve_format(user_format: &Option<String>, sample: &[&str]) -> Format {
    match user_format {
        Some(f) => match f.as_str() {
            "json" => Format::Json,
            "logfmt" => Format::Logfmt,
            "bracket" => Format::Bracket,
            "cri" | "docker" | "docker-cri" => Format::DockerCri,
            "syslog" => Format::Syslog,
            "access" | "access-log" => Format::AccessLog,
            "build" | "build-tool" | "cargo" | "gradle" | "xcode" => Format::BuildTool,
            _ => {
                eprintln!("lq: unknown format '{}', auto-detecting", f);
                detect_format(&sample[..sample.len().min(20)])
            }
        },
        None => detect_format(&sample[..sample.len().min(20)]),
    }
}

fn process_streaming_line(
    line: &str,
    format: Format,
    pipeline: &lq_core::Pipeline,
) -> anyhow::Result<()> {
    let entry = parse_line(line, format);

    // In streaming mode, only filter stages apply
    let passes = pipeline.stages.iter().all(|stage| match stage {
        Stage::Filter(filters) => filters.iter().all(|f| f.matches(&entry)),
        _ => true,
    });

    if passes {
        println!("{}", serde_json::to_string(&entry)?);
    }

    Ok(())
}

fn process_buffered_results(
    lines: &[&str],
    format: Format,
    pipeline: &lq_core::Pipeline,
) -> Vec<serde_json::Value> {
    let entries: Vec<_> = lines.iter().map(|l| parse_line(l, format)).collect();
    execute_pipeline(&entries, pipeline)
}

/// Output results respecting an optional token budget.
/// Progressive disclosure: output entries until ~budget tokens used.
/// Rough heuristic: 1 token ≈ 4 chars of JSON.
fn output_with_budget(results: &[serde_json::Value], budget: Option<usize>) -> anyhow::Result<()> {
    match budget {
        None => {
            for val in results {
                println!("{}", serde_json::to_string(val)?);
            }
        }
        Some(max_tokens) => {
            let mut tokens_used = 0usize;
            let mut disclosed = 0usize;
            for val in results {
                let json = serde_json::to_string(val)?;
                let entry_tokens = json.len() / 4 + 1;
                if tokens_used + entry_tokens > max_tokens && disclosed > 0 {
                    let remaining = results.len() - disclosed;
                    if remaining > 0 {
                        println!(
                            "{}",
                            serde_json::to_string(&serde_json::json!({
                                "_truncated": true,
                                "shown": disclosed,
                                "remaining": remaining,
                                "total": results.len(),
                                "hint": "Increase --budget to see more"
                            }))?
                        );
                    }
                    break;
                }
                println!("{}", json);
                tokens_used += entry_tokens;
                disclosed += 1;
            }
        }
    }
    Ok(())
}

/// Follow a file like `tail -f`, applying filters to new lines.
fn follow_file(
    path: &str,
    pipeline: &lq_core::Pipeline,
    budget: Option<usize>,
) -> anyhow::Result<()> {
    use std::fs::File;

    let mut file = File::open(path)
        .map_err(|e| anyhow::anyhow!("lq: {}: {}", path, e))?;

    // Read initial content for format detection
    let mut initial = String::new();
    file.read_to_string(&mut initial)?;
    let init_lines: Vec<&str> = initial.lines().collect();
    let format = resolve_format(&None, &init_lines);

    // Seek to end
    file.seek(SeekFrom::End(0))?;
    let mut reader = BufReader::new(file);
    let mut tokens_used = 0usize;
    let max_tokens = budget.unwrap_or(usize::MAX);

    eprintln!("lq: following {} ({:?})", path, format);

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }

        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        let entry = parse_line(trimmed, format);

        // Apply filter stages
        let passes = pipeline.stages.iter().all(|stage| match stage {
            Stage::Filter(filters) => filters.iter().all(|f| f.matches(&entry)),
            _ => true,
        });

        if passes {
            let json = serde_json::to_string(&entry)?;
            let entry_tokens = json.len() / 4 + 1;
            if tokens_used + entry_tokens > max_tokens {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "_budget_exhausted": true,
                        "tokens_used": tokens_used,
                        "budget": max_tokens,
                    }))?
                );
                break;
            }
            println!("{}", json);
            tokens_used += entry_tokens;
        }

        line.clear();
    }

    Ok(())
}

fn print_summary(name: &str, lines: &[&str], format: Format) -> anyhow::Result<()> {
    let mut levels = std::collections::HashMap::new();
    let mut sources = std::collections::HashSet::new();
    let mut first_ts = None;
    let mut last_ts = None;
    let mut error_messages: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for line in lines {
        let entry = parse_line(line, format);
        if let Some(level) = entry.level {
            *levels.entry(level).or_insert(0u64) += 1;
            if level >= lq_parse::Level::Error {
                let msg = if entry.message.len() > 80 {
                    format!("{}...", &entry.message[..77])
                } else {
                    entry.message.clone()
                };
                *error_messages.entry(msg).or_insert(0) += 1;
            }
        }
        if let Some(ref src) = entry.source {
            sources.insert(src.clone());
        }
        if let Some(ts) = entry.timestamp {
            if first_ts.is_none() || ts < first_ts.unwrap() {
                first_ts = Some(ts);
            }
            if last_ts.is_none() || ts > last_ts.unwrap() {
                last_ts = Some(ts);
            }
        }
    }

    let total = lines.len() as f64;
    let error_count = levels.get(&lq_parse::Level::Error).copied().unwrap_or(0)
        + levels.get(&lq_parse::Level::Fatal).copied().unwrap_or(0);
    let error_rate = if total > 0.0 { error_count as f64 / total } else { 0.0 };

    // Top 5 errors by frequency
    let mut top_errors: Vec<_> = error_messages.into_iter().collect();
    top_errors.sort_by(|a, b| b.1.cmp(&a.1));
    top_errors.truncate(5);

    let summary = serde_json::json!({
        "file": name,
        "format": format,
        "lines": lines.len(),
        "time_range": {
            "first": first_ts,
            "last": last_ts,
        },
        "levels": levels,
        "error_rate": format!("{:.1}%", error_rate * 100.0),
        "top_errors": top_errors.into_iter().map(|(msg, count)| {
            serde_json::json!({"message": msg, "count": count})
        }).collect::<Vec<_>>(),
        "sources": sources.into_iter().collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
