//! x_find - Ultra-fast Twitter/X archive search
//!
//! This library provides the core functionality for indexing and searching
//! Twitter data archives exported from twitter.com.

pub mod cli;
pub mod model;
pub mod parser;
pub mod search;
pub mod storage;

pub use cli::*;
pub use model::*;
pub use parser::ArchiveParser;
pub use search::SearchEngine;
pub use storage::Storage;

/// Default database filename
pub const DEFAULT_DB_NAME: &str = "xf.db";

/// Default index directory name
pub const DEFAULT_INDEX_DIR: &str = "xf_index";

/// Get the default data directory for xf
pub fn default_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("xf")
}

/// Get the default database path
pub fn default_db_path() -> std::path::PathBuf {
    default_data_dir().join(DEFAULT_DB_NAME)
}

/// Get the default index path
pub fn default_index_path() -> std::path::PathBuf {
    default_data_dir().join(DEFAULT_INDEX_DIR)
}
