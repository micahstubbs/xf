//! Custom error types for xf.
//!
//! Provides structured error handling with detailed context for better
//! diagnostics and user experience.

use std::path::PathBuf;
use thiserror::Error;

/// Primary error type for xf operations.
///
/// Each variant provides specific context about what went wrong,
/// enabling better error messages and programmatic error handling.
#[derive(Error, Debug)]
pub enum XfError {
    // =========================================================================
    // Archive Errors
    // =========================================================================
    /// Archive directory not found at the specified path.
    #[error("Archive not found at '{path}'")]
    ArchiveNotFound { path: PathBuf },

    /// Archive exists but is missing the expected structure.
    #[error("Invalid archive structure: {reason}")]
    InvalidArchive { reason: String },

    /// Required file missing from archive.
    #[error("Missing required file in archive: {file}")]
    MissingArchiveFile { file: String },

    /// Failed to parse archive data file.
    #[error("Failed to parse '{file}': {reason}")]
    ParseError { file: String, reason: String },

    /// Archive manifest is invalid or corrupt.
    #[error("Invalid manifest: {reason}")]
    InvalidManifest { reason: String },

    // =========================================================================
    // Database Errors
    // =========================================================================
    /// Database file not found (not yet indexed).
    #[error("No indexed archive found. Run 'xf index <archive_path>' first.\nExpected database at: {path}")]
    DatabaseNotFound { path: PathBuf },

    /// Database schema version mismatch.
    #[error("Database schema version mismatch: expected {expected}, found {found}. Consider re-indexing with --force.")]
    SchemaMismatch { expected: i32, found: i32 },

    /// Database is locked by another process.
    #[error("Database is locked. Ensure no other xf processes are running.\nIf the problem persists, remove the lock files:\n  rm {path}-wal {path}-shm")]
    DatabaseLocked { path: PathBuf },

    /// Database operation failed.
    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    // =========================================================================
    // Search Index Errors
    // =========================================================================
    /// Search index not found.
    #[error("Search index not found at '{path}'. Re-run indexing.")]
    IndexNotFound { path: PathBuf },

    /// Search index is corrupted.
    #[error("Search index corrupted: {reason}. Re-index with 'xf index --force'.")]
    IndexCorrupted { reason: String },

    /// Search query parsing failed.
    #[error("Invalid search query: {reason}")]
    InvalidQuery { reason: String },

    /// Search operation failed.
    #[error("Search error: {0}")]
    SearchError(String),

    /// Tantivy-specific error.
    #[error("Search engine error: {0}")]
    TantivyError(#[from] tantivy::TantivyError),

    // =========================================================================
    // IO Errors
    // =========================================================================
    /// File read/write error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Path-specific IO error with context.
    #[error("Failed to {operation} '{path}': {source}")]
    PathError {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // =========================================================================
    // Configuration Errors
    // =========================================================================
    /// Configuration file parsing error.
    #[error("Invalid configuration in '{path}': {reason}")]
    ConfigError { path: PathBuf, reason: String },

    /// Environment variable error.
    #[error("Invalid environment variable {var}: {reason}")]
    EnvVarError { var: String, reason: String },

    // =========================================================================
    // Data Validation Errors
    // =========================================================================
    /// Invalid date format in archive data.
    #[error("Invalid date format '{value}' in {context}")]
    InvalidDate { value: String, context: String },

    /// Invalid tweet ID.
    #[error("Invalid tweet ID: {id}")]
    InvalidTweetId { id: String },

    /// Data not found.
    #[error("{item_type} with ID '{id}' not found")]
    NotFound { item_type: &'static str, id: String },

    // =========================================================================
    // CLI Errors
    // =========================================================================
    /// Invalid command-line argument.
    #[error("Invalid argument: {reason}")]
    InvalidArgument { reason: String },

    /// Unsupported operation.
    #[error("{operation} is not yet implemented")]
    NotImplemented { operation: String },

    // =========================================================================
    // Generic Errors
    // =========================================================================
    /// Catch-all for other errors with context.
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Wrapped anyhow error for gradual migration.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type alias for xf operations.
pub type Result<T> = std::result::Result<T, XfError>;

impl XfError {
    /// Create an archive not found error.
    pub fn archive_not_found(path: impl Into<PathBuf>) -> Self {
        Self::ArchiveNotFound { path: path.into() }
    }

    /// Create an invalid archive error.
    pub fn invalid_archive(reason: impl Into<String>) -> Self {
        Self::InvalidArchive {
            reason: reason.into(),
        }
    }

    /// Create a parse error.
    pub fn parse_error(file: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ParseError {
            file: file.into(),
            reason: reason.into(),
        }
    }

    /// Create a database not found error.
    pub fn database_not_found(path: impl Into<PathBuf>) -> Self {
        Self::DatabaseNotFound { path: path.into() }
    }

    /// Create an index not found error.
    pub fn index_not_found(path: impl Into<PathBuf>) -> Self {
        Self::IndexNotFound { path: path.into() }
    }

    /// Create an invalid query error.
    pub fn invalid_query(reason: impl Into<String>) -> Self {
        Self::InvalidQuery {
            reason: reason.into(),
        }
    }

    /// Create a not found error.
    pub fn not_found(item_type: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            item_type,
            id: id.into(),
        }
    }

    /// Create a path error with context.
    pub fn path_error(
        operation: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::PathError {
            operation,
            path: path.into(),
            source,
        }
    }

    /// Wrap an error with additional context.
    pub fn with_context<E>(context: impl Into<String>, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::WithContext {
            context: context.into(),
            source: Box::new(source),
        }
    }

    /// Check if this error is recoverable (user can fix it).
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ArchiveNotFound { .. }
                | Self::DatabaseNotFound { .. }
                | Self::IndexNotFound { .. }
                | Self::InvalidQuery { .. }
                | Self::InvalidArgument { .. }
                | Self::NotFound { .. }
        )
    }

    /// Check if this error suggests re-indexing.
    pub fn suggests_reindex(&self) -> bool {
        matches!(
            self,
            Self::SchemaMismatch { .. }
                | Self::IndexCorrupted { .. }
                | Self::IndexNotFound { .. }
        )
    }

    /// Get a suggestion for how to fix this error, if applicable.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::ArchiveNotFound { .. } => {
                Some("Verify the archive path and ensure the X data export is extracted.")
            }
            Self::DatabaseNotFound { .. } => {
                Some("Run 'xf index <archive_path>' to create the database.")
            }
            Self::IndexNotFound { .. } | Self::IndexCorrupted { .. } => {
                Some("Run 'xf index --force <archive_path>' to rebuild the index.")
            }
            Self::SchemaMismatch { .. } => {
                Some("Run 'xf index --force <archive_path>' to upgrade the schema.")
            }
            Self::DatabaseLocked { .. } => {
                Some("Close other xf instances or remove stale lock files.")
            }
            Self::InvalidQuery { .. } => Some(
                "Check query syntax. Use quotes for phrases, AND/OR for boolean, * for wildcards.",
            ),
            _ => None,
        }
    }
}

/// Extension trait for adding context to Results.
pub trait ResultExt<T> {
    /// Add context to an error.
    fn context(self, context: impl Into<String>) -> Result<T>;

    /// Add context lazily (only evaluated on error).
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|e| XfError::with_context(context, e))
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| XfError::with_context(f(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = XfError::archive_not_found("/path/to/archive");
        assert!(err.to_string().contains("/path/to/archive"));
    }

    #[test]
    fn test_error_suggestions() {
        let err = XfError::database_not_found("/path/to/db");
        assert!(err.suggestion().is_some());
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_reindex_suggestion() {
        let err = XfError::IndexCorrupted {
            reason: "checksum mismatch".to_string(),
        };
        assert!(err.suggests_reindex());
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let xf_err: XfError = io_err.into();
        assert!(matches!(xf_err, XfError::IoError(_)));
    }

    #[test]
    fn test_from_rusqlite_error() {
        // This test verifies the From impl exists
        fn accepts_xf_error(_: XfError) {}
        let sqlite_err = rusqlite::Error::InvalidQuery;
        accepts_xf_error(sqlite_err.into());
    }
}
