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
pub mod date_parser;
pub mod doctor;
pub mod error;
pub mod logging;
pub mod model;
pub mod parser;
pub mod perf;
pub mod repl;
pub mod search;
pub mod stats_analytics;
pub mod storage;

pub use cli::*;
pub use error::{
    Result, ResultExt, VALID_CONFIG_KEYS, VALID_DATA_TYPES, VALID_OUTPUT_FIELDS, XfError,
    find_closest_match, format_did_you_mean, format_error, format_unknown_value_error,
};
pub use model::*;
pub use parser::ArchiveParser;
pub use search::SearchEngine;
pub use storage::Storage;

use chrono::{DateTime, Datelike, Utc};

/// Default database filename
pub const DEFAULT_DB_NAME: &str = "xf.db";

/// Default index directory name
pub const DEFAULT_INDEX_DIR: &str = "xf_index";

/// Standard width for content dividers in CLI output
pub const CONTENT_DIVIDER_WIDTH: usize = 60;

/// Standard width for major header dividers in CLI output
pub const HEADER_DIVIDER_WIDTH: usize = 70;

const BYTES_PER_KB: u64 = 1024;
const BYTES_PER_MB: u64 = 1024 * 1024;
const BYTES_PER_GB: u64 = 1024 * 1024 * 1024;

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

/// Format an integer with thousands separators.
#[must_use]
pub fn format_number(value: i64) -> String {
    let abs = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(abs.len() + abs.len() / 3);

    for (idx, ch) in abs.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }

    let mut formatted: String = out.chars().rev().collect();
    if value < 0 {
        formatted.insert(0, '-');
    }
    formatted
}

/// Format an unsigned integer with thousands separators.
#[must_use]
pub fn format_number_u64(value: u64) -> String {
    let mut out = String::with_capacity(24);

    for (idx, ch) in value.to_string().chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }

    out.chars().rev().collect()
}

/// Format a usize with thousands separators.
#[must_use]
pub fn format_number_usize(value: usize) -> String {
    format_number_u64(u64::try_from(value).unwrap_or(u64::MAX))
}

/// Format a datetime as a human-friendly relative string.
///
/// Uses smart thresholds for readability:
/// - < 1 minute: "just now"
/// - < 1 hour: "Nm ago"
/// - < 24 hours: "Nh ago"
/// - < 7 days: "Nd ago"
/// - Same calendar year: "Mon D"
/// - Different year: "Mon D, YYYY"
#[must_use]
pub fn format_relative_date(dt: DateTime<Utc>) -> String {
    format_relative_date_with_base(dt, Utc::now())
}

/// Format a datetime relative to a fixed base time (useful for tests).
#[must_use]
pub fn format_relative_date_with_base(dt: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let duration = now.signed_duration_since(dt);

    // Handle future dates (shouldn't happen, but be safe)
    if duration.num_seconds() < 0 {
        return dt.format("%b %d, %Y").to_string();
    }

    let seconds = duration.num_seconds();
    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();

    if seconds < 60 {
        "just now".to_string()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else if hours < 24 {
        format!("{hours}h ago")
    } else if days < 7 {
        format!("{days}d ago")
    } else if dt.year() == now.year() {
        // Same calendar year: "Jan 15"
        dt.format("%b %d").to_string()
    } else {
        // Different year: "Jan 15, 2023"
        dt.format("%b %d, %Y").to_string()
    }
}

/// Format an optional datetime with human-friendly output.
#[must_use]
pub fn format_optional_date(value: Option<DateTime<Utc>>) -> String {
    value.map_or_else(|| "unknown".to_string(), format_relative_date)
}

/// Escape text for CSV by sanitizing newlines and quotes.
#[must_use]
pub fn csv_escape_text(text: &str) -> String {
    text.replace('"', "\"\"").replace(['\n', '\r'], " ")
}

/// Format a long identifier as a short token (e.g., 1234...6789).
#[must_use]
pub fn format_short_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 10 {
        return id.to_string();
    }
    let start: String = chars.iter().take(4).collect();
    let end: String = chars.iter().rev().take(4).rev().collect();
    format!("{start}...{end}")
}

/// Format bytes into a human-friendly string.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    if bytes < BYTES_PER_KB {
        format!("{bytes} B")
    } else if bytes < BYTES_PER_MB {
        format_bytes_with_unit(bytes, BYTES_PER_KB, "KB")
    } else if bytes < BYTES_PER_GB {
        format_bytes_with_unit(bytes, BYTES_PER_MB, "MB")
    } else {
        format_bytes_with_unit(bytes, BYTES_PER_GB, "GB")
    }
}

/// Format bytes for signed input, clamping negatives to zero.
#[must_use]
pub fn format_bytes_i64(bytes: i64) -> String {
    let bytes = u64::try_from(bytes.max(0)).unwrap_or(0);
    format_bytes(bytes)
}

fn format_bytes_with_unit(bytes: u64, unit: u64, suffix: &str) -> String {
    let whole = bytes / unit;
    let tenths = (bytes % unit) * 10 / unit;
    format!("{whole}.{tenths} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        csv_escape_text, format_bytes_i64, format_number, format_relative_date_with_base,
        format_short_id,
    };
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn format_number_adds_separators() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(12_345_678), "12,345,678");
        assert_eq!(format_number(-12_345), "-12,345");
    }

    #[test]
    fn format_relative_date_thresholds() {
        let base = Utc
            .with_ymd_and_hms(2025, 1, 10, 12, 0, 0)
            .single()
            .unwrap();

        assert_eq!(
            format_relative_date_with_base(base - Duration::seconds(30), base),
            "just now"
        );
        assert_eq!(
            format_relative_date_with_base(base - Duration::minutes(5), base),
            "5m ago"
        );
        assert_eq!(
            format_relative_date_with_base(base - Duration::hours(3), base),
            "3h ago"
        );
        assert_eq!(
            format_relative_date_with_base(base - Duration::days(2), base),
            "2d ago"
        );

        let same_year = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().unwrap();
        assert_eq!(format_relative_date_with_base(same_year, base), "Jan 01");

        let different_year = Utc
            .with_ymd_and_hms(2024, 12, 11, 0, 0, 0)
            .single()
            .unwrap();
        assert_eq!(
            format_relative_date_with_base(different_year, base),
            "Dec 11, 2024"
        );

        let future = base + Duration::days(2);
        assert_eq!(
            format_relative_date_with_base(future, base),
            future.format("%b %d, %Y").to_string()
        );
    }

    #[test]
    fn csv_escape_text_sanitizes_newlines_and_quotes() {
        let input = "Hello\r\n\"world\", ok";
        let escaped = csv_escape_text(input);
        assert_eq!(escaped, "Hello  \"\"world\"\", ok");
    }

    #[test]
    fn format_short_id_truncates_long_ids() {
        assert_eq!(format_short_id("short"), "short");
        assert_eq!(format_short_id("1234567890123"), "1234...0123");
    }

    #[test]
    fn format_bytes_i64_clamps_negative() {
        assert_eq!(format_bytes_i64(-5), "0 B");
    }
}
