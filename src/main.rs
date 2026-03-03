mod client;
mod config;
mod daemon;
mod daemon_state;
mod feedback;
mod graph;
mod handler;
mod index;
mod name_index;
mod node;
mod protocol;
mod recall;
mod search;
mod util;
mod wal;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "memcore", version, about = "Non-parametric memory core for AI agents")]
struct Cli {
    /// Suppress success messages (errors still print to stderr)
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize memcore directory structure and default model
    Init {
        #[arg(long)]
        dir: Option<String>,
    },

    /// Create a new memory node
    Create {
        /// Node name (letters, digits, spaces, hyphens; 2-128 chars; starts with letter)
        name: String,
        /// Read content from file instead of stdin
        #[arg(short)]
        f: Option<String>,
    },

    /// Read one or more nodes
    Get {
        /// Node name(s)
        names: Vec<String>,
    },

    /// Full replacement update of an existing node
    Update {
        /// Node name
        name: String,
        /// Read content from file instead of stdin
        #[arg(short)]
        f: Option<String>,
    },

    /// Patch a node's body (local modification)
    Patch {
        /// Node name
        name: String,
        /// Replace old text with new text
        #[arg(long, num_args = 2)]
        replace: Option<Vec<String>>,
        /// Append text to body
        #[arg(long)]
        append: Option<String>,
        /// Prepend text to body
        #[arg(long)]
        prepend: Option<String>,
    },

    /// Delete a memory node (irreversible)
    Delete {
        /// Node name
        name: String,
    },

    /// Rename a node (preserves all metadata)
    Rename {
        /// Current node name
        old: String,
        /// New node name
        new: String,
    },

    /// List all nodes
    Ls {
        #[arg(long, default_value = "name")]
        sort: String,
    },

    /// System diagnostics (global health or cluster scan)
    Inspect {
        /// Optional node name for cluster-level diagnosis
        node: Option<String>,
        #[arg(long, default_value = "human")]
        format: String,
        /// Similarity threshold for finding similar pairs (0.0–1.0, default 0.85)
        #[arg(long, short = 't')]
        threshold: Option<f32>,
        /// Max number of similar pairs to display (default 50)
        #[arg(long, default_value_t = 50)]
        cap: usize,
    },

    /// Comprehensive memory retrieval (vector + graph + scoring)
    Recall {
        /// Semantic query (omit for working memory snapshot)
        query: Option<String>,
        #[arg(long, default_value_t = 5)]
        top_k: usize,
        #[arg(long, default_value_t = 1)]
        depth: usize,
        /// Name prefix search mode
        #[arg(long)]
        name: Option<String>,
    },

    /// Pure vector similarity search
    Search {
        /// Query text
        query: String,
        #[arg(long, default_value_t = 5)]
        top_k: usize,
    },

    /// Multi-term semantic vector search (wider recall via multiple queries)
    MultiSearch {
        /// Query texts (one or more)
        #[arg(required = true, num_args = 1..)]
        queries: Vec<String>,
        /// Max results per query term
        #[arg(long, default_value_t = 5)]
        top_k: usize,
    },

    /// Multi-term comprehensive retrieval (vector + graph + scoring)
    MultiRecall {
        /// Query texts (one or more)
        #[arg(required = true, num_args = 1..)]
        queries: Vec<String>,
        /// Max results per query term
        #[arg(long, default_value_t = 5)]
        top_k: usize,
        #[arg(long, default_value_t = 1)]
        depth: usize,
    },

    /// Show neighbors of a node via graph traversal
    Neighbors {
        /// Node name
        name: String,
        #[arg(long, default_value_t = 1)]
        depth: usize,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },

    /// Create a bidirectional edge between two nodes
    Link {
        a: String,
        b: String,
    },

    /// Remove a bidirectional edge between two nodes
    Unlink {
        a: String,
        b: String,
    },

    /// Positive feedback — memory helped correct judgment
    Boost {
        /// Node name
        name: String,
    },

    /// Negative feedback — memory led to incorrect judgment
    Penalize {
        /// Node name
        name: String,
    },

    /// Mark node as core long-term memory
    Pin {
        /// Node name
        name: String,
    },

    /// Remove core memory marker
    Unpin {
        /// Node name
        name: String,
    },

    /// Show system status
    Status,

    /// Rebuild all indices from .md files
    Reindex,

    /// Clean dangling references
    Gc,

    /// Compute similarity distribution baseline
    Baseline,

    /// Gracefully stop the daemon
    Stop,

    /// Run as daemon (internal, not user-facing)
    #[command(hide = true)]
    Daemon,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon => {
            daemon::run().await
        }
        other => {
            client::dispatch(other, cli.quiet).await
        }
    }
}
