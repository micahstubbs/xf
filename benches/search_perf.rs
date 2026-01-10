//! Performance benchmarks for xf.
//!
//! Benchmarks cover:
//! - Search operations (single term, phrase, complex boolean)
//! - Indexing operations (tweets, likes, DMs)
//! - Storage operations (writes, reads, FTS queries)
//!
//! Run with: `cargo bench --bench search_perf`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;
use tempfile::TempDir;

use xf::model::*;
use xf::search::SearchEngine;
use xf::storage::Storage;

/// Generate synthetic tweet data for benchmarking
fn generate_test_tweets(count: usize) -> Vec<Tweet> {
    let sample_texts = [
        "Just finished reading an amazing book about Rust programming!",
        "The weather today is absolutely beautiful, perfect for a walk.",
        "Working on a new machine learning project using Python and TensorFlow.",
        "Can't believe how fast this new search engine is! Sub-millisecond queries!",
        "Exploring the latest features in async Rust - the ecosystem is maturing nicely.",
        "Had a great coffee at the local cafe this morning. Highly recommend!",
        "The concert last night was incredible. Best live performance I've seen.",
        "Just deployed a new microservice architecture using Kubernetes.",
        "Learning about distributed systems and consensus algorithms today.",
        "The sunset from my balcony is breathtaking right now.",
    ];

    let hashtags_pool = ["rust", "programming", "tech", "coding", "software", "dev"];

    (0..count)
        .map(|i| {
            let text_idx = i % sample_texts.len();
            let hashtag_idx = i % hashtags_pool.len();

            Tweet {
                id: format!("{}", 1000000000 + i),
                created_at: chrono::Utc::now() - chrono::Duration::days((count - i) as i64),
                full_text: format!("{} #{}", sample_texts[text_idx], hashtags_pool[hashtag_idx]),
                source: Some("benchmark".to_string()),
                favorite_count: (i % 100) as i64,
                retweet_count: (i % 50) as i64,
                lang: Some("en".to_string()),
                in_reply_to_status_id: None,
                in_reply_to_user_id: None,
                in_reply_to_screen_name: None,
                is_retweet: false,
                hashtags: vec![hashtags_pool[hashtag_idx].to_string()],
                user_mentions: vec![],
                urls: vec![],
                media: vec![],
            }
        })
        .collect()
}

/// Generate synthetic like data for benchmarking
fn generate_test_likes(count: usize) -> Vec<Like> {
    let sample_texts = [
        "This is a liked tweet about technology and innovation.",
        "Great insights on software development best practices.",
        "Interesting perspective on the future of AI.",
        "Love this take on functional programming paradigms.",
        "Excellent thread on system design principles.",
    ];

    (0..count)
        .map(|i| Like {
            tweet_id: format!("{}", 2000000000 + i),
            full_text: Some(sample_texts[i % sample_texts.len()].to_string()),
            expanded_url: Some(format!("https://x.com/user/status/{}", 2000000000 + i)),
        })
        .collect()
}

/// Create a search engine with indexed test data
fn create_benchmark_search_engine(tweet_count: usize) -> (SearchEngine, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let engine = SearchEngine::open(temp_dir.path()).expect("Failed to create search engine");

    let tweets = generate_test_tweets(tweet_count);
    let mut writer = engine.writer(50_000_000).expect("Failed to create writer");
    engine.index_tweets(&mut writer, &tweets).expect("Failed to index tweets");
    writer.commit().expect("Failed to commit");
    engine.reload().expect("Failed to reload");

    (engine, temp_dir)
}

/// Create a storage instance with test data
fn create_benchmark_storage(tweet_count: usize) -> (Storage, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("bench.db");
    let mut storage = Storage::open(&db_path).expect("Failed to create storage");

    let tweets = generate_test_tweets(tweet_count);
    storage.store_tweets(&tweets).expect("Failed to store tweets");

    (storage, temp_dir)
}

// ============================================================================
// Search Benchmarks
// ============================================================================

fn bench_search_single_term(c: &mut Criterion) {
    let (engine, _temp) = create_benchmark_search_engine(10_000);

    let mut group = c.benchmark_group("search_single_term");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    // Benchmark different result limits
    for limit in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(limit),
            limit,
            |b, &limit| {
                b.iter(|| {
                    engine.search(black_box("rust"), None, black_box(limit))
                })
            },
        );
    }

    group.finish();
}

fn bench_search_phrase(c: &mut Criterion) {
    let (engine, _temp) = create_benchmark_search_engine(10_000);

    let mut group = c.benchmark_group("search_phrase");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    let phrases = [
        "machine learning",
        "search engine",
        "Rust programming",
        "distributed systems",
    ];

    for phrase in phrases.iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(phrase),
            phrase,
            |b, phrase| {
                b.iter(|| {
                    engine.search(black_box(&format!("\"{}\"", phrase)), None, 100)
                })
            },
        );
    }

    group.finish();
}

fn bench_search_complex_boolean(c: &mut Criterion) {
    let (engine, _temp) = create_benchmark_search_engine(10_000);

    let mut group = c.benchmark_group("search_complex");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    let queries = [
        ("and_query", "rust AND programming"),
        ("or_query", "rust OR python"),
        ("not_query", "programming NOT python"),
        ("mixed", "(rust OR python) AND learning"),
    ];

    for (name, query) in queries.iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            query,
            |b, query| {
                b.iter(|| {
                    engine.search(black_box(query), None, 100)
                })
            },
        );
    }

    group.finish();
}

fn bench_search_with_type_filter(c: &mut Criterion) {
    let (engine, _temp) = create_benchmark_search_engine(10_000);

    let mut group = c.benchmark_group("search_filtered");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    // Search with type filter
    group.bench_function("with_tweet_filter", |b| {
        b.iter(|| {
            engine.search(
                black_box("rust"),
                Some(&[xf::search::DocType::Tweet]),
                100,
            )
        })
    });

    // Search without filter for comparison
    group.bench_function("without_filter", |b| {
        b.iter(|| {
            engine.search(black_box("rust"), None, 100)
        })
    });

    group.finish();
}

// ============================================================================
// Indexing Benchmarks
// ============================================================================

fn bench_indexing_tweets(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing_tweets");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(50);

    for count in [100, 1000, 5000].iter() {
        let tweets = generate_test_tweets(*count);

        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &tweets,
            |b, tweets| {
                b.iter_with_setup(
                    || {
                        let temp_dir = TempDir::new().unwrap();
                        let engine = SearchEngine::open(temp_dir.path()).unwrap();
                        let writer = engine.writer(50_000_000).unwrap();
                        (engine, writer, temp_dir)
                    },
                    |(engine, mut writer, _temp)| {
                        engine.index_tweets(&mut writer, black_box(tweets)).unwrap();
                        writer.commit().unwrap();
                    },
                )
            },
        );
    }

    group.finish();
}

fn bench_indexing_likes(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexing_likes");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(50);

    for count in [100, 1000, 5000].iter() {
        let likes = generate_test_likes(*count);

        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &likes,
            |b, likes| {
                b.iter_with_setup(
                    || {
                        let temp_dir = TempDir::new().unwrap();
                        let engine = SearchEngine::open(temp_dir.path()).unwrap();
                        let writer = engine.writer(50_000_000).unwrap();
                        (engine, writer, temp_dir)
                    },
                    |(engine, mut writer, _temp)| {
                        engine.index_likes(&mut writer, black_box(likes)).unwrap();
                        writer.commit().unwrap();
                    },
                )
            },
        );
    }

    group.finish();
}

// ============================================================================
// Storage Benchmarks
// ============================================================================

fn bench_storage_write_tweets(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_write");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(50);

    for count in [100, 1000, 5000].iter() {
        let tweets = generate_test_tweets(*count);

        group.throughput(Throughput::Elements(*count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &tweets,
            |b, tweets| {
                b.iter_with_setup(
                    || {
                        let temp_dir = TempDir::new().unwrap();
                        let db_path = temp_dir.path().join("bench.db");
                        let storage = Storage::open(&db_path).unwrap();
                        (storage, temp_dir)
                    },
                    |(mut storage, _temp)| {
                        storage.store_tweets(black_box(tweets)).unwrap();
                    },
                )
            },
        );
    }

    group.finish();
}

fn bench_storage_read_tweet(c: &mut Criterion) {
    let (storage, _temp) = create_benchmark_storage(10_000);

    let mut group = c.benchmark_group("storage_read");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    // Read existing tweet
    group.bench_function("existing_tweet", |b| {
        b.iter(|| {
            storage.get_tweet(black_box("1000005000"))
        })
    });

    // Read non-existing tweet
    group.bench_function("missing_tweet", |b| {
        b.iter(|| {
            storage.get_tweet(black_box("9999999999"))
        })
    });

    group.finish();
}

fn bench_storage_fts_search(c: &mut Criterion) {
    let (storage, _temp) = create_benchmark_storage(10_000);

    let mut group = c.benchmark_group("storage_fts");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    // FTS search
    group.bench_function("fts_simple", |b| {
        b.iter(|| {
            storage.search_tweets(black_box("rust"), 100)
        })
    });

    group.finish();
}

// ============================================================================
// Scalability Benchmarks
// ============================================================================

fn bench_search_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("search_scalability");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(30);

    // Test search performance at different index sizes
    for size in [1_000, 5_000, 10_000, 50_000].iter() {
        let (engine, _temp) = create_benchmark_search_engine(*size);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("index_size", size),
            &engine,
            |b, engine| {
                b.iter(|| {
                    engine.search(black_box("rust"), None, 100)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(
    name = search_benches;
    config = Criterion::default()
        .significance_level(0.05)
        .noise_threshold(0.02);
    targets =
        bench_search_single_term,
        bench_search_phrase,
        bench_search_complex_boolean,
        bench_search_with_type_filter
);

criterion_group!(
    name = indexing_benches;
    config = Criterion::default()
        .significance_level(0.05);
    targets =
        bench_indexing_tweets,
        bench_indexing_likes
);

criterion_group!(
    name = storage_benches;
    config = Criterion::default()
        .significance_level(0.05);
    targets =
        bench_storage_write_tweets,
        bench_storage_read_tweet,
        bench_storage_fts_search
);

criterion_group!(
    name = scalability_benches;
    config = Criterion::default()
        .significance_level(0.05)
        .sample_size(20);
    targets =
        bench_search_scalability
);

criterion_main!(search_benches, indexing_benches, storage_benches, scalability_benches);
