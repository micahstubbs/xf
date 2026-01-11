//! Health check types for `xf doctor`.
//!
//! This module defines common structures used by archive, database, and index
//! diagnostics. Individual checks live in their respective modules.

use serde::Serialize;

/// High-level category for a health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCategory {
    Archive,
    Database,
    Index,
    Performance,
}

/// Status for an individual health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warning,
    Error,
}

impl CheckStatus {
    /// Whether the check is healthy enough for continued operation.
    #[must_use]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// Single health check result.
#[derive(Debug, Clone, Serialize)]
pub struct HealthCheck {
    pub category: CheckCategory,
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// Table row counts (and optional size) for reporting.
#[derive(Debug, Clone, Serialize)]
pub struct TableStat {
    pub name: String,
    pub rows: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<i64>,
}

// ============================================================================
// Archive Structure Validation (xf-11.4.1)
// ============================================================================

use chrono::{Datelike, Utc};
use glob::glob;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::ArchiveParser;

/// File requirement specification.
struct FileRequirement {
    pattern: &'static str,
    required: bool,
    description: &'static str,
}

/// Expected files in an X archive.
const ARCHIVE_FILES: &[FileRequirement] = &[
    FileRequirement {
        pattern: "data/tweets.js",
        required: false, // Could be split into parts
        description: "Main tweets file",
    },
    FileRequirement {
        pattern: "data/tweets-part*.js",
        required: false, // Alternative to single file
        description: "Tweets parts",
    },
    FileRequirement {
        pattern: "data/direct-messages.js",
        required: false,
        description: "Direct messages",
    },
    FileRequirement {
        pattern: "data/direct-messages-group*.js",
        required: false,
        description: "Group DM parts",
    },
    FileRequirement {
        pattern: "data/like.js",
        required: false,
        description: "Likes/favorites",
    },
    FileRequirement {
        pattern: "data/follower.js",
        required: false,
        description: "Followers list",
    },
    FileRequirement {
        pattern: "data/following.js",
        required: false,
        description: "Following list",
    },
    FileRequirement {
        pattern: "data/block.js",
        required: false,
        description: "Blocked accounts",
    },
    FileRequirement {
        pattern: "data/mute.js",
        required: false,
        description: "Muted accounts",
    },
    FileRequirement {
        pattern: "data/grok-conversation*.js",
        required: false,
        description: "Grok AI conversations",
    },
];

/// Check that required archive files are present.
///
/// # Errors
/// Returns error if glob pattern matching fails.
pub fn check_required_files(archive_path: &Path) -> crate::Result<Vec<HealthCheck>> {
    let mut checks = Vec::new();
    let mut has_tweets = false;

    for req in ARCHIVE_FILES {
        let full_pattern = archive_path.join(req.pattern);
        let pattern_str = full_pattern.to_string_lossy();

        debug!("Checking for pattern: {}", pattern_str);

        let matches: Vec<_> = glob(&pattern_str)
            .map_err(|e| crate::XfError::invalid_archive(format!("Invalid glob pattern: {e}")))?
            .filter_map(Result::ok)
            .collect();

        let exists = !matches.is_empty();

        if req.pattern.contains("tweets") && exists {
            has_tweets = true;
        }

        let status = if exists {
            CheckStatus::Pass
        } else if req.required {
            CheckStatus::Error
        } else {
            // Optional files that are missing are just info, not warnings
            continue; // Skip optional missing files from output
        };

        checks.push(HealthCheck {
            category: CheckCategory::Archive,
            name: format!("File: {} ({})", req.pattern, req.description),
            status,
            message: if exists {
                format!("Found {} file(s)", matches.len())
            } else {
                "Not found".into()
            },
            suggestion: if !exists && req.required {
                Some("Ensure archive was fully extracted".into())
            } else {
                None
            },
        });
    }

    // Special check: must have at least tweets.js or tweets-part*.js
    if !has_tweets {
        checks.push(HealthCheck {
            category: CheckCategory::Archive,
            name: "Tweets data".into(),
            status: CheckStatus::Error,
            message: "No tweets.js or tweets-part*.js found".into(),
            suggestion: Some(
                "Archive must contain tweet data. Check if archive was fully extracted.".into(),
            ),
        });
    }

    Ok(checks)
}

/// Validate JSON structure of archive files.
///
/// # Errors
/// Returns error if file reading or parsing fails.
pub fn check_json_structure(archive_path: &Path) -> crate::Result<Vec<HealthCheck>> {
    let mut checks = Vec::new();

    let files_to_check = [
        ("data/tweets.js", "Tweets"),
        ("data/like.js", "Likes"),
        ("data/direct-messages.js", "DMs"),
        ("data/follower.js", "Followers"),
        ("data/following.js", "Following"),
    ];

    for (file, label) in files_to_check {
        let path = archive_path.join(file);
        if !path.exists() {
            continue;
        }

        debug!("Validating JSON structure: {}", file);

        match validate_js_wrapped_json(&path) {
            Ok((count, warnings)) => {
                let status = if warnings.is_empty() {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warning
                };
                checks.push(HealthCheck {
                    category: CheckCategory::Archive,
                    name: format!("Parse: {label}"),
                    status,
                    message: format!("{count} items parsed"),
                    suggestion: warnings.first().cloned(),
                });
            }
            Err(e) => {
                warn!("Parse error for {}: {}", file, e);
                checks.push(HealthCheck {
                    category: CheckCategory::Archive,
                    name: format!("Parse: {label}"),
                    status: CheckStatus::Error,
                    message: format!("Parse error: {e}"),
                    suggestion: Some("Check if file is corrupted or incomplete".into()),
                });
            }
        }
    }

    Ok(checks)
}

/// Validate a JavaScript-wrapped JSON file and return item count with warnings.
fn validate_js_wrapped_json(path: &Path) -> crate::Result<(usize, Vec<String>)> {
    let start = Instant::now();
    let content =
        fs::read_to_string(path).map_err(|e| crate::XfError::path_error("read", path, e))?;

    // Strip JS wrapper: window.YTD.tweets.part0 = [...]
    let json_start = content.find('[').ok_or_else(|| {
        crate::XfError::parse_error(
            path.display().to_string(),
            "No JSON array found".to_string(),
        )
    })?;
    let json = &content[json_start..];

    // Parse as generic JSON array
    let items: Vec<serde_json::Value> = serde_json::from_str(json).map_err(|e| {
        crate::XfError::parse_error(path.display().to_string(), format!("Invalid JSON: {e}"))
    })?;

    let warnings = Vec::new();
    // Could add specific field validation here if needed

    if content.len() >= 5 * 1024 * 1024 {
        info!(
            "Parsed {} ({} bytes) in {}ms",
            path.display(),
            content.len(),
            start.elapsed().as_millis()
        );
    }

    Ok((items.len(), warnings))
}

/// Check for duplicate tweet IDs in the archive.
///
/// # Errors
/// Returns error if parsing fails.
pub fn check_duplicate_ids(archive_path: &Path) -> crate::Result<HealthCheck> {
    let parser = ArchiveParser::new(archive_path);
    let start = Instant::now();
    let tweets = parser.parse_tweets()?;
    if tweets.len() >= 100_000 {
        info!(
            "Parsed {} tweets for duplicate check in {}ms",
            tweets.len(),
            start.elapsed().as_millis()
        );
    }

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut duplicates: Vec<String> = Vec::new();

    for tweet in &tweets {
        if !seen_ids.insert(tweet.id.clone()) {
            duplicates.push(tweet.id.clone());
        }
    }

    Ok(HealthCheck {
        category: CheckCategory::Archive,
        name: "Duplicate Tweet IDs".into(),
        status: if duplicates.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warning
        },
        message: if duplicates.is_empty() {
            format!("{} unique tweet IDs", seen_ids.len())
        } else {
            format!("{} duplicate IDs found", duplicates.len())
        },
        suggestion: if duplicates.is_empty() {
            None
        } else {
            Some(format!(
                "Duplicate IDs: {}{}",
                duplicates[..3.min(duplicates.len())].join(", "),
                if duplicates.len() > 3 { "..." } else { "" }
            ))
        },
    })
}

/// Check timestamp consistency in tweets.
///
/// # Errors
/// Returns error if parsing fails.
#[allow(clippy::cast_sign_loss)]
pub fn check_timestamp_consistency(archive_path: &Path) -> crate::Result<HealthCheck> {
    let parser = ArchiveParser::new(archive_path);
    let start = Instant::now();
    let tweets = parser.parse_tweets()?;
    if tweets.len() >= 100_000 {
        info!(
            "Parsed {} tweets for timestamp check in {}ms",
            tweets.len(),
            start.elapsed().as_millis()
        );
    }

    let mut issues: Vec<String> = Vec::new();
    let now = Utc::now();
    let twitter_launch_year = 2006;

    for tweet in &tweets {
        // Check for future dates
        if tweet.created_at > now {
            issues.push(format!("{}: future date", tweet.id));
        }
        // Check for impossibly old dates (before Twitter existed)
        if tweet.created_at.year() < twitter_launch_year {
            issues.push(format!("{}: before {twitter_launch_year}", tweet.id));
        }
    }

    Ok(HealthCheck {
        category: CheckCategory::Archive,
        name: "Timestamp Validity".into(),
        status: if issues.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warning
        },
        message: if issues.is_empty() {
            format!("All {} timestamps valid", tweets.len())
        } else {
            format!("{} timestamp issues found", issues.len())
        },
        suggestion: if issues.is_empty() {
            None
        } else {
            Some(format!(
                "Issues: {}{}",
                issues[..3.min(issues.len())].join("; "),
                if issues.len() > 3 { "..." } else { "" }
            ))
        },
    })
}

/// Run all archive validation checks.
///
/// # Errors
/// Returns error if any check fails to execute.
pub fn validate_archive(archive_path: &Path) -> crate::Result<Vec<HealthCheck>> {
    let mut all_checks = Vec::new();

    // File presence checks
    all_checks.extend(check_required_files(archive_path)?);

    // JSON structure validation
    all_checks.extend(check_json_structure(archive_path)?);

    // Duplicate ID check (only if tweets exist)
    let tweets_path = archive_path.join("data/tweets.js");
    if tweets_path.exists()
        || glob(&archive_path.join("data/tweets-part*.js").to_string_lossy())
            .map(|mut g| g.next().is_some())
            .unwrap_or(false)
    {
        all_checks.push(check_duplicate_ids(archive_path)?);
        all_checks.push(check_timestamp_consistency(archive_path)?);
    }

    Ok(all_checks)
}
