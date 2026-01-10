//! CLI definitions for xf.
//!
//! Uses clap for argument parsing with derive macros.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// xf - Ultra-fast Twitter/X archive search
#[derive(Parser, Debug)]
#[command(name = "xf")]
#[command(author = "Jeffrey Emanuel <jeff@jeffreyemanuel.dev>")]
#[command(version = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n  Built: ", env!("VERGEN_BUILD_TIMESTAMP"),
    "\n  Rustc: ", env!("VERGEN_RUSTC_SEMVER"),
    "\n  Target: ", env!("VERGEN_CARGO_TARGET_TRIPLE"),
))]
#[command(about = "Ultra-fast CLI for searching Twitter/X data archives")]
#[command(long_about = r#"
xf (x_find) - A blazingly fast command-line tool for indexing and searching
your Twitter/X data archive.

Features:
  - Full-text search with BM25 ranking
  - Search tweets, likes, DMs, and Grok chats
  - Sub-millisecond query latency via Tantivy
  - SQLite storage for metadata queries
  - JSON and human-readable output formats

Quick start:
  1. Download your Twitter data from twitter.com/settings
  2. Run: xf index /path/to/twitter-archive
  3. Search: xf search "your query"
"#)]
pub struct Cli {
    /// Path to the database file
    #[arg(long, env = "XF_DB", global = true)]
    pub db: Option<PathBuf>,

    /// Path to the search index directory
    #[arg(long, env = "XF_INDEX", global = true)]
    pub index: Option<PathBuf>,

    /// Output format
    #[arg(long, short = 'f', default_value = "text", global = true)]
    pub format: OutputFormat,

    /// Be verbose (show debug info)
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    /// Be quiet (suppress non-error output)
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Index a Twitter data archive
    Index(IndexArgs),

    /// Search the indexed archive
    Search(SearchArgs),

    /// Show archive statistics
    Stats(StatsArgs),

    /// Show information about a specific tweet
    Tweet(TweetArgs),

    /// List available data in the archive
    List(ListArgs),

    /// Export data in various formats
    Export(ExportArgs),

    /// Show or manage configuration
    Config(ConfigArgs),

    /// Update xf to the latest version
    Update,

    /// Generate shell completions
    Completions(CompletionsArgs),
}

#[derive(Args, Debug)]
pub struct IndexArgs {
    /// Path to the Twitter archive directory
    pub archive_path: PathBuf,

    /// Force full re-index (delete existing data)
    #[arg(long, short = 'F')]
    pub force: bool,

    /// Only index specific data types
    #[arg(long, value_delimiter = ',')]
    pub only: Option<Vec<DataType>>,

    /// Skip specific data types
    #[arg(long, value_delimiter = ',')]
    pub skip: Option<Vec<DataType>>,

    /// Number of parallel workers
    #[arg(long, short = 'j', default_value = "0")]
    pub jobs: usize,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Filter by data type (tweet, like, dm, grok)
    #[arg(long, short = 't', value_delimiter = ',')]
    pub types: Option<Vec<DataType>>,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "20")]
    pub limit: usize,

    /// Skip first N results (for pagination)
    #[arg(long, default_value = "0")]
    pub offset: usize,

    /// Sort order
    #[arg(long, short = 's', default_value = "relevance")]
    pub sort: SortOrder,

    /// Show only tweets from this date onwards (YYYY-MM-DD)
    #[arg(long)]
    pub since: Option<String>,

    /// Show only tweets until this date (YYYY-MM-DD)
    #[arg(long)]
    pub until: Option<String>,

    /// Search only in replies
    #[arg(long)]
    pub replies_only: bool,

    /// Exclude replies from results
    #[arg(long)]
    pub no_replies: bool,

    /// Include surrounding context (for DMs)
    #[arg(long, short = 'c')]
    pub context: bool,

    /// Fields to include in output
    #[arg(long, value_delimiter = ',')]
    pub fields: Option<Vec<String>>,
}

#[derive(Args, Debug)]
pub struct StatsArgs {
    /// Show detailed breakdown by month/year
    #[arg(long, short = 'd')]
    pub detailed: bool,

    /// Include top hashtags
    #[arg(long)]
    pub hashtags: bool,

    /// Include top mentions
    #[arg(long)]
    pub mentions: bool,

    /// Number of top items to show
    #[arg(long, short = 'n', default_value = "10")]
    pub top: usize,
}

#[derive(Args, Debug)]
pub struct TweetArgs {
    /// Tweet ID to show
    pub id: String,

    /// Show thread context (replies)
    #[arg(long, short = 't')]
    pub thread: bool,

    /// Show engagement metrics
    #[arg(long, short = 'e')]
    pub engagement: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// What to list
    #[arg(default_value = "files")]
    pub what: ListTarget,

    /// Limit number of items
    #[arg(long, short = 'n', default_value = "50")]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// What to export
    pub what: ExportTarget,

    /// Output file path (stdout if not specified)
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Export format
    #[arg(long, short = 'f', default_value = "json")]
    pub format: ExportFormat,

    /// Limit number of items
    #[arg(long, short = 'n')]
    pub limit: Option<usize>,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    /// Show current configuration
    #[arg(long)]
    pub show: bool,

    /// Set a configuration value
    #[arg(long)]
    pub set: Option<String>,

    /// Path to archive (sets default)
    #[arg(long)]
    pub archive: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum DataType {
    Tweet,
    Like,
    Dm,
    Grok,
    Follower,
    Following,
    Block,
    Mute,
    All,
}

impl DataType {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Tweet,
            Self::Like,
            Self::Dm,
            Self::Grok,
            Self::Follower,
            Self::Following,
            Self::Block,
            Self::Mute,
        ]
    }
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    JsonPretty,
    Compact,
    Csv,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum SortOrder {
    #[default]
    Relevance,
    Date,
    DateDesc,
    Engagement,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ListTarget {
    #[default]
    Files,
    Tweets,
    Likes,
    Dms,
    Conversations,
    Followers,
    Following,
    Blocks,
    Mutes,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ExportTarget {
    #[default]
    Tweets,
    Likes,
    Dms,
    Followers,
    Following,
    All,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ExportFormat {
    #[default]
    Json,
    Jsonl,
    Csv,
}
