# percept

**Structured query tools for AI agents.**

An AI agent reading a 300-line source file consumes ~8,000 tokens. An agent running `aq --skeleton src/main.rs` consumes ~400. Percept is a family of `*q` CLI tools that give agents structured, composable, token-efficient perception of codebases, git history, logs, test results, and more — all as JSON pipelines.

```bash
# What changed recently, and did it break anything?
gq --changed-since '3 days' | jq '.[].file' | \
  xargs -I{} aq --skeleton {} | \
  cargo test 2>&1 | tq --summary

# Find code not covered by documentation
cq join <(aq --skeleton src/) <(mdq '# API' README.md | jq -R '[{name:.}]') \
  -l '.name' -r '.name' -t anti

# Match bills to transactions by semantic similarity
mq match bills.json transactions.json \
  --left-key '.payee' --right-key '.merchant' --threshold 0.8
```

## Tools

| Tool | What it queries | Output |
|------|----------------|--------|
| [`aq`](crates/aq-cli/) | Source code AST (15 languages) | Functions, classes, signatures, skeletons |
| [`nq`](crates/aq-nlp/) | Natural language text | Entities, relations, narrative structure |
| [`gq`](crates/gq/) | Git history | Commits, blame, churn, changed files |
| [`oq`](crates/oq/) | Observation cache | Cached `aq` results, git-hash invalidated |
| [`lq`](crates/lq-cli/) | Log files | Filtered events, patterns, aggregations |
| [`tq`](crates/tq-cli/) | Test output | Pass/fail summaries, flaky detection, diffs |
| [`mq`](crates/mq-cli/) | Semantic similarity | Vector search, fuzzy match, classification |
| [`cq`](tools/cq) | JSON streams | Joins, lookups, diffs, grouping across tools |

All tools output JSON. All tools compose with `jq`. All tools are designed to fit inside an agent's context budget.

## Install

```bash
cargo install --git https://github.com/PhilipHayes/percept aq
cargo install --git https://github.com/PhilipHayes/percept gq
cargo install --git https://github.com/PhilipHayes/percept oq
cargo install --git https://github.com/PhilipHayes/percept lq
cargo install --git https://github.com/PhilipHayes/percept tq
cargo install --git https://github.com/PhilipHayes/percept mq

# cq is a bash script — requires jq
cp tools/cq /usr/local/bin/cq && chmod +x /usr/local/bin/cq
```

## Why query, not read?

When an agent reads a file, it consumes all of it — structure, comments, whitespace, irrelevant sections — and spends tokens reasoning about what's relevant. When it queries, it gets exactly what it asked for, structured for downstream use.

The tools in percept apply this principle to every substrate an agent touches:

- **Code** → `aq` extracts AST nodes. No more reading entire files to find a function signature.
- **Git** → `gq` structures history as JSON. No more parsing `git log` text.
- **Logs** → `lq` filters and aggregates. No more grepping through raw output.
- **Tests** → `tq` summarises results. No more reading test runner output line by line.
- **Semantics** → `mq` does vector similarity locally. No API calls, no rate limits.
- **Correlation** → `cq` joins outputs across tools. The glue layer.

## Benchmarks

See [`benchmarks/`](benchmarks/) for detailed comparison data. Summary: across a representative codebase query task, token consumption drops 90–95% compared to file reading, with no loss of task accuracy.

## Language support (aq)

Rust, TypeScript, JavaScript, Python, Go, Java, C, C++, Dart, Swift, JSON, TSX.

## License

MIT
