//! End-to-end CLI tests for xf.
//!
//! These tests run the actual xf binary and verify:
//! - Command-line interface behavior
//! - Output format and content
//! - Error handling and messages
//! - Integration between all components
//!
//! # Test Organization
//!
//! Tests are organized by command:
//! - `test_index_*` - Index command tests
//! - `test_search_*` - Search command tests
//! - `test_stats_*` - Stats command tests
//! - `test_cli_*` - General CLI tests (flags, help, version)
//!
//! # Logging
//!
//! All tests use detailed logging for debugging:
//! - Test start/end timestamps
//! - Command output capture
//! - Timing information

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::TempDir;

// =============================================================================
// Test Utilities
// =============================================================================

/// Log a test event with timestamp
macro_rules! test_log {
    ($($arg:tt)*) => {
        let timestamp = chrono::Utc::now().format("%H:%M:%S%.3f");
        eprintln!("[TEST {}] {}", timestamp, format!($($arg)*));
    };
}

/// Create a test archive with the given data files
fn create_test_archive(
    tweets: Option<&str>,
    likes: Option<&str>,
    followers: Option<&str>,
    following: Option<&str>,
    dms: Option<&str>,
) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let data_dir = temp_dir.path().join("data");
    fs::create_dir_all(&data_dir).expect("Failed to create data directory");

    if let Some(content) = tweets {
        fs::write(data_dir.join("tweets.js"), content).expect("Failed to write tweets.js");
    }
    if let Some(content) = likes {
        fs::write(data_dir.join("like.js"), content).expect("Failed to write like.js");
    }
    if let Some(content) = followers {
        fs::write(data_dir.join("follower.js"), content).expect("Failed to write follower.js");
    }
    if let Some(content) = following {
        fs::write(data_dir.join("following.js"), content).expect("Failed to write following.js");
    }
    if let Some(content) = dms {
        fs::write(data_dir.join("direct-messages.js"), content)
            .expect("Failed to write direct-messages.js");
    }

    let archive_path = temp_dir.path().to_path_buf();
    (temp_dir, archive_path)
}

/// Create a minimal valid test archive
fn create_minimal_archive() -> (TempDir, PathBuf) {
    create_test_archive(
        Some(SAMPLE_TWEETS),
        Some(SAMPLE_LIKES),
        Some(SAMPLE_FOLLOWERS),
        Some(SAMPLE_FOLLOWING),
        None,
    )
}

/// Create an archive with Unicode content for edge case testing
fn create_unicode_archive() -> (TempDir, PathBuf) {
    create_test_archive(Some(SAMPLE_UNICODE_TWEETS), None, None, None, None)
}

/// Create an archive with empty data files
fn create_empty_archive() -> (TempDir, PathBuf) {
    create_test_archive(
        Some("window.YTD.tweets.part0 = []"),
        Some("window.YTD.like.part0 = []"),
        Some("window.YTD.follower.part0 = []"),
        Some("window.YTD.following.part0 = []"),
        None,
    )
}

/// Get the xf command ready for testing
fn xf_cmd() -> Command {
    cargo_bin_cmd!("xf")
}

// =============================================================================
// Sample Test Data
// =============================================================================

const SAMPLE_TWEETS: &str = r#"window.YTD.tweets.part0 = [
    {
        "tweet": {
            "id_str": "1234567890123456789",
            "created_at": "Wed Jan 08 12:00:00 +0000 2025",
            "full_text": "Hello world! This is a test tweet about Rust programming. #rust #programming",
            "source": "<a href=\"https://x.com\">X Web App</a>",
            "favorite_count": "42",
            "retweet_count": "7",
            "lang": "en",
            "entities": {
                "hashtags": [{"text": "rust"}, {"text": "programming"}],
                "user_mentions": [],
                "urls": []
            }
        }
    },
    {
        "tweet": {
            "id_str": "1234567890123456790",
            "created_at": "Thu Jan 09 14:30:00 +0000 2025",
            "full_text": "Learning about Tantivy search engine. It's incredibly fast for full-text search!",
            "source": "<a href=\"https://x.com\">X Web App</a>",
            "favorite_count": "100",
            "retweet_count": "25",
            "lang": "en",
            "entities": {
                "hashtags": [{"text": "search"}, {"text": "tantivy"}],
                "user_mentions": [],
                "urls": []
            }
        }
    },
    {
        "tweet": {
            "id_str": "1234567890123456791",
            "created_at": "Fri Jan 10 09:15:00 +0000 2025",
            "full_text": "SQLite is an amazing embedded database. Perfect for local data storage.",
            "source": "<a href=\"https://x.com\">X Web App</a>",
            "favorite_count": "55",
            "retweet_count": "12",
            "lang": "en",
            "entities": {
                "hashtags": [{"text": "sqlite"}, {"text": "database"}],
                "user_mentions": [],
                "urls": []
            }
        }
    }
]"#;

const SAMPLE_LIKES: &str = r#"window.YTD.like.part0 = [
    {
        "like": {
            "tweetId": "9876543210987654321",
            "fullText": "Great article about database optimization techniques and query performance",
            "expandedUrl": "https://example.com/article"
        }
    },
    {
        "like": {
            "tweetId": "9876543210987654322",
            "fullText": "Interesting thread on Rust async programming patterns",
            "expandedUrl": "https://example.com/thread"
        }
    }
]"#;

const SAMPLE_FOLLOWERS: &str = r#"window.YTD.follower.part0 = [
    {"follower": {"accountId": "111111111", "userLink": "https://x.com/user111"}},
    {"follower": {"accountId": "222222222", "userLink": "https://x.com/user222"}},
    {"follower": {"accountId": "333333333", "userLink": "https://x.com/user333"}}
]"#;

const SAMPLE_FOLLOWING: &str = r#"window.YTD.following.part0 = [
    {"following": {"accountId": "444444444", "userLink": "https://x.com/user444"}},
    {"following": {"accountId": "555555555", "userLink": "https://x.com/user555"}}
]"#;

const SAMPLE_UNICODE_TWEETS: &str = r#"window.YTD.tweets.part0 = [
    {
        "tweet": {
            "id_str": "1000000000000000001",
            "created_at": "Wed Jan 08 12:00:00 +0000 2025",
            "full_text": "Testing emoji support: 🦀 Rust is awesome! 🚀 Let's go!",
            "source": "<a href=\"https://x.com\">X Web App</a>",
            "favorite_count": "10",
            "retweet_count": "2",
            "lang": "en",
            "entities": {"hashtags": [], "user_mentions": [], "urls": []}
        }
    },
    {
        "tweet": {
            "id_str": "1000000000000000002",
            "created_at": "Thu Jan 09 14:30:00 +0000 2025",
            "full_text": "日本語のテストツイート。Unicode handling test!",
            "source": "<a href=\"https://x.com\">X Web App</a>",
            "favorite_count": "5",
            "retweet_count": "1",
            "lang": "ja",
            "entities": {"hashtags": [], "user_mentions": [], "urls": []}
        }
    },
    {
        "tweet": {
            "id_str": "1000000000000000003",
            "created_at": "Fri Jan 10 09:15:00 +0000 2025",
            "full_text": "Special chars: <>&\"' and newlines\nare handled correctly",
            "source": "<a href=\"https://x.com\">X Web App</a>",
            "favorite_count": "3",
            "retweet_count": "0",
            "lang": "en",
            "entities": {"hashtags": [], "user_mentions": [], "urls": []}
        }
    }
]"#;

// =============================================================================
// Help and Version Tests
// =============================================================================

#[test]
fn test_cli_help() {
    test_log!("Starting test_cli_help");
    let start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("xf"))
        .stdout(predicate::str::contains("Usage"));

    test_log!("test_cli_help completed in {:?}", start.elapsed());
}

#[test]
fn test_cli_version() {
    test_log!("Starting test_cli_version");
    let start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("xf"));

    test_log!("test_cli_version completed in {:?}", start.elapsed());
}

#[test]
fn test_cli_no_args() {
    test_log!("Starting test_cli_no_args");
    let start = Instant::now();

    let mut cmd = xf_cmd();
    // Running with no args should show help or error
    let output = cmd.output().expect("Failed to run command");

    // Either succeeds with help or fails with usage hint
    assert!(output.status.success() || !output.stderr.is_empty());

    test_log!("test_cli_no_args completed in {:?}", start.elapsed());
}

// =============================================================================
// Index Command Tests
// =============================================================================

#[test]
fn test_index_valid_archive() {
    test_log!("Starting test_index_valid_archive");
    let start = Instant::now();

    let (_temp_dir, archive_path) = create_minimal_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    test_log!("Archive path: {:?}", archive_path);
    test_log!("Database path: {:?}", db_path);
    test_log!("Index path: {:?}", index_path);

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    // Verify database was created
    assert!(db_path.exists(), "Database file should exist");

    // Verify index directory was created
    assert!(index_path.exists(), "Index directory should exist");

    test_log!(
        "test_index_valid_archive completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_index_empty_archive() {
    test_log!("Starting test_index_empty_archive");
    let start = Instant::now();

    let (_temp_dir, archive_path) = create_empty_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    test_log!(
        "test_index_empty_archive completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_index_nonexistent_path() {
    test_log!("Starting test_index_nonexistent_path");
    let start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg("/nonexistent/path/to/archive")
        .assert()
        .failure();

    test_log!(
        "test_index_nonexistent_path completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_index_unicode_content() {
    test_log!("Starting test_index_unicode_content");
    let start = Instant::now();

    let (_temp_dir, archive_path) = create_unicode_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    test_log!("Testing with Unicode content archive");

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    test_log!(
        "test_index_unicode_content completed in {:?}",
        start.elapsed()
    );
}

// =============================================================================
// Search Command Tests
// =============================================================================

/// Helper to create an indexed archive and return paths.
/// Returns (`archive_temp`, `output_dir`, `db_path`, `index_path`).
/// Note: `archive_temp` must be kept alive to prevent cleanup during tests,
/// even though we've already indexed the data.
fn create_indexed_archive() -> (TempDir, TempDir, PathBuf, PathBuf) {
    let (archive_temp, archive_path) = create_minimal_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    // Index the archive
    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    // Return archive_temp to keep it alive (prevents early cleanup)
    (archive_temp, output_dir, db_path, index_path)
}

#[test]
fn test_search_basic_query() {
    test_log!("Starting test_search_basic_query");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Searching for 'rust'");

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("rust")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Rust").or(predicate::str::contains("rust")));

    test_log!("test_search_basic_query completed in {:?}", start.elapsed());
}

#[test]
fn test_search_no_results() {
    test_log!("Starting test_search_no_results");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Searching for 'xyznonexistent123'");

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("xyznonexistent123")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    test_log!("test_search_no_results completed in {:?}", start.elapsed());
}

#[test]
fn test_search_with_limit() {
    test_log!("Starting test_search_with_limit");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Searching with limit 1");

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("tweet")
        .arg("--limit")
        .arg("1")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    test_log!("test_search_with_limit completed in {:?}", start.elapsed());
}

#[test]
fn test_search_type_filter_tweets() {
    test_log!("Starting test_search_type_filter_tweets");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Searching with type filter: tweet");

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("rust")
        .arg("--types")
        .arg("tweet")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    test_log!(
        "test_search_type_filter_tweets completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_search_type_filter_likes() {
    test_log!("Starting test_search_type_filter_likes");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Searching with type filter: like");

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("database")
        .arg("--types")
        .arg("like")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    test_log!(
        "test_search_type_filter_likes completed in {:?}",
        start.elapsed()
    );
}

// =============================================================================
// Stats Command Tests
// =============================================================================

#[test]
fn test_stats_after_indexing() {
    test_log!("Starting test_stats_after_indexing");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Getting stats");

    let mut cmd = xf_cmd();
    cmd.arg("stats")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success()
        // Should contain count information
        .stdout(
            predicate::str::contains("tweet")
                .or(predicate::str::contains("Tweet"))
                .or(predicate::str::contains("3")),
        );

    test_log!(
        "test_stats_after_indexing completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_stats_nonexistent_db() {
    test_log!("Starting test_stats_nonexistent_db");
    let start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("stats")
        .arg("--db")
        .arg("/nonexistent/path/to/db.db")
        .assert()
        .failure();

    test_log!(
        "test_stats_nonexistent_db completed in {:?}",
        start.elapsed()
    );
}

// =============================================================================
// Output Format Tests
// =============================================================================

#[test]
fn test_search_json_output() {
    test_log!("Starting test_search_json_output");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    test_log!("Searching with JSON output format");

    let mut cmd = xf_cmd();
    let output = cmd
        .arg("search")
        .arg("rust")
        .arg("--format")
        .arg("json")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .output()
        .expect("Failed to run command");

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // JSON output should be valid JSON or empty
        if !stdout.trim().is_empty() {
            // Should at least have JSON-like structure
            assert!(
                stdout.contains('[') || stdout.contains('{'),
                "JSON output should contain JSON structure: {stdout}"
            );
        }
    }

    test_log!("test_search_json_output completed in {:?}", start.elapsed());
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_invalid_command() {
    test_log!("Starting test_invalid_command");
    let start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("nonexistent_command")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error").or(predicate::str::contains("unrecognized")));

    test_log!("test_invalid_command completed in {:?}", start.elapsed());
}

#[test]
fn test_missing_required_args() {
    test_log!("Starting test_missing_required_args");
    let start = Instant::now();

    // Index without archive path
    let mut cmd = xf_cmd();
    cmd.arg("index").assert().failure();

    // Search without query
    let mut cmd = xf_cmd();
    cmd.arg("search").assert().failure();

    test_log!(
        "test_missing_required_args completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_search_missing_index() {
    test_log!("Starting test_search_missing_index");
    let start = Instant::now();

    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("missing_index");

    fs::write(&db_path, "").expect("Failed to create test db file");

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("rust")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("No search index found"));

    test_log!(
        "test_search_missing_index completed in {:?}",
        start.elapsed()
    );
}

// =============================================================================
// Quiet/Verbose Mode Tests
// =============================================================================

#[test]
fn test_quiet_mode() {
    test_log!("Starting test_quiet_mode");
    let start = Instant::now();

    let (_temp_dir, archive_path) = create_minimal_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .arg("--quiet")
        .assert()
        .success();

    test_log!("test_quiet_mode completed in {:?}", start.elapsed());
}

#[test]
fn test_verbose_mode() {
    test_log!("Starting test_verbose_mode");
    let start = Instant::now();

    let (_temp_dir, archive_path) = create_minimal_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .arg("--verbose")
        .assert()
        .success();

    test_log!("test_verbose_mode completed in {:?}", start.elapsed());
}

// =============================================================================
// Performance Tests (Basic)
// =============================================================================

#[test]
fn test_index_performance_basic() {
    test_log!("Starting test_index_performance_basic");
    let start = Instant::now();

    let (_temp_dir, archive_path) = create_minimal_archive();
    let output_dir = TempDir::new().expect("Failed to create output dir");
    let db_path = output_dir.path().join("test.db");
    let index_path = output_dir.path().join("test_index");

    let index_start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("index")
        .arg(&archive_path)
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    let index_time = index_start.elapsed();
    test_log!("Indexing took {:?}", index_time);

    // Basic performance check - indexing a small archive should be fast
    assert!(
        index_time.as_secs() < 30,
        "Indexing small archive took too long: {index_time:?}"
    );

    test_log!(
        "test_index_performance_basic completed in {:?}",
        start.elapsed()
    );
}

#[test]
fn test_search_performance_basic() {
    test_log!("Starting test_search_performance_basic");
    let start = Instant::now();

    let (_archive_temp, _output_dir, db_path, index_path) = create_indexed_archive();

    let search_start = Instant::now();

    let mut cmd = xf_cmd();
    cmd.arg("search")
        .arg("rust")
        .arg("--db")
        .arg(&db_path)
        .arg("--index")
        .arg(&index_path)
        .assert()
        .success();

    let search_time = search_start.elapsed();
    test_log!("Search took {:?}", search_time);

    // Search should be very fast (sub-second)
    assert!(
        search_time.as_secs() < 5,
        "Search took too long: {search_time:?}"
    );

    test_log!(
        "test_search_performance_basic completed in {:?}",
        start.elapsed()
    );
}
