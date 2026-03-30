use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "mq",
    about = "Semantic query tool for the Agent Perception Layer"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Embed and index JSON items from stdin into a collection
    Index {
        /// Collection name
        #[arg(long)]
        collection: String,
        /// jq-style key expression to extract item key from JSON
        #[arg(long)]
        key: String,
        /// jq-style text expression to extract text to embed (defaults to full JSON stringification)
        #[arg(long)]
        text: Option<String>,
        /// Embedding model to use
        #[arg(long, default_value = "bge-small")]
        model: String,
        /// Upsert: update existing entries instead of erroring on duplicates
        #[arg(long)]
        upsert: bool,
    },
    /// Semantic similarity search against a collection
    Search {
        /// The query text to search for
        query: String,
        /// Collection to search
        #[arg(long)]
        collection: String,
        /// Number of results to return
        #[arg(long, short, default_value = "5")]
        k: usize,
        /// Minimum similarity threshold (0.0–1.0)
        #[arg(long, default_value = "0.0")]
        threshold: f32,
        /// Embedding model (must match collection's model)
        #[arg(long, default_value = "bge-small")]
        model: String,
    },
    /// Collection statistics
    Stats {
        /// Collection name
        #[arg(long)]
        collection: String,
    },
    /// Remove stale entries by key (keys read from stdin, one per line)
    Invalidate {
        /// Collection name
        #[arg(long)]
        collection: String,
    },
    /// Fuzzy join: match items from two JSON sources by semantic similarity
    Match {
        /// Path to left JSON file (or - for stdin)
        left: String,
        /// Path to right JSON file
        right: String,
        /// jq-style key expression for left items
        #[arg(long)]
        left_key: String,
        /// jq-style key expression for right items
        #[arg(long)]
        right_key: String,
        /// Minimum similarity threshold (0.0–1.0)
        #[arg(long, default_value = "0.75")]
        threshold: f32,
        /// Embedding model to use
        #[arg(long, default_value = "bge-small")]
        model: String,
    },
    /// Find items semantically similar to a known item in a collection
    Similar {
        /// Key of the item to find similar items for
        key: String,
        /// Collection to search
        #[arg(long)]
        collection: String,
        /// Number of results to return
        #[arg(long, short, default_value = "5")]
        k: usize,
        /// Minimum similarity threshold (0.0–1.0)
        #[arg(long, default_value = "0.0")]
        threshold: f32,
    },
    /// Cross-collection semantic correlation: find related items between two collections
    Relate {
        /// Left collection name
        left: String,
        /// Right collection name
        right: String,
        /// Number of top matches per left item
        #[arg(long, short, default_value = "3")]
        k: usize,
        /// Minimum similarity threshold (0.0–1.0)
        #[arg(long, default_value = "0.5")]
        threshold: f32,
    },
    /// Classify collection items against predefined categories by semantic similarity
    Classify {
        /// Collection to classify
        #[arg(long)]
        collection: String,
        /// Comma-separated category labels
        #[arg(long)]
        categories: String,
        /// Minimum similarity threshold (0.0–1.0)
        #[arg(long, default_value = "0.3")]
        threshold: f32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Index {
            collection,
            key,
            text,
            model,
            upsert,
        } => commands::index(&collection, &key, text.as_deref(), &model, upsert),
        Command::Search {
            query,
            collection,
            k,
            threshold,
            model,
        } => commands::search(&query, &collection, k, threshold, &model),
        Command::Stats { collection } => commands::stats(&collection),
        Command::Invalidate { collection } => commands::invalidate(&collection),
        Command::Match {
            left,
            right,
            left_key,
            right_key,
            threshold,
            model,
        } => commands::match_cmd(&left, &right, &left_key, &right_key, threshold, &model),
        Command::Similar {
            key,
            collection,
            k,
            threshold,
        } => commands::similar(&key, &collection, k, threshold),
        Command::Relate {
            left,
            right,
            k,
            threshold,
        } => commands::relate(&left, &right, k, threshold),
        Command::Classify {
            collection,
            categories,
            threshold,
        } => commands::classify(&collection, &categories, threshold),
    }
}
