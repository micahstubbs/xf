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

// ============================================================================
// Performance Benchmarks (xf-11.4.4)
// ============================================================================

use crate::SearchEngine;

/// Performance thresholds for query latency (milliseconds).
mod thresholds {
    /// Acceptable: under 50ms
    pub const QUERY_ACCEPTABLE_MS: f64 = 50.0;
    /// Slow: over 100ms is a warning
    pub const QUERY_SLOW_MS: f64 = 100.0;

    /// Index load time thresholds
    pub const LOAD_ACCEPTABLE_MS: f64 = 500.0;
    pub const LOAD_SLOW_MS: f64 = 1000.0;

    /// Number of iterations for benchmark stability
    pub const BENCHMARK_ITERATIONS: usize = 10;
}

/// Latency statistics from a benchmark run.
#[derive(Debug, Clone, Serialize)]
pub struct LatencyStats {
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub iterations: usize,
}

impl LatencyStats {
    /// Compute statistics from a vector of durations in milliseconds.
    fn from_durations(durations: &mut [f64]) -> Self {
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = durations.len();
        let sum: f64 = durations.iter().sum();
        let n_f64 = f64::from(u32::try_from(n).unwrap_or(u32::MAX));

        Self {
            min_ms: durations.first().copied().unwrap_or(0.0),
            max_ms: durations.last().copied().unwrap_or(0.0),
            mean_ms: if n > 0 { sum / n_f64 } else { 0.0 },
            p50_ms: percentile(durations, 50),
            p95_ms: percentile(durations, 95),
            p99_ms: percentile(durations, 99),
            iterations: n,
        }
    }

    /// Format as a concise string for health check messages.
    #[must_use]
    fn format_summary(&self) -> String {
        format!(
            "p50={:.1}ms, p95={:.1}ms, p99={:.1}ms (n={})",
            self.p50_ms, self.p95_ms, self.p99_ms, self.iterations
        )
    }
}

/// Calculate a percentile from a sorted slice.
fn percentile(sorted: &[f64], pct: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (pct * sorted.len() / 100).min(sorted.len() - 1);
    sorted[idx]
}

/// Benchmark index load time.
///
/// Opens the index multiple times and measures load latency.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn benchmark_index_load(index_path: &Path) -> HealthCheck {
    let mut durations = Vec::with_capacity(3);

    // Fewer iterations for load test since it's more expensive
    for _ in 0..3 {
        let start = Instant::now();
        let result = SearchEngine::open(index_path);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(_) => durations.push(elapsed_ms),
            Err(e) => {
                return HealthCheck {
                    category: CheckCategory::Performance,
                    name: "Index Load Time".to_string(),
                    status: CheckStatus::Error,
                    message: format!("Failed to load index: {e}"),
                    suggestion: Some("Verify index exists and is not corrupted".to_string()),
                };
            }
        }
    }

    let mut sorted = durations.clone();
    let latency_stats = LatencyStats::from_durations(&mut sorted);
    let median = latency_stats.p50_ms;

    let (check_status, suggestion) = if median < thresholds::LOAD_ACCEPTABLE_MS {
        (CheckStatus::Pass, None)
    } else if median < thresholds::LOAD_SLOW_MS {
        (
            CheckStatus::Warning,
            Some("Index load is slow. Consider running 'xf optimize'".to_string()),
        )
    } else {
        (
            CheckStatus::Warning,
            Some("Index load is very slow. Consider SSD storage or index rebuild".to_string()),
        )
    };

    HealthCheck {
        category: CheckCategory::Performance,
        name: "Index Load Time".to_string(),
        status: check_status,
        message: format!("{median:.0}ms"),
        suggestion,
    }
}

/// Benchmark simple single-word queries.
#[must_use]
pub fn benchmark_simple_query(engine: &SearchEngine) -> HealthCheck {
    let test_queries = ["the", "and", "test", "hello", "world"];
    let mut durations = Vec::with_capacity(thresholds::BENCHMARK_ITERATIONS * test_queries.len());

    for query in test_queries {
        for _ in 0..thresholds::BENCHMARK_ITERATIONS {
            let start = Instant::now();
            let _ = engine.search(query, None, 10);
            durations.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    let latency = LatencyStats::from_durations(&mut durations);
    let (check_status, suggestion) = evaluate_query_latency(&latency);

    HealthCheck {
        category: CheckCategory::Performance,
        name: "Simple Query Latency".to_string(),
        status: check_status,
        message: latency.format_summary(),
        suggestion,
    }
}

/// Benchmark phrase queries (multi-word, quoted).
#[must_use]
pub fn benchmark_phrase_query(engine: &SearchEngine) -> HealthCheck {
    let test_queries = ["\"hello world\"", "\"the quick\"", "\"test message\""];
    let mut durations = Vec::with_capacity(thresholds::BENCHMARK_ITERATIONS * test_queries.len());

    for query in test_queries {
        for _ in 0..thresholds::BENCHMARK_ITERATIONS {
            let start = Instant::now();
            let _ = engine.search(query, None, 10);
            durations.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    let latency = LatencyStats::from_durations(&mut durations);
    let (check_status, suggestion) = evaluate_query_latency(&latency);

    HealthCheck {
        category: CheckCategory::Performance,
        name: "Phrase Query Latency".to_string(),
        status: check_status,
        message: latency.format_summary(),
        suggestion,
    }
}

/// Benchmark complex boolean queries.
#[must_use]
pub fn benchmark_complex_query(engine: &SearchEngine) -> HealthCheck {
    let test_queries = [
        "hello AND world",
        "test OR example",
        "NOT spam",
        "(hello OR hi) AND world",
    ];
    let mut durations = Vec::with_capacity(thresholds::BENCHMARK_ITERATIONS * test_queries.len());

    for query in test_queries {
        for _ in 0..thresholds::BENCHMARK_ITERATIONS {
            let start = Instant::now();
            let _ = engine.search(query, None, 10);
            durations.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    let latency = LatencyStats::from_durations(&mut durations);
    let (check_status, suggestion) = evaluate_query_latency(&latency);

    HealthCheck {
        category: CheckCategory::Performance,
        name: "Complex Query Latency".to_string(),
        status: check_status,
        message: latency.format_summary(),
        suggestion,
    }
}

/// Benchmark FTS5 queries via `SQLite`.
#[must_use]
pub fn benchmark_fts5_query(storage: &crate::Storage) -> HealthCheck {
    let test_queries = ["the", "test", "hello", "and"];
    let mut durations = Vec::with_capacity(thresholds::BENCHMARK_ITERATIONS * test_queries.len());

    for query in test_queries {
        for _ in 0..thresholds::BENCHMARK_ITERATIONS {
            let start = Instant::now();
            // Use search_tweets which queries via FTS5
            let _ = storage.search_tweets(query, 10);
            durations.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    let latency = LatencyStats::from_durations(&mut durations);
    let (check_status, suggestion) = evaluate_query_latency(&latency);

    HealthCheck {
        category: CheckCategory::Performance,
        name: "FTS5 Query Latency".to_string(),
        status: check_status,
        message: latency.format_summary(),
        suggestion,
    }
}

/// Evaluate query latency against thresholds.
fn evaluate_query_latency(latency: &LatencyStats) -> (CheckStatus, Option<String>) {
    let p95 = latency.p95_ms;

    if p95 < thresholds::QUERY_ACCEPTABLE_MS {
        (CheckStatus::Pass, None)
    } else if p95 < thresholds::QUERY_SLOW_MS {
        (
            CheckStatus::Warning,
            Some("Query latency is elevated. Consider 'xf optimize'".to_string()),
        )
    } else {
        (
            CheckStatus::Warning,
            Some("Query latency is high. Consider index optimization or SSD storage".to_string()),
        )
    }
}

/// Run all performance benchmarks.
///
/// Returns a vector of health checks covering index load time,
/// simple/phrase/complex query latencies, and FTS5 performance.
pub fn run_performance_benchmarks(
    index_path: &Path,
    engine: &SearchEngine,
    storage: &crate::Storage,
) -> Vec<HealthCheck> {
    let mut checks = Vec::with_capacity(5);

    info!("Running performance benchmarks...");

    // Index load time
    debug!("Benchmarking index load time");
    checks.push(benchmark_index_load(index_path));

    // Simple queries
    debug!("Benchmarking simple queries");
    checks.push(benchmark_simple_query(engine));

    // Phrase queries
    debug!("Benchmarking phrase queries");
    checks.push(benchmark_phrase_query(engine));

    // Complex boolean queries
    debug!("Benchmarking complex queries");
    checks.push(benchmark_complex_query(engine));

    // FTS5 queries
    debug!("Benchmarking FTS5 queries");
    checks.push(benchmark_fts5_query(storage));

    info!("Performance benchmarks complete: {} checks", checks.len());

    checks
}
