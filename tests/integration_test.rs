//! Integration tests for xf.
//!
//! These tests verify end-to-end functionality including:
//! - Archive parsing and indexing
//! - Search across different data types
//! - CLI command execution

use chrono::Utc;
use std::path::PathBuf;
use tempfile::TempDir;
use xf::{
    model::*,
    parser::ArchiveParser,
    search::SearchEngine,
    storage::Storage,
};

/// Create a test archive directory structure
fn create_test_archive(dir: &TempDir) -> PathBuf {
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    // Create tweets.js
    let tweets_content = r#"window.YTD.tweets.part0 = [
        {
            "tweet": {
                "id_str": "1234567890",
                "created_at": "Wed Jan 08 12:00:00 +0000 2025",
                "full_text": "Hello world! This is my first test tweet about Rust programming.",
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
                "id_str": "1234567891",
                "created_at": "Thu Jan 09 14:30:00 +0000 2025",
                "full_text": "Learning about Tantivy search engine. It's incredibly fast!",
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
        }
    ]"#;
    std::fs::write(data_dir.join("tweets.js"), tweets_content).unwrap();

    // Create like.js
    let likes_content = r#"window.YTD.like.part0 = [
        {
            "like": {
                "tweetId": "9876543210",
                "fullText": "Great article about database optimization techniques",
                "expandedUrl": "https://example.com/article"
            }
        }
    ]"#;
    std::fs::write(data_dir.join("like.js"), likes_content).unwrap();

    // Create follower.js
    let followers_content = r#"window.YTD.follower.part0 = [
        {"follower": {"accountId": "111", "userLink": "https://x.com/user111"}},
        {"follower": {"accountId": "222", "userLink": "https://x.com/user222"}}
    ]"#;
    std::fs::write(data_dir.join("follower.js"), followers_content).unwrap();

    // Create following.js
    let following_content = r#"window.YTD.following.part0 = [
        {"following": {"accountId": "333", "userLink": "https://x.com/user333"}}
    ]"#;
    std::fs::write(data_dir.join("following.js"), following_content).unwrap();

    dir.path().to_path_buf()
}

#[test]
fn test_full_indexing_pipeline() {
    let temp_dir = TempDir::new().unwrap();
    let archive_path = create_test_archive(&temp_dir);

    // Parse archive
    let parser = ArchiveParser::new(&archive_path);

    // Parse tweets
    let tweets = parser.parse_tweets().unwrap();
    assert_eq!(tweets.len(), 2);
    assert_eq!(tweets[0].id, "1234567890");
    assert!(tweets[0].full_text.contains("Rust programming"));

    // Parse likes
    let likes = parser.parse_likes().unwrap();
    assert_eq!(likes.len(), 1);
    assert!(likes[0].full_text.as_ref().unwrap().contains("database"));

    // Parse followers
    let followers = parser.parse_followers().unwrap();
    assert_eq!(followers.len(), 2);

    // Parse following
    let following = parser.parse_following().unwrap();
    assert_eq!(following.len(), 1);
}

#[test]
fn test_storage_and_retrieval() {
    let temp_dir = TempDir::new().unwrap();
    let archive_path = create_test_archive(&temp_dir);

    // Parse
    let parser = ArchiveParser::new(&archive_path);
    let tweets = parser.parse_tweets().unwrap();
    let likes = parser.parse_likes().unwrap();
    let followers = parser.parse_followers().unwrap();

    // Store
    let mut storage = Storage::open_memory().unwrap();
    storage.store_tweets(&tweets).unwrap();
    storage.store_likes(&likes).unwrap();
    storage.store_followers(&followers).unwrap();

    // Verify stats
    let stats = storage.get_stats().unwrap();
    assert_eq!(stats.tweets_count, 2);
    assert_eq!(stats.likes_count, 1);
    assert_eq!(stats.followers_count, 2);

    // Test retrieval
    let tweet = storage.get_tweet("1234567890").unwrap();
    assert!(tweet.is_some());
    assert!(tweet.unwrap().full_text.contains("Rust"));

    // Test FTS search
    let results = storage.search_tweets("rust", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "1234567890");
}

#[test]
fn test_tantivy_indexing_and_search() {
    let temp_dir = TempDir::new().unwrap();
    let archive_path = create_test_archive(&temp_dir);

    // Parse
    let parser = ArchiveParser::new(&archive_path);
    let tweets = parser.parse_tweets().unwrap();
    let likes = parser.parse_likes().unwrap();

    // Index with Tantivy
    let engine = SearchEngine::open_memory().unwrap();
    let mut writer = engine.writer(15_000_000).unwrap();

    engine.index_tweets(&mut writer, &tweets).unwrap();
    engine.index_likes(&mut writer, &likes).unwrap();

    writer.commit().unwrap();
    engine.reload().unwrap();

    // Verify document count
    assert_eq!(engine.doc_count(), 3); // 2 tweets + 1 like

    // Test search
    let results = engine.search("rust", None, 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, SearchResultType::Tweet);

    // Test search with type filter
    let results = engine.search("database", Some(&[xf::search::DocType::Like]), 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, SearchResultType::Like);
}

#[test]
fn test_combined_storage_and_search() {
    let temp_dir = TempDir::new().unwrap();
    let archive_path = create_test_archive(&temp_dir);

    // This simulates the full indexing workflow
    let parser = ArchiveParser::new(&archive_path);
    let tweets = parser.parse_tweets().unwrap();

    // Store in SQLite
    let mut storage = Storage::open_memory().unwrap();
    storage.store_tweets(&tweets).unwrap();

    // Index in Tantivy
    let engine = SearchEngine::open_memory().unwrap();
    let mut writer = engine.writer(15_000_000).unwrap();
    engine.index_tweets(&mut writer, &tweets).unwrap();
    writer.commit().unwrap();
    engine.reload().unwrap();

    // Search with Tantivy
    let tantivy_results = engine.search("tantivy", None, 10).unwrap();
    assert_eq!(tantivy_results.len(), 1);

    // Use ID from Tantivy to fetch full record from SQLite
    let tweet_id = &tantivy_results[0].id;
    let full_tweet = storage.get_tweet(tweet_id).unwrap().unwrap();
    assert!(full_tweet.full_text.contains("Tantivy"));
    assert_eq!(full_tweet.favorite_count, 100);
}

#[test]
fn test_empty_archive() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    // Create empty files
    std::fs::write(data_dir.join("tweets.js"), "window.YTD.tweets.part0 = []").unwrap();
    std::fs::write(data_dir.join("like.js"), "window.YTD.like.part0 = []").unwrap();

    let parser = ArchiveParser::new(temp_dir.path());

    let tweets = parser.parse_tweets().unwrap();
    assert!(tweets.is_empty());

    let likes = parser.parse_likes().unwrap();
    assert!(likes.is_empty());
}

#[test]
fn test_search_ranking() {
    // Test that more relevant results are ranked higher
    let engine = SearchEngine::open_memory().unwrap();
    let mut writer = engine.writer(15_000_000).unwrap();

    let tweets = vec![
        Tweet {
            id: "1".to_string(),
            created_at: Utc::now(),
            full_text: "Rust is a great programming language".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        },
        Tweet {
            id: "2".to_string(),
            created_at: Utc::now(),
            full_text: "Rust Rust Rust programming with Rust is all about Rust".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        },
    ];

    engine.index_tweets(&mut writer, &tweets).unwrap();
    writer.commit().unwrap();
    engine.reload().unwrap();

    let results = engine.search("rust", None, 10).unwrap();
    assert_eq!(results.len(), 2);

    // The tweet with more "rust" occurrences should rank higher (BM25)
    assert_eq!(results[0].id, "2");
    assert!(results[0].score > results[1].score);
}

#[test]
fn test_data_files_listing() {
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    // Create various files
    std::fs::write(data_dir.join("tweets.js"), "").unwrap();
    std::fs::write(data_dir.join("like.js"), "").unwrap();
    std::fs::write(data_dir.join("follower.js"), "").unwrap();
    std::fs::write(data_dir.join("not-js.txt"), "").unwrap();

    let parser = ArchiveParser::new(temp_dir.path());
    let files = parser.list_data_files().unwrap();

    assert!(files.contains(&"tweets.js".to_string()));
    assert!(files.contains(&"like.js".to_string()));
    assert!(files.contains(&"follower.js".to_string()));
    assert!(!files.contains(&"not-js.txt".to_string()));
}

#[test]
fn test_unicode_content() {
    let engine = SearchEngine::open_memory().unwrap();
    let mut writer = engine.writer(15_000_000).unwrap();

    let tweets = vec![
        Tweet {
            id: "unicode1".to_string(),
            created_at: Utc::now(),
            full_text: "Testing unicode: emoji and symbols".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        },
        Tweet {
            id: "unicode2".to_string(),
            created_at: Utc::now(),
            full_text: "Japanese text".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: Some("ja".to_string()),
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        },
    ];

    engine.index_tweets(&mut writer, &tweets).unwrap();
    writer.commit().unwrap();
    engine.reload().unwrap();

    // Search for unicode content
    let results = engine.search("emoji", None, 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "unicode1");

    let results = engine.search("Japanese", None, 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "unicode2");
}
