use std::io::{self, IsTerminal, Read};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "aq", about = "AST Query — jq for syntax trees")]
struct Cli {
    /// The aq query expression
    query: Option<String>,

    /// Source files to query (supports globs like src/**/*.rs)
    #[arg(trailing_var_arg = true)]
    files: Vec<String>,

    /// Force a specific language (otherwise inferred from extension)
    #[arg(long)]
    lang: Option<String>,

    /// Output format: json (default), compact, text
    #[arg(long, default_value = "json")]
    format: String,

    /// Token budget — truncate output to approximately this many tokens
    #[arg(long)]
    budget: Option<usize>,

    /// Skeleton mode — output structural overview of files
    #[arg(long)]
    skeleton: bool,

    /// Signatures mode — output function/method signatures
    #[arg(long)]
    signatures: bool,

    /// JSON query mode — accept a structured JSON query instead of DSL string
    #[arg(long)]
    query_json: Option<String>,

    /// Include all nodes (including anonymous/punctuation). Default: named only.
    #[arg(long)]
    all_nodes: bool,

    /// Show parse confidence metadata (_confidence field in output).
    /// Reports error/missing node counts so agents can calibrate trust.
    #[arg(long)]
    confidence: bool,

    /// Show query plan without executing
    #[arg(long)]
    explain: bool,

    /// Corpus mode — combine multiple files into a single narrative analysis
    #[arg(long)]
    corpus: bool,

    /// File ordering for corpus mode: path (default), name, natural
    #[arg(long, default_value = "path")]
    order: String,

    /// Paragraph-based coreference window for corpus mode
    #[arg(long, default_value = "100")]
    coref_window: usize,
}

/// CLI for `nq index` subcommand.
#[derive(Parser, Debug)]
#[command(name = "nq index", about = "Index files for corpus-mode NLP analysis")]
struct IndexCli {
    /// Directory or glob pattern of files to index
    paths: Vec<String>,

    /// Force re-index all files, ignoring cache
    #[arg(long)]
    force: bool,

    /// Show index status without re-indexing
    #[arg(long)]
    status: bool,

    /// Show what would be indexed without actually indexing
    #[arg(long)]
    dry_run: bool,

    /// Remove cache entries for files no longer present
    #[arg(long)]
    prune: bool,

    /// Number of parallel workers (default: number of CPUs)
    #[arg(long)]
    workers: Option<usize>,

    /// Output format: json (default), msgpack
    #[arg(long, default_value = "json")]
    format: String,

    /// spaCy model name
    #[arg(long, default_value = "en_core_web_sm")]
    model: String,

    /// Paragraph-based coreference window
    #[arg(long, default_value = "100")]
    coref_window: usize,
}

fn main() -> anyhow::Result<()> {
    // Early dispatch: `nq index` subcommand (before clap parses the main Cli)
    if is_nq_binary() {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 && args[1] == "index" {
            // Re-parse with IndexCli, skipping argv[0] and "index"
            let index_args = std::iter::once(args[0].clone()).chain(args[2..].iter().cloned());
            let index_cli = IndexCli::parse_from(index_args);
            return run_nq_index(index_cli);
        }
    }

    let cli = Cli::parse();

    // Budget tracking — shared across all files
    let mut budget_tracker = BudgetTracker::new(cli.budget);

    // --query-json mode: structured JSON query for programmatic agent use
    // In this mode, the `query` positional arg is treated as the first file path
    // since the query comes from --query-json.
    if let Some(ref json_query) = cli.query_json {
        let query_str = compile_json_query(json_query)?;
        let sources = collect_sources_no_query(&cli)?;
        let backend = resolve_backend(cli.lang.as_deref());
        let tokens = aq_core::lex(&query_str)
            .map_err(|e| anyhow::anyhow!("{}", format_lex_error(&query_str, &e)))?;
        let expr = aq_core::parse(&tokens)
            .map_err(|e| anyhow::anyhow!("{}", format_parse_error(&query_str, &e)))?;
        if cli.explain {
            eprintln!("Compiled query: {}", query_str);
            eprintln!("{:#?}", expr);
            return Ok(());
        }
        for (source, lang, file_path) in sources {
            let (root, _metrics) =
                parse_source(&*backend, &source, &lang, &file_path, cli.all_nodes, false)?;
            let results = aq_core::eval(&expr, &root).map_err(|e| anyhow::anyhow!("{}", e))?;
            for result in &results {
                let output = format_result(result, &cli.format);
                budget_tracker.emit(&output);
            }
        }
        budget_tracker.print_truncation_metadata();
        return Ok(());
    }

    // --corpus mode: merge multiple files into a single narrative analysis.
    if cli.corpus {
        return run_corpus_mode(&cli, &mut budget_tracker);
    }

    // --skeleton mode: structural overview, no query required.
    // In skeleton mode, the positional `query` arg is treated as the first file path
    // since no query expression is needed.
    if cli.skeleton {
        let sources = collect_sources_no_query(&cli)?;
        let backend = resolve_backend(cli.lang.as_deref());
        for (source, lang, file_path) in sources {
            let (root, metrics) = parse_source(
                &*backend,
                &source,
                &lang,
                &file_path,
                cli.all_nodes,
                cli.confidence,
            )?;
            let output = if root.node_type == "document" {
                format_nlp_skeleton(&root, &file_path, &cli.format)
            } else {
                format_skeleton(&root, &file_path, &cli.format, metrics.as_ref())
            };
            budget_tracker.emit(&output);
        }
        budget_tracker.print_truncation_metadata();
        return Ok(());
    }

    // --signatures mode: function/method signatures
    if cli.signatures {
        let sources = collect_sources_no_query(&cli)?;
        let backend = resolve_backend(cli.lang.as_deref());
        for (source, lang, file_path) in sources {
            let (root, metrics) = parse_source(
                &*backend,
                &source,
                &lang,
                &file_path,
                cli.all_nodes,
                cli.confidence,
            )?;
            let output = format_signatures(&root, &file_path, &cli.format, metrics.as_ref());
            budget_tracker.emit(&output);
        }
        budget_tracker.print_truncation_metadata();
        return Ok(());
    }

    // Normal query mode — query is required
    let query_str = match cli.query {
        Some(ref q) => q.as_str(),
        None => {
            eprintln!(
                "aq: no query provided. Use --skeleton for structural overview, or pass a query."
            );
            std::process::exit(1);
        }
    };

    run_query_str(&cli, query_str, &mut budget_tracker)
}

fn run_query_str(
    cli: &Cli,
    query_str: &str,
    budget_tracker: &mut BudgetTracker,
) -> anyhow::Result<()> {
    // Lex the query — with enhanced error messages
    let tokens = aq_core::lex(query_str)
        .map_err(|e| anyhow::anyhow!("{}", format_lex_error(query_str, &e)))?;

    // Parse the query — with enhanced error messages
    let expr = aq_core::parse(&tokens)
        .map_err(|e| anyhow::anyhow!("{}", format_parse_error(query_str, &e)))?;

    if cli.explain {
        eprintln!("{:#?}", expr);
        return Ok(());
    }

    // Collect sources (files or stdin)
    let sources = collect_sources(cli)?;
    let backend = resolve_backend(cli.lang.as_deref());

    // Process each source
    for (source, lang, file_path) in sources {
        let (root, _metrics) =
            parse_source(&*backend, &source, &lang, &file_path, cli.all_nodes, false)?;
        let results = aq_core::eval(&expr, &root).map_err(|e| anyhow::anyhow!("{}", e))?;

        for result in &results {
            let output = format_result(result, &cli.format);
            budget_tracker.emit(&output);
        }
    }

    budget_tracker.print_truncation_metadata();
    Ok(())
}

// ---------------------------------------------------------------------------
// nq index subcommand
// ---------------------------------------------------------------------------

fn run_nq_index(cli: IndexCli) -> anyhow::Result<()> {
    if cli.paths.is_empty() {
        eprintln!("nq index: no paths provided\n\nUsage: nq index <PATH>...\n\nFor more information, try 'nq index --help'");
        std::process::exit(2);
    }

    let cache = aq_nlp::nq_cache::NqCache::open()?;

    // Discover files from paths/globs
    let files = aq_nlp::index::expand_globs(&cli.paths).map_err(|e| anyhow::anyhow!("{}", e))?;

    if files.is_empty() {
        eprintln!("nq index: no indexable files found");
        return Ok(());
    }

    // --status: read-only report
    if cli.status {
        let report = aq_nlp::index::status(&files, &cache);
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(());
    }

    // --dry-run: estimation only
    if cli.dry_run {
        let report = aq_nlp::index::dry_run(&files, &cache);
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(());
    }

    // --prune: remove orphan cache entries
    if cli.prune {
        let pruned = aq_nlp::index::prune(&files, &cache);
        println!("{{\"pruned\": {}}}", pruned);
        return Ok(());
    }

    eprintln!("nq index: found {} files", files.len());

    let options = aq_nlp::index::IndexOptions {
        cache,
        dry_run: false,
        force: cli.force,
    };

    let results = aq_nlp::index::index_files(&files, &options);
    aq_nlp::index::print_results(&results);
    aq_nlp::index::write_manifest(&results, &files, &options.cache);

    let (indexed, cached, stale, errors) = aq_nlp::index::summarize(&results);
    eprintln!(
        "nq index: {} indexed, {} cached, {} stale-pipeline, {} errors",
        indexed, cached, stale, errors
    );

    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// --corpus mode: unified multi-file narrative analysis
// ---------------------------------------------------------------------------

fn run_corpus_mode(cli: &Cli, budget_tracker: &mut BudgetTracker) -> anyhow::Result<()> {
    use aq_nlp::nq_cache::NqCache;

    let sources = collect_sources_no_query(cli)?;
    if sources.is_empty() {
        anyhow::bail!("--corpus: no input files provided");
    }

    let cache = NqCache::open()?;
    let backend = resolve_backend(cli.lang.as_deref());

    // Phase 1: parse each file (use per-file cache when available).
    let mut file_trees: Vec<(aq_core::OwnedNode, String)> = Vec::new();
    let mut file_hashes: Vec<(String, String)> = Vec::new();

    for (source, lang, file_path) in &sources {
        let content_hash = NqCache::content_hash(source);

        // Try Phase 1 cache.
        let tree = match cache.get(std::path::Path::new(file_path), &content_hash) {
            Ok(Some(t)) => t,
            _ => {
                let (t, _) = parse_source(&*backend, source, lang, file_path, false, false)?;
                let _ = cache.put(std::path::Path::new(file_path), &content_hash, &t);
                t
            }
        };
        file_trees.push((tree, file_path.clone()));
        file_hashes.push((file_path.clone(), content_hash));
    }

    // Order files according to --order flag.
    match cli.order.as_str() {
        "name" => file_trees.sort_by(|a, b| {
            let na = std::path::Path::new(&a.1).file_name().unwrap_or_default();
            let nb = std::path::Path::new(&b.1).file_name().unwrap_or_default();
            na.cmp(nb)
        }),
        "natural" => {}                                // preserve input order
        _ => file_trees.sort_by(|a, b| a.1.cmp(&b.1)), // "path" (default)
    }

    // Phase 2 cache check.
    let cache_pairs: Vec<(&str, &str)> = file_hashes
        .iter()
        .map(|(p, h)| (p.as_str(), h.as_str()))
        .collect();

    let (merged_tree, metadata) = match cache.get_merged(&cache_pairs) {
        Ok(Some((tree, meta))) => (tree, meta),
        _ => {
            let (tree, meta) = aq_nlp::corpus::build_corpus(file_trees);
            let _ = cache.put_merged(&cache_pairs, &tree, &meta);
            (tree, meta)
        }
    };

    // Dispatch: skeleton or query.
    if cli.skeleton {
        let output = format_corpus_skeleton(&merged_tree, &metadata, &cli.format);
        budget_tracker.emit(&output);
    } else if let Some(ref query_str) = cli.query {
        let tokens = aq_core::lex(query_str)
            .map_err(|e| anyhow::anyhow!("{}", format_lex_error(query_str, &e)))?;
        let expr = aq_core::parse(&tokens)
            .map_err(|e| anyhow::anyhow!("{}", format_parse_error(query_str, &e)))?;
        if cli.explain {
            eprintln!("{:#?}", expr);
            return Ok(());
        }
        let results = aq_core::eval(&expr, &merged_tree).map_err(|e| anyhow::anyhow!("{}", e))?;
        for result in &results {
            let output = format_result(result, &cli.format);
            budget_tracker.emit(&output);
        }
    } else {
        let output = serde_json::to_string_pretty(&merged_tree)
            .unwrap_or_else(|_| format!("{:?}", merged_tree));
        budget_tracker.emit(&output);
    }

    budget_tracker.print_truncation_metadata();
    Ok(())
}

/// Check if a conflict pair text contains only bare pronouns as entities.
fn is_pronoun_conflict_text(text: &str) -> bool {
    use aq_nlp::narrative::is_bare_pronoun_text;
    // Strip "Conflict: " prefix if present.
    let text = text.strip_prefix("Conflict: ").unwrap_or(text);
    // Try " ↔ " separator (per-file format), then " vs " (corpus format).
    let (left, right) = if let Some((l, r)) = text.split_once(" \u{2194} ") {
        (l.trim(), r.trim())
    } else if let Some((l, r)) = text.split_once(" vs ") {
        (l.trim(), r.trim())
    } else {
        return false;
    };
    // Strip trailing "(trend)" from right side.
    let right = right.split('(').next().unwrap_or(right).trim();
    is_bare_pronoun_text(left) && is_bare_pronoun_text(right)
}

/// Format a merged corpus skeleton with cross-file narrative summary.
fn format_corpus_skeleton(
    tree: &aq_core::OwnedNode,
    metadata: &aq_nlp::corpus::CorpusMetadata,
    format: &str,
) -> String {
    use std::collections::HashMap;

    let mut total_paragraphs = 0usize;
    let mut total_sentences = 0usize;
    let mut total_words = 0usize;
    let mut entity_counts: HashMap<String, usize> = HashMap::new();
    let mut scene_count = 0usize;
    let mut arc_distribution: HashMap<String, usize> = HashMap::new();
    let mut conflict_pairs: Vec<String> = Vec::new();
    let mut central_conflict = String::from("none");

    for child in &tree.children {
        match child.node_type.as_str() {
            "paragraph" => {
                total_paragraphs += 1;
                for sent in &child.children {
                    if sent.node_type == "sentence" {
                        total_sentences += 1;
                        for tok in &sent.children {
                            if tok.node_type == "token" {
                                total_words += 1;
                            }
                        }
                    }
                }
            }
            "entity" => {
                let name = child.text.as_deref().unwrap_or("unknown").to_string();
                let mentions = child
                    .field_indices
                    .get("locations")
                    .map(|v| v.len())
                    .unwrap_or(1);
                *entity_counts.entry(name).or_default() += mentions;
            }
            "scene" => {
                scene_count += 1;
            }
            "arc" => {
                let shape = child
                    .field_indices
                    .get("shape")
                    .and_then(|v| v.first())
                    .and_then(|&i| child.children.get(i))
                    .and_then(|n| n.text.as_deref())
                    .unwrap_or("unknown")
                    .to_string();
                *arc_distribution.entry(shape).or_default() += 1;
            }
            "conflict" => {
                if let Some(text) = child.text.as_deref() {
                    conflict_pairs.push(text.to_string());
                }
            }
            "narrative_summary" => {
                if let Some(text) = child.text.as_deref() {
                    for part in text.split_whitespace() {
                        if let Some(val) = part.strip_prefix("scenes=") {
                            scene_count = val.parse().unwrap_or(scene_count);
                        }
                    }
                }
                // Read grafted central_conflict from narrative_summary node.
                if let Some(v) = child
                    .field_indices
                    .get("central_conflict")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    if v != "none" {
                        central_conflict = v.to_string();
                    }
                }
            }
            _ => {}
        }
    }

    if central_conflict == "none" {
        // Filter out pronoun-only conflicts when selecting fallback.
        if let Some(first) = conflict_pairs
            .iter()
            .find(|pair| !is_pronoun_conflict_text(pair))
            .or_else(|| conflict_pairs.first())
        {
            central_conflict = first.clone();
        }
    }

    // Build characters list sorted by mention count.
    let mut characters: Vec<_> = entity_counts.into_iter().collect();
    characters.sort_by(|a, b| b.1.cmp(&a.1));
    let characters_json: Vec<serde_json::Value> = characters
        .iter()
        .map(|(name, mentions)| serde_json::json!({"name": name, "mentions": mentions}))
        .collect();

    let mut arc_dist_json = serde_json::Map::new();
    let mut sorted_arcs: Vec<_> = arc_distribution.into_iter().collect();
    sorted_arcs.sort_by_key(|(k, _)| k.clone());
    for (shape, count) in sorted_arcs {
        arc_dist_json.insert(shape, serde_json::json!(count));
    }

    let skeleton = serde_json::json!({
        "mode": "corpus",
        "files": metadata.files,
        "file_count": metadata.files.len(),
        "total_paragraphs": total_paragraphs,
        "total_sentences": total_sentences,
        "total_words": total_words,
        "scenes": scene_count,
        "characters": characters_json,
        "character_count": characters.len(),
        "central_conflict": central_conflict,
        "conflict_count": conflict_pairs.len(),
        "arc_distribution": arc_dist_json,
    });

    match format {
        "compact" => serde_json::to_string(&skeleton).unwrap_or_default(),
        _ => serde_json::to_string_pretty(&skeleton).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Glob expansion
// ---------------------------------------------------------------------------

/// Expand file paths, resolving glob patterns.
fn expand_file_paths(paths: &[String]) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    for path in paths {
        if path.contains('*') || path.contains('?') || path.contains('[') {
            let entries = glob::glob(path)
                .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", path, e))?;
            let mut matched = false;
            for entry in entries {
                let entry = entry.map_err(|e| anyhow::anyhow!("Glob error: {}", e))?;
                if entry.is_file() {
                    result.push(entry.to_string_lossy().into_owned());
                    matched = true;
                }
            }
            if !matched {
                anyhow::bail!("No files matched glob pattern: {}", path);
            }
        } else {
            result.push(path.clone());
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Source collection (files or stdin)
// ---------------------------------------------------------------------------

/// Map a file extension to a language name string understood by backends.
fn lang_from_path(path: &std::path::Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some("rust".into()),
        "py" | "pyi" => Some("python".into()),
        "js" | "mjs" | "cjs" | "jsx" => Some("javascript".into()),
        "ts" => Some("typescript".into()),
        "tsx" => Some("tsx".into()),
        "dart" => Some("dart".into()),
        "go" => Some("go".into()),
        "java" => Some("java".into()),
        "c" | "h" => Some("c".into()),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some("cpp".into()),
        "swift" => Some("swift".into()),
        "json" => Some("json".into()),
        "txt" | "text" => Some("english".into()),
        "md" | "markdown" => Some("english".into()),
        "rst" => Some("english".into()),
        "adoc" | "asciidoc" => Some("english".into()),
        _ => None,
    }
}

/// Check if we're running as the "nq" binary.
fn is_nq_binary() -> bool {
    std::env::args()
        .next()
        .and_then(|bin| {
            std::path::Path::new(&bin)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s == "nq")
        })
        .unwrap_or(false)
}

/// Resolve the backend to use based on the requested language or binary name.
fn resolve_backend(lang: Option<&str>) -> Box<dyn aq_core::Backend> {
    if let Some(lang) = lang {
        if lang == "english" {
            return Box::new(aq_nlp::NlpBackend);
        }
    }
    if let Some(bin_name) = std::env::args().next() {
        let name = std::path::Path::new(&bin_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if name == "nq" {
            return Box::new(aq_nlp::NlpBackend);
        }
    }
    Box::new(aq_treesitter::TreeSitterBackend)
}

/// Parse a source file using the resolved backend.
/// For tree-sitter, supports --all-nodes and --confidence flags.
/// For other backends, those flags are no-ops.
fn parse_source(
    backend: &dyn aq_core::Backend,
    source: &str,
    lang: &str,
    file_path: &str,
    all_nodes: bool,
    confidence: bool,
) -> anyhow::Result<(
    aq_core::OwnedNode,
    Option<aq_treesitter::parse::ParseMetrics>,
)> {
    // Tree-sitter-specific features (all_nodes, confidence) require direct ParsedTree access.
    if all_nodes || confidence {
        if let Some(ts_lang) = aq_treesitter::langs::Language::from_name(lang) {
            let parsed = aq_treesitter::parse::ParsedTree::parse(
                source.to_string(),
                ts_lang,
                Some(file_path.to_string()),
            )
            .map_err(|e| anyhow::anyhow!("{}", e))?;
            let metrics = if confidence {
                Some(parsed.metrics())
            } else {
                None
            };
            let root = if all_nodes {
                parsed.to_owned_node_all()
            } else {
                parsed.to_owned_node()
            };
            return Ok((root, metrics));
        }
    }
    // Default: use the backend trait.
    let root = backend
        .parse(source, lang, Some(file_path))
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok((root, None))
}

/// Collect sources for skeleton/signatures mode. In these modes, the `query` positional arg
/// is treated as the first file path (since no query is needed).
fn collect_sources_no_query(cli: &Cli) -> anyhow::Result<Vec<(String, String, String)>> {
    // Combine query (if it looks like a file path) and files
    let mut all_files: Vec<String> = Vec::new();
    if let Some(ref q) = cli.query {
        all_files.push(q.clone());
    }
    all_files.extend(cli.files.iter().cloned());

    if !all_files.is_empty() {
        let expanded = expand_file_paths(&all_files)?;
        let mut sources = Vec::new();
        for file_path in &expanded {
            let path = std::path::Path::new(file_path);
            let lang = if let Some(ref lang_str) = cli.lang {
                lang_str.clone()
            } else {
                lang_from_path(path).unwrap_or_else(|| {
                    if is_nq_binary() {
                        "english".to_string()
                    } else {
                        String::new()
                    }
                })
            };
            if lang.is_empty() {
                anyhow::bail!("Cannot detect language for: {}", file_path);
            }
            let source = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", file_path, e))?;
            sources.push((source, lang, file_path.clone()));
        }
        Ok(sources)
    } else if !io::stdin().is_terminal() {
        let lang_str =
            cli.lang
                .as_deref()
                .unwrap_or_else(|| if is_nq_binary() { "english" } else { "" });
        if lang_str.is_empty() {
            anyhow::bail!("--lang is required when reading from stdin");
        }
        let mut source = String::new();
        io::stdin().read_to_string(&mut source)?;
        Ok(vec![(source, lang_str.to_string(), "<stdin>".into())])
    } else {
        anyhow::bail!("No input files provided.");
    }
}

/// Collect sources from files or stdin. Returns (source_code, language_name, file_path).
fn collect_sources(cli: &Cli) -> anyhow::Result<Vec<(String, String, String)>> {
    let mut sources = Vec::new();

    if !cli.files.is_empty() {
        let expanded = expand_file_paths(&cli.files)?;
        for file_path in &expanded {
            let path = std::path::Path::new(file_path);
            let lang = if let Some(ref lang_str) = cli.lang {
                lang_str.clone()
            } else {
                lang_from_path(path).unwrap_or_else(|| {
                    if is_nq_binary() {
                        "english".to_string()
                    } else {
                        String::new()
                    }
                })
            };
            if lang.is_empty() {
                anyhow::bail!("Cannot detect language for: {}", file_path);
            }
            let source = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", file_path, e))?;
            sources.push((source, lang, file_path.clone()));
        }
    } else if !io::stdin().is_terminal() {
        // Read from stdin
        let lang_str =
            cli.lang
                .as_deref()
                .unwrap_or_else(|| if is_nq_binary() { "english" } else { "" });
        if lang_str.is_empty() {
            anyhow::bail!("--lang is required when reading from stdin");
        }
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|e| anyhow::anyhow!("Failed to read stdin: {}", e))?;
        sources.push((source, lang_str.to_string(), "<stdin>".into()));
    } else {
        anyhow::bail!("No input files provided. Pipe input via stdin or pass file paths.");
    }

    Ok(sources)
}

// ---------------------------------------------------------------------------
// --query-json: structured JSON queries for programmatic agent use
// ---------------------------------------------------------------------------

/// Compile a JSON query specification into an aq query string.
///
/// Supported fields:
/// - `traverse`: "desc" | "children" | "desc(N)" — traversal axis (default: "desc")
/// - `type`: string | [string] — node type filter(s) (required)
/// - `select`: string — additional filter expression
/// - `project`: string | [string] — fields to extract
/// - `limit`: number — max results
/// - `format`: "json" | "csv" | "text" — output format per result
///
/// Example:
/// ```json
/// {
///   "traverse": "desc",
///   "type": "function_item",
///   "project": [".name | @text", "@line", "@end - @start"],
///   "limit": 10
/// }
/// ```
/// Compiles to: `desc:function_item | [.name | @text, @line, @end - @start] | limit(10)`
fn compile_json_query(json_str: &str) -> anyhow::Result<String> {
    let spec: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| anyhow::anyhow!("Invalid JSON query: {}", e))?;

    let obj = spec
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("JSON query must be an object"))?;

    // Required: type
    let type_filter = match obj.get("type") {
        Some(serde_json::Value::String(t)) => t.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let types: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if types.len() == 1 {
                types[0].clone()
            } else {
                format!("({})", types.join(" | "))
            }
        }
        _ => anyhow::bail!("JSON query requires 'type' field (string or array of strings)"),
    };

    // Optional: traverse (default "desc")
    let traverse = obj
        .get("traverse")
        .and_then(|v| v.as_str())
        .unwrap_or("desc");

    // Build the base query: traverse:type
    let mut query = format!("{}:{}", traverse, type_filter);

    // Optional: select filter
    if let Some(serde_json::Value::String(sel)) = obj.get("select") {
        query = format!("{} | select({})", query, sel);
    }

    // Optional: project — determines output shape
    if let Some(proj) = obj.get("project") {
        match proj {
            serde_json::Value::String(field) => {
                query = format!("{} | {}", query, field);
            }
            serde_json::Value::Array(fields) => {
                let field_strs: Vec<String> = fields
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if field_strs.len() == 1 {
                    query = format!("{} | {}", query, field_strs[0]);
                } else {
                    // Build object with auto-generated keys or array
                    // Use array format: [field1, field2, ...]
                    query = format!("{} | [{}]", query, field_strs.join(", "));
                }
            }
            _ => {}
        }
    }

    // Optional: limit — wraps in array, limits, then iterates
    if let Some(limit) = obj.get("limit").and_then(|v| v.as_u64()) {
        query = format!("[{}] | limit({}) | .[]", query, limit);
    }

    Ok(query)
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn format_result(result: &aq_core::EvalResult, format: &str) -> String {
    match format {
        "text" => format_text(result),
        "compact" => format_compact(result),
        _ => format_json(result),
    }
}

fn format_json(result: &aq_core::EvalResult) -> String {
    let val = aq_core::result_to_json(result);
    serde_json::to_string_pretty(&val).unwrap_or_default()
}

fn format_compact(result: &aq_core::EvalResult) -> String {
    let val = aq_core::result_to_json(result);
    serde_json::to_string(&val).unwrap_or_default()
}

fn format_text(result: &aq_core::EvalResult) -> String {
    match result {
        aq_core::EvalResult::Node(n) => n.subtree_text().or(n.text()).unwrap_or("").to_string(),
        aq_core::EvalResult::Value(v) => match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Skeleton mode
// ---------------------------------------------------------------------------

fn metrics_to_json(m: &aq_treesitter::parse::ParseMetrics) -> serde_json::Value {
    serde_json::json!({
        "source_bytes": m.source_bytes,
        "total_nodes": m.total_nodes,
        "error_nodes": m.error_nodes,
        "missing_nodes": m.missing_nodes,
        "confidence": (m.confidence * 1000.0).round() / 1000.0,
    })
}

/// Extract the declaration name from a node, unwrapping wrapper types.
///
/// Handles:
/// - Direct `name` field (function_declaration, class_declaration, etc.)
/// - `export_statement` → unwrap to inner `declaration` field, then extract name
/// - `lexical_declaration` → find first `variable_declarator` child, get its `name`
fn extract_name(node: &aq_core::OwnedNode) -> Option<String> {
    use aq_core::AqNode;

    // 1. Direct name field — works for most declarations
    if let Some(name) = node
        .child_by_field("name")
        .and_then(|n| n.text().or(n.subtree_text()))
    {
        return Some(name.to_string());
    }

    let node_type = node.node_type.as_str();

    // 2. export_statement → unwrap via "declaration" field
    if node_type == "export_statement" {
        if let Some(decl) = node.child_by_field("declaration") {
            // Inner declaration should have a "name" field
            if let Some(name) = decl
                .child_by_field("name")
                .and_then(|n| n.text().or(n.subtree_text()))
            {
                return Some(name.to_string());
            }
            // Inner declaration might be lexical_declaration — recurse
            if decl.node_type() == "lexical_declaration" {
                for c in decl.named_children() {
                    if c.node_type() == "variable_declarator" {
                        if let Some(name) = c
                            .child_by_field("name")
                            .and_then(|n| n.text().or(n.subtree_text()))
                        {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
        return None;
    }

    // 3. lexical_declaration → first variable_declarator's name
    if node_type == "lexical_declaration" {
        for c in &node.children {
            if c.node_type == "variable_declarator" {
                if let Some(name) = c
                    .child_by_field("name")
                    .and_then(|n| n.text().or(n.subtree_text()))
                {
                    return Some(name.to_string());
                }
            }
        }
    }

    // 4. Original fallback: declarator field
    if let Some(name) = node
        .child_by_field("declarator")
        .and_then(|n| n.text().or(n.subtree_text()))
    {
        return Some(name.to_string());
    }

    None
}

/// NLP-specific skeleton: paragraph/sentence/word counts and entity stats.
fn format_nlp_skeleton(root: &aq_core::OwnedNode, file_path: &str, format: &str) -> String {
    use std::collections::HashMap;

    let mut paragraphs = 0;
    let mut sentences = 0;
    let mut word_count = 0;
    let mut entity_type_counts: HashMap<String, usize> = HashMap::new();
    let mut entity_details: Vec<(String, String, usize, usize)> = Vec::new(); // (name, type, mention_count, alias_count)
    let mut interaction_count = 0usize;
    let mut coref_chain_count = 0usize;
    let mut total_alias_count = 0usize;
    let mut coref_confidences: Vec<f64> = Vec::new();
    let mut passive_count = 0;
    let mut top_interactions: Vec<(String, String, String, bool)> = Vec::new(); // (agent, verb, patient, is_passive)
    let mut role_distribution: HashMap<String, usize> = HashMap::new();
    let mut verb_class_distribution: HashMap<String, usize> = HashMap::new();
    let mut role_confidences: Vec<f64> = Vec::new();
    let mut unclassified_verb_count = 0usize;
    let mut discourse_count = 0usize;
    let mut discourse_relation_dist: HashMap<String, usize> = HashMap::new();
    let mut discourse_connective_count = 0usize;
    let mut discourse_cross_para_count = 0usize;
    let mut discourse_confidences: Vec<f64> = Vec::new();
    let mut narrative_scene_count = 0usize;
    let mut narrative_character_count = 0usize;
    let mut narrative_conflict_count = 0usize;
    let mut narrative_central_conflict = String::from("none");
    let mut narrative_issue_count = 0usize;
    let mut narrative_unresolved = 0usize;
    let mut narrative_arc_dist = serde_json::Map::new();

    for child in &root.children {
        match child.node_type.as_str() {
            "paragraph" => {
                paragraphs += 1;
                for sent in &child.children {
                    if sent.node_type == "sentence" {
                        sentences += 1;
                        for token in &sent.children {
                            if token.node_type == "token" {
                                word_count += 1;
                            }
                        }
                    }
                }
            }
            "entity" => {
                let entity_name = child.text.as_deref().unwrap_or("unknown").to_string();
                let entity_type = child
                    .field_indices
                    .get("type")
                    .and_then(|indices| indices.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                    .unwrap_or("UNKNOWN")
                    .to_string();

                // Count location children as mention count
                let mention_count = child
                    .field_indices
                    .get("locations")
                    .map(|indices| indices.len())
                    .unwrap_or(1);

                // Check for coref data
                let has_aliases = child.field_indices.contains_key("aliases");
                let mut alias_count = 0usize;
                if has_aliases {
                    coref_chain_count += 1;
                    if let Some(aliases_indices) = child.field_indices.get("aliases") {
                        if let Some(&aliases_idx) = aliases_indices.first() {
                            if let Some(aliases_node) = child.children.get(aliases_idx) {
                                alias_count = aliases_node.children.len();
                                total_alias_count += alias_count;
                            }
                        }
                    }
                    if let Some(conf_indices) = child.field_indices.get("avg_confidence") {
                        if let Some(&conf_idx) = conf_indices.first() {
                            if let Some(conf_node) = child.children.get(conf_idx) {
                                if let Some(conf_text) = conf_node.text.as_deref() {
                                    if let Ok(conf) = conf_text.parse::<f64>() {
                                        coref_confidences.push(conf);
                                    }
                                }
                            }
                        }
                    }
                }

                *entity_type_counts.entry(entity_type.clone()).or_insert(0) += 1;
                entity_details.push((entity_name, entity_type, mention_count, alias_count));
            }
            "interaction" => {
                interaction_count += 1;
                let agent = child
                    .field_indices
                    .get("agent")
                    .and_then(|indices| indices.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                    .unwrap_or("")
                    .to_string();
                let verb = child
                    .field_indices
                    .get("verb")
                    .and_then(|indices| indices.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                    .unwrap_or("")
                    .to_string();
                let patient = child
                    .field_indices
                    .get("patient")
                    .and_then(|indices| indices.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                    .unwrap_or("")
                    .to_string();
                let voice = child
                    .field_indices
                    .get("voice")
                    .and_then(|indices| indices.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                    .unwrap_or("active");
                let is_passive = voice == "passive";
                if is_passive {
                    passive_count += 1;
                }
                top_interactions.push((agent, verb, patient, is_passive));
                // Role stats
                if let Some(vc) = child
                    .field_indices
                    .get("verb_class")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    if vc == "unknown" {
                        unclassified_verb_count += 1;
                    } else {
                        *verb_class_distribution.entry(vc.to_string()).or_insert(0) += 1;
                    }
                }
                for role_child in &child.children {
                    if role_child.node_type == "role" {
                        if let Some(role_text) = role_child.text.as_deref() {
                            *role_distribution.entry(role_text.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                if let Some(conf_text) = child
                    .field_indices
                    .get("role_confidence")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    if let Ok(conf) = conf_text.parse::<f64>() {
                        role_confidences.push(conf);
                    }
                }
            }
            "discourse" => {
                discourse_count += 1;
                // Read type
                if let Some(type_text) = child
                    .field_indices
                    .get("type")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    *discourse_relation_dist
                        .entry(type_text.to_string())
                        .or_default() += 1;
                }
                // Read connective
                if child.field_indices.contains_key("connective") {
                    discourse_connective_count += 1;
                }
                // Read scope
                if let Some(scope_text) = child
                    .field_indices
                    .get("scope")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    if scope_text == "cross_paragraph" {
                        discourse_cross_para_count += 1;
                    }
                }
                // Read confidence
                if let Some(conf_text) = child
                    .field_indices
                    .get("confidence")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    if let Ok(conf) = conf_text.parse::<f64>() {
                        discourse_confidences.push(conf);
                    }
                }
            }
            "narrative_summary" => {
                if let Some(v) = child
                    .field_indices
                    .get("scene_count")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    narrative_scene_count = v.parse().unwrap_or(0);
                }
                if let Some(v) = child
                    .field_indices
                    .get("character_count")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    narrative_character_count = v.parse().unwrap_or(0);
                }
                if let Some(v) = child
                    .field_indices
                    .get("conflict_count")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    narrative_conflict_count = v.parse().unwrap_or(0);
                }
                if let Some(v) = child
                    .field_indices
                    .get("central_conflict")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    narrative_central_conflict = v.to_string();
                }
                if let Some(v) = child
                    .field_indices
                    .get("issue_count")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    narrative_issue_count = v.parse().unwrap_or(0);
                }
                if let Some(v) = child
                    .field_indices
                    .get("unresolved_conflicts")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    narrative_unresolved = v.parse().unwrap_or(0);
                }
                if let Some(v) = child
                    .field_indices
                    .get("arc_distribution")
                    .and_then(|i| i.first())
                    .and_then(|&idx| child.children.get(idx))
                    .and_then(|n| n.text.as_deref())
                {
                    for part in v.split(", ") {
                        if let Some((key, val)) = part.split_once(':') {
                            if let Ok(n) = val.parse::<usize>() {
                                narrative_arc_dist.insert(key.to_string(), serde_json::json!(n));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Sort entity_details by mention count descending
    entity_details.sort_by(|a, b| b.2.cmp(&a.2));

    // Build top_entities (top 10)
    let top_entities: Vec<serde_json::Value> = entity_details
        .iter()
        .take(10)
        .map(|(name, etype, mentions, aliases)| {
            let mut obj = serde_json::json!({
                "name": name,
                "type": etype,
                "mentions": mentions,
            });
            if *aliases > 0 {
                obj["aliases"] = serde_json::json!(aliases);
            }
            obj
        })
        .collect();

    // Build entity type counts as a sorted map
    let mut entity_types = serde_json::Map::new();
    let mut sorted_types: Vec<_> = entity_type_counts.into_iter().collect();
    sorted_types.sort_by_key(|(k, _)| k.clone());
    for (etype, count) in sorted_types {
        entity_types.insert(etype, serde_json::json!(count));
    }

    // Build top_interactions JSON array (top 10)
    let top_interactions_json: Vec<serde_json::Value> = top_interactions
        .iter()
        .take(10)
        .map(|(agent, verb, patient, is_passive)| {
            serde_json::json!({
                "agent": agent,
                "verb": verb,
                "patient": patient,
                "passive": is_passive,
            })
        })
        .collect();

    // Build role_stats
    let mut role_dist = serde_json::Map::new();
    let mut sorted_roles: Vec<_> = role_distribution.into_iter().collect();
    sorted_roles.sort_by_key(|(k, _)| k.clone());
    for (role, count) in sorted_roles {
        role_dist.insert(role, serde_json::json!(count));
    }
    let mut vc_dist = serde_json::Map::new();
    let mut sorted_vcs: Vec<_> = verb_class_distribution.into_iter().collect();
    sorted_vcs.sort_by_key(|(k, _)| k.clone());
    for (vc, count) in sorted_vcs {
        vc_dist.insert(vc, serde_json::json!(count));
    }
    let classified = interaction_count.saturating_sub(unclassified_verb_count);
    let classified_pct = if interaction_count > 0 {
        (classified as f64 / interaction_count as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };
    let avg_conf = if role_confidences.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(
            (role_confidences.iter().sum::<f64>() / role_confidences.len() as f64 * 100.0).round()
                / 100.0
        )
    };

    // Build discourse_stats
    let mut disc_rel_dist = serde_json::Map::new();
    let mut sorted_disc_rels: Vec<_> = discourse_relation_dist.into_iter().collect();
    sorted_disc_rels.sort_by_key(|(k, _)| k.clone());
    for (rel, count) in sorted_disc_rels {
        disc_rel_dist.insert(rel, serde_json::json!(count));
    }
    let avg_discourse_conf = if discourse_confidences.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(
            (discourse_confidences.iter().sum::<f64>() / discourse_confidences.len() as f64
                * 100.0)
                .round()
                / 100.0
        )
    };

    let skeleton = serde_json::json!({
        "file": file_path,
        "paragraphs": paragraphs,
        "sentences": sentences,
        "word_count": word_count,
        "entities": entity_types,
        "top_entities": top_entities,
        "interaction_count": interaction_count,
        "passive_count": passive_count,
        "top_interactions": top_interactions_json,
        "coref_chain_count": coref_chain_count,
        "total_alias_count": total_alias_count,
        "avg_coref_confidence": if coref_confidences.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!((coref_confidences.iter().sum::<f64>() / coref_confidences.len() as f64 * 100.0).round() / 100.0)
        },
        "role_stats": {
            "role_distribution": role_dist,
            "verb_class_distribution": vc_dist,
            "unclassified_verb_count": unclassified_verb_count,
            "avg_role_confidence": avg_conf,
            "classified_interaction_pct": classified_pct,
        },
        "discourse_stats": {
            "relation_count": discourse_count,
            "relation_distribution": disc_rel_dist,
            "connective_count": discourse_connective_count,
            "cross_paragraph_count": discourse_cross_para_count,
            "avg_discourse_confidence": avg_discourse_conf,
        },
        "narrative_stats": {
            "scene_count": narrative_scene_count,
            "character_count": narrative_character_count,
            "arc_shape_distribution": narrative_arc_dist,
            "conflict_count": narrative_conflict_count,
            "central_conflict": narrative_central_conflict,
            "issue_count": narrative_issue_count,
            "unresolved_conflicts": narrative_unresolved,
        },
    });

    match format {
        "compact" => serde_json::to_string(&skeleton).unwrap_or_default(),
        _ => serde_json::to_string_pretty(&skeleton).unwrap_or_default(),
    }
}

fn format_skeleton(
    root: &aq_core::OwnedNode,
    file_path: &str,
    format: &str,
    metrics: Option<&aq_treesitter::parse::ParseMetrics>,
) -> String {
    use aq_core::AqNode;

    let mut imports = Vec::new();
    let mut declarations = Vec::new();

    for child in &root.children {
        let node_type = child.node_type.as_str();

        // Skip comments and attributes — they're noise in a skeleton
        if is_noise_type(node_type) {
            continue;
        }

        if is_import_type(node_type) {
            let text = child
                .subtree_text
                .as_deref()
                .or(child.text.as_deref())
                .unwrap_or("")
                .trim();
            imports.push(serde_json::json!(text));
        } else {
            // Declaration: extract metadata
            let name = extract_name(child)
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null);

            let lines = child.end_line.saturating_sub(child.start_line) + 1;

            let members = child
                .child_by_field("body")
                .map(|body| body.named_children().len())
                .unwrap_or(0);

            let mut decl = serde_json::Map::new();
            decl.insert("type".into(), serde_json::json!(node_type));
            decl.insert("name".into(), name);
            decl.insert("line".into(), serde_json::json!(child.start_line));
            decl.insert("end_line".into(), serde_json::json!(child.end_line));
            decl.insert("lines".into(), serde_json::json!(lines));
            if members > 0 {
                decl.insert("members".into(), serde_json::json!(members));
            }
            declarations.push(serde_json::Value::Object(decl));
        }
    }

    let mut skeleton = serde_json::json!({
        "file": file_path,
        "imports": imports,
        "declarations": declarations,
    });

    if let Some(m) = metrics {
        skeleton["_confidence"] = metrics_to_json(m);
    }

    match format {
        "compact" => serde_json::to_string(&skeleton).unwrap_or_default(),
        _ => serde_json::to_string_pretty(&skeleton).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Signatures mode
// ---------------------------------------------------------------------------

fn format_signatures(
    root: &aq_core::OwnedNode,
    file_path: &str,
    format: &str,
    metrics: Option<&aq_treesitter::parse::ParseMetrics>,
) -> String {
    let mut sigs = Vec::new();
    collect_signatures(root, &mut sigs);

    let mut output = serde_json::json!({
        "file": file_path,
        "signatures": sigs,
    });

    if let Some(m) = metrics {
        output["_confidence"] = metrics_to_json(m);
    }

    match format {
        "compact" => serde_json::to_string(&output).unwrap_or_default(),
        _ => serde_json::to_string_pretty(&output).unwrap_or_default(),
    }
}

fn collect_signatures(node: &aq_core::OwnedNode, sigs: &mut Vec<serde_json::Value>) {
    use aq_core::AqNode;

    let node_type = node.node_type.as_str();

    if is_function_type(node_type) || is_method_type(node_type) {
        let name = node
            .child_by_field("name")
            .or_else(|| find_child_by_type(node, "identifier"))
            .and_then(|n| n.text().or(n.subtree_text()))
            .map(String::from)
            .or_else(|| {
                // Operators: extract from binary_operator / unary_operator child
                find_child_by_type(node, "binary_operator")
                    .or_else(|| find_child_by_type(node, "unary_operator"))
                    .and_then(|n| n.text().or(n.subtree_text()))
                    .map(|s| format!("operator {s}"))
            })
            .or_else(|| {
                // Last resort: first token before '(' in subtree text
                node.subtree_text()
                    .and_then(|t| t.split('(').next())
                    .and_then(|t| t.split_whitespace().last())
                    .map(String::from)
            });

        let params = node
            .child_by_field("parameters")
            .or_else(|| find_child_by_type(node, "formal_parameter_list"))
            .map(|p| p.subtree_text().or(p.text()).unwrap_or("").to_string());

        let return_type = node
            .child_by_field("return_type")
            .or_else(|| find_child_by_type(node, "type_identifier"))
            .or_else(|| find_child_by_type(node, "void_type"))
            .or_else(|| find_child_by_type(node, "function_type"))
            .and_then(|r| r.subtree_text().or(r.text()))
            .map(String::from);

        let lines = node.end_line.saturating_sub(node.start_line) + 1;

        let mut sig = serde_json::Map::new();
        sig.insert("type".into(), serde_json::json!(node_type));
        sig.insert(
            "name".into(),
            serde_json::json!(name.unwrap_or_else(|| "<anonymous>".to_string())),
        );
        if let Some(p) = params {
            sig.insert("params".into(), serde_json::json!(p));
        }
        if let Some(r) = return_type {
            sig.insert("return_type".into(), serde_json::json!(r));
        }
        sig.insert("line".into(), serde_json::json!(node.start_line));
        sig.insert("lines".into(), serde_json::json!(lines));
        sigs.push(serde_json::Value::Object(sig));
    }

    // Recurse into children to find nested functions/methods
    for child in &node.children {
        collect_signatures(child, sigs);
    }
}

fn find_child_by_type<'a>(
    node: &'a aq_core::OwnedNode,
    type_name: &str,
) -> Option<&'a dyn aq_core::AqNode> {
    node.children
        .iter()
        .find(|c| c.node_type == type_name)
        .map(|c| c as &dyn aq_core::AqNode)
}

fn is_function_type(t: &str) -> bool {
    t.contains("function")
        && (t.contains("declaration")
            || t.contains("definition")
            || t.contains("item")
            || t.contains("signature"))
}

fn is_method_type(t: &str) -> bool {
    // Dart's method_signature is a structural wrapper — skip it to avoid
    // duplicates with no fields; its specific children (getter_signature,
    // setter_signature, function_signature, etc.) are matched instead.
    if t == "method_signature" {
        return false;
    }
    let suffix = t.contains("declaration")
        || t.contains("definition")
        || t.contains("item")
        || t.contains("signature");
    (t.contains("method")
        || t.contains("constructor")
        || t.contains("getter")
        || t.contains("setter")
        || t.contains("operator"))
        && suffix
}

fn is_noise_type(node_type: &str) -> bool {
    node_type.contains("comment")
        || node_type == "attribute_item"
        || node_type == "decorator"
        || node_type == "expression_statement" // top-level expressions are usually noise
}

fn is_import_type(node_type: &str) -> bool {
    node_type.contains("import")
        || node_type == "use_declaration"
        || node_type == "use_item"
        || node_type.contains("include")
        || node_type.contains("require")
}

// ---------------------------------------------------------------------------
// Enhanced error formatting
// ---------------------------------------------------------------------------

fn format_lex_error(query: &str, err: &aq_core::LexError) -> String {
    format_query_error(query, err.position, &err.message, "Lex")
}

fn format_parse_error(query: &str, err: &aq_core::ParseError) -> String {
    format_query_error(query, err.position, &err.message, "Parse")
}

/// Format a query error with position context showing the problematic location.
///
/// Output format (agent-parseable):
/// ```
/// Parse error at position 15: Expected ')', got Eof
///   desc:function | select(.name
///                          ^^^^^
/// ```
fn format_query_error(query: &str, position: usize, message: &str, kind: &str) -> String {
    let mut out = format!("{} error at position {}: {}", kind, position, message);

    if !query.is_empty() {
        // Find the line containing the error position
        let (line_start, line_end) = find_line_bounds(query, position);
        let line_text = &query[line_start..line_end];

        // Position within the line
        let col = position.saturating_sub(line_start);

        out.push_str(&format!("\n  {}\n  ", line_text));

        // Add caret pointing at the error position
        for _ in 0..col {
            out.push(' ');
        }
        // Show caret(s) — extend to end of current token or a few chars
        let remaining = line_end.saturating_sub(position);
        let caret_len = remaining.clamp(1, 5);
        for _ in 0..caret_len {
            out.push('^');
        }
    }

    out
}

fn find_line_bounds(s: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(s.len());
    let start = s[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = s[pos..].find('\n').map(|i| pos + i).unwrap_or(s.len());
    (start, end)
}

// ---------------------------------------------------------------------------
// Budget tracking
// ---------------------------------------------------------------------------

struct BudgetTracker {
    budget: Option<usize>,
    remaining: Option<usize>,
    total: usize,
    shown: usize,
}

impl BudgetTracker {
    fn new(budget: Option<usize>) -> Self {
        Self {
            budget,
            remaining: budget,
            total: 0,
            shown: 0,
        }
    }

    /// Emit a formatted result string, respecting the budget.
    fn emit(&mut self, output: &str) {
        self.total += 1;
        let tokens = estimate_tokens(output);

        if let Some(ref mut remaining) = self.remaining {
            if tokens > *remaining {
                // Over budget — count but don't print
                return;
            }
            *remaining = remaining.saturating_sub(tokens);
        }

        self.shown += 1;
        if !output.is_empty() {
            println!("{}", output);
        }
    }

    /// Print truncation metadata if budget was active and something was cut.
    fn print_truncation_metadata(&self) {
        if self.budget.is_some() && self.total > self.shown {
            let meta = serde_json::json!({
                "_truncated": true,
                "_total": self.total,
                "_shown": self.shown,
            });
            println!("{}", serde_json::to_string(&meta).unwrap_or_default());
        }
    }
}

/// Estimate token count for a string (simple heuristic: ~4 chars per token).
fn estimate_tokens(s: &str) -> usize {
    s.len().div_ceil(4) // ceiling division
}
