# Contributing to percept

Thank you for your interest in contributing. A few things to know upfront.

## Design principles

Every tool in this repo exists to serve one idea: **agents should query structured data, not read raw files**. Before a PR is opened, it's worth asking whether the change serves that principle — smaller token footprint, more composable output, better pipeline fit.

Each tool has a clear boundary. `aq` is for code structure. `gq` is for git history. `lq` is for logs. `tq` is for test results. `mq` is for semantic similarity. `oq` is the cache layer. `nq` is for natural language. `cq` is the correlation layer that joins them. PRs that blur these boundaries will generally be declined — not because the idea is bad, but because scope creep is how good tools become mediocre ones.

## Getting started

```bash
git clone https://github.com/PhilipHayes/percept
cd percept
cargo build --workspace --exclude mq-embed --exclude mq-store --exclude mq-cli
cargo test --workspace --exclude mq-embed --exclude mq-store --exclude mq-cli
```

`mq` requires an ONNX runtime for local embedding inference. To build and test it:

```bash
FASTEMBED_CACHE_DIR=~/.cache/fastembed cargo test -p mq-embed -p mq-store -p mq-cli
```

## What makes a good PR

- A failing test that demonstrates the bug, then a fix that makes it pass
- New behaviour documented with an example in the tool's README
- No new dependencies without a clear reason — the tools are intentionally lean
- Output format changes are breaking changes; treat them that way

## Code style

`cargo fmt` and `cargo clippy -- -D warnings` must both pass. CI enforces this.

## `cq` is a bash script

`cq` is intentionally a bash script powered by `jq`. It does not need to become a Rust binary. Contributions that keep it lean and composable are welcome; contributions that add heavy logic or new dependencies are not.
