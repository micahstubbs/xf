//! xf - Ultra-fast X data archive search
//!
//! This library provides the core functionality for indexing and searching
//! X data archives exported from x.com.
//!
//! # Modules
//!
//! - [`cli`] - Command-line interface definitions
//! - [`error`] - Custom error types with rich context
//! - [`model`] - Data models for X archive data
//! - [`parser`] - Archive parsing and data extraction
//! - [`search`] - Tantivy-based full-text search engine
//! - [`storage`] - `SQLite` storage layer

pub mod cli;
pub mod config;
pub mod error;
pub mod logging;
pub mod model;
pub mod parser;
pub mod perf;
pub mod search;
pub mod stats_analytics;
pub mod storage;

pub use cli::*;
pub use error::{Result, ResultExt, XfError};
pub use model::*;
pub use parser::ArchiveParser;
pub use search::SearchEngine;
pub use storage::Storage;

/// Default database filename
pub const DEFAULT_DB_NAME: &str = "xf.db";

/// Default index directory name
pub const DEFAULT_INDEX_DIR: &str = "xf_index";

/// Get the default data directory for xf
#[must_use]
pub fn default_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("xf")
}

/// Get the default database path
#[must_use]
pub fn default_db_path() -> std::path::PathBuf {
    default_data_dir().join(DEFAULT_DB_NAME)
}

/// Get the default index path
#[must_use]
pub fn default_index_path() -> std::path::PathBuf {
    default_data_dir().join(DEFAULT_INDEX_DIR)
}
