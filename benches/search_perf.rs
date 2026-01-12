//! Performance benchmarks for xf using the standardized perf corpus.
//!
//! Run with: `cargo bench --bench search_perf`

use anyhow::{Context, Result, anyhow};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use tempfile::TempDir;

use xf::canonicalize::canonicalize_for_embedding;
use xf::embedder::Embedder;
use xf::hash_embedder::HashEmbedder;
use xf::hybrid::{candidate_count, rrf_fuse};
use xf::model::{DmConversation, GrokMessage, Like, Tweet};
use xf::stats_analytics::{ContentStats, EngagementStats, TemporalStats};
use xf::vector::VectorIndex;
use xf::{ArchiveParser, SearchEngine, Storage};

struct PerfCorpus {
    tweets: Vec<Tweet>,
    likes: Vec<Like>,
    dms: Vec<DmConversation>,
    grok: Vec<GrokMessage>,
}

fn perf_corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/perf_corpus")
}

fn load_perf_corpus() -> Result<&'static PerfCorpus> {
    static CORPUS: OnceLock<std::result::Result<PerfCorpus, String>> = OnceLock::new();
    let corpus = CORPUS.get_or_init(|| {
        let root = perf_corpus_root();
        let parser = ArchiveParser::new(root);
        let tweets = parser
            .parse_tweets()
            .map_err(|e| format!("parse tweets: {e}"))?;
        let likes = parser
            .parse_likes()
            .map_err(|e| format!("parse likes: {e}"))?;
        let dms = parser
            .parse_direct_messages()
            .map_err(|e| format!("parse dms: {e}"))?;
        let grok = parser
            .parse_grok_messages()
            .map_err(|e| format!("parse grok: {e}"))?;

        Ok(PerfCorpus {
            tweets,
            likes,
            dms,
            grok,
        })
    });

    corpus.as_ref().map_err(|err| anyhow!(err.clone()))
}

struct IndexedState {
    engine: SearchEngine,
    storage: Storage,
    vector_index: Option<VectorIndex>,
    _temp: TempDir,
}

fn build_indexed_state(with_embeddings: bool) -> Result<IndexedState> {
    let corpus = load_perf_corpus()?;

    let temp_dir = TempDir::new().context("temp dir")?;
    let db_path = temp_dir.path().join("bench.db");
    let index_path = temp_dir.path().join("index");
    std::fs::create_dir_all(&index_path).context("create index dir")?;

    let mut storage = Storage::open(&db_path).context("open storage")?;
    let engine = SearchEngine::open(&index_path).context("open search engine")?;
    let mut writer = engine.writer(100_000_000).context("create writer")?;

    storage
        .store_tweets(&corpus.tweets)
        .context("store tweets")?;
    engine
        .index_tweets(&mut writer, &corpus.tweets)
        .context("index tweets")?;

    storage.store_likes(&corpus.likes).context("store likes")?;
    engine
        .index_likes(&mut writer, &corpus.likes)
        .context("index likes")?;

    storage
        .store_dm_conversations(&corpus.dms)
        .context("store dms")?;
    engine
        .index_dms(&mut writer, &corpus.dms)
        .context("index dms")?;

    storage
        .store_grok_messages(&corpus.grok)
        .context("store grok")?;
    engine
        .index_grok_messages(&mut writer, &corpus.grok)
        .context("index grok")?;

    writer.commit().context("commit index")?;
    engine.reload().context("reload searcher")?;

    if with_embeddings {
        xf::generate_embeddings(&storage, false).context("generate embeddings")?;
    }

    let vector_index = if with_embeddings {
        Some(VectorIndex::load_from_storage(&storage).context("load vector index")?)
    } else {
        None
    };

    Ok(IndexedState {
        engine,
        storage,
        vector_index,
        _temp: temp_dir,
    })
}

fn query_embedding(query: &str) -> Result<Vec<f32>> {
    let embedder = HashEmbedder::default();
    let canonical = canonicalize_for_embedding(query);
    embedder
        .embed(&canonical)
        .map_err(|e| anyhow!("embed query: {e}"))
}

// ============================================================================
// Search Benchmarks (perf corpus)
// ============================================================================

fn bench_hybrid_search_cold(c: &mut Criterion) {
    let state = match build_indexed_state(true) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_hybrid_search_cold setup failed: {err}");
            return;
        }
    };
    let query = "rust";
    let limit = 20;
    let offset = 0;
    let candidates = candidate_count(limit, offset);
    let query_vec = match query_embedding(query) {
        Ok(vec) => vec,
        Err(err) => {
            eprintln!("bench_hybrid_search_cold embed failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("search_hybrid_cold");
    group.measurement_time(Duration::from_secs(12));
    group.sample_size(30);

    group.bench_function("cold", |b| {
        b.iter(|| match VectorIndex::load_from_storage(&state.storage) {
            Ok(vector_index) => {
                let lexical = state
                    .engine
                    .search(black_box(query), None, candidates)
                    .unwrap_or_default();
                let semantic = vector_index.search_top_k(&query_vec, candidates, None);
                let fused = rrf_fuse(&lexical, &semantic, limit, offset);
                black_box(fused.len());
            }
            Err(err) => {
                eprintln!("bench_hybrid_search_cold load failed: {err}");
                black_box(0usize);
            }
        });
    });

    group.finish();
}

fn bench_hybrid_search_warm(c: &mut Criterion) {
    let state = match build_indexed_state(true) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_hybrid_search_warm setup failed: {err}");
            return;
        }
    };
    let query = "rust";
    let limit = 20;
    let offset = 0;
    let candidates = candidate_count(limit, offset);
    let query_vec = match query_embedding(query) {
        Ok(vec) => vec,
        Err(err) => {
            eprintln!("bench_hybrid_search_warm embed failed: {err}");
            return;
        }
    };
    let Some(vector_index) = state.vector_index.as_ref() else {
        eprintln!("bench_hybrid_search_warm missing vector index");
        return;
    };

    let mut group = c.benchmark_group("search_hybrid_warm");
    group.measurement_time(Duration::from_secs(12));
    group.sample_size(50);

    group.bench_function("warm", |b| {
        b.iter(|| {
            let lexical = state
                .engine
                .search(black_box(query), None, candidates)
                .unwrap_or_default();
            let semantic = vector_index.search_top_k(&query_vec, candidates, None);
            let fused = rrf_fuse(&lexical, &semantic, limit, offset);
            black_box(fused.len());
        });
    });

    group.finish();
}

fn bench_lexical_search(c: &mut Criterion) {
    let state = match build_indexed_state(false) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_lexical_search setup failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("search_lexical");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    for limit in &[20, 100] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(limit), limit, |b, &limit| {
            b.iter(|| {
                let results = state
                    .engine
                    .search(black_box("machine"), None, limit)
                    .unwrap_or_default();
                black_box(results.len());
            });
        });
    }

    group.finish();
}

fn bench_semantic_search(c: &mut Criterion) {
    let state = match build_indexed_state(true) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_semantic_search setup failed: {err}");
            return;
        }
    };
    let Some(vector_index) = state.vector_index.as_ref() else {
        eprintln!("bench_semantic_search missing vector index");
        return;
    };
    let query_vec = match query_embedding("machine learning") {
        Ok(vec) => vec,
        Err(err) => {
            eprintln!("bench_semantic_search embed failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("search_semantic");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(80);

    for limit in &[20, 100] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(limit), limit, |b, &limit| {
            b.iter(|| {
                let results = vector_index.search_top_k(&query_vec, limit, None);
                black_box(results.len());
            });
        });
    }

    group.finish();
}

fn bench_search_pagination(c: &mut Criterion) {
    let state = match build_indexed_state(true) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_search_pagination setup failed: {err}");
            return;
        }
    };
    let Some(vector_index) = state.vector_index.as_ref() else {
        eprintln!("bench_search_pagination missing vector index");
        return;
    };
    let query = "rust";
    let limit = 20;
    let offset = 40;
    let candidates = candidate_count(limit, offset);
    let query_vec = match query_embedding(query) {
        Ok(vec) => vec,
        Err(err) => {
            eprintln!("bench_search_pagination embed failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("search_pagination");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(80);

    group.bench_function("hybrid_offset", |b| {
        b.iter(|| {
            let lexical = state
                .engine
                .search(black_box(query), None, candidates)
                .unwrap_or_default();
            let semantic = vector_index.search_top_k(&query_vec, candidates, None);
            let fused = rrf_fuse(&lexical, &semantic, limit, offset);
            black_box(fused.len());
        });
    });

    group.finish();
}

// ============================================================================
// Indexing Benchmarks (perf corpus)
// ============================================================================

#[allow(clippy::too_many_lines)]
fn bench_full_index(c: &mut Criterion) {
    let corpus = match load_perf_corpus() {
        Ok(corpus) => corpus,
        Err(err) => {
            eprintln!("bench_full_index load failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("index_full");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(10);

    let dm_messages: usize = corpus.dms.iter().map(|c| c.messages.len()).sum();
    let total_docs = corpus.tweets.len() + corpus.likes.len() + dm_messages + corpus.grok.len();
    group.throughput(Throughput::Elements(
        u64::try_from(total_docs).unwrap_or(u64::MAX),
    ));

    group.bench_function("full_index", |b| {
        b.iter_with_setup(
            || {
                let temp_dir = match TempDir::new() {
                    Ok(dir) => dir,
                    Err(err) => {
                        eprintln!("bench_full_index temp dir failed: {err}");
                        return None;
                    }
                };
                let db_path = temp_dir.path().join("bench.db");
                let index_path = temp_dir.path().join("index");
                if let Err(err) = std::fs::create_dir_all(&index_path) {
                    eprintln!("bench_full_index create index dir failed: {err}");
                    return None;
                }
                Some((temp_dir, db_path, index_path))
            },
            |state| {
                let Some((_temp_dir, db_path, index_path)) = state else {
                    return;
                };
                let mut storage = match Storage::open(&db_path) {
                    Ok(storage) => storage,
                    Err(err) => {
                        eprintln!("bench_full_index open storage failed: {err}");
                        return;
                    }
                };
                let engine = match SearchEngine::open(&index_path) {
                    Ok(engine) => engine,
                    Err(err) => {
                        eprintln!("bench_full_index open search engine failed: {err}");
                        return;
                    }
                };
                let mut writer = match engine.writer(100_000_000) {
                    Ok(writer) => writer,
                    Err(err) => {
                        eprintln!("bench_full_index create writer failed: {err}");
                        return;
                    }
                };

                if storage.store_tweets(&corpus.tweets).is_err() {
                    eprintln!("bench_full_index store tweets failed");
                    return;
                }
                if engine.index_tweets(&mut writer, &corpus.tweets).is_err() {
                    eprintln!("bench_full_index index tweets failed");
                    return;
                }

                if storage.store_likes(&corpus.likes).is_err() {
                    eprintln!("bench_full_index store likes failed");
                    return;
                }
                if engine.index_likes(&mut writer, &corpus.likes).is_err() {
                    eprintln!("bench_full_index index likes failed");
                    return;
                }

                if storage.store_dm_conversations(&corpus.dms).is_err() {
                    eprintln!("bench_full_index store dms failed");
                    return;
                }
                if engine.index_dms(&mut writer, &corpus.dms).is_err() {
                    eprintln!("bench_full_index index dms failed");
                    return;
                }

                if storage.store_grok_messages(&corpus.grok).is_err() {
                    eprintln!("bench_full_index store grok failed");
                    return;
                }
                if engine
                    .index_grok_messages(&mut writer, &corpus.grok)
                    .is_err()
                {
                    eprintln!("bench_full_index index grok failed");
                    return;
                }

                if writer.commit().is_err() {
                    eprintln!("bench_full_index commit failed");
                    return;
                }
                if engine.reload().is_err() {
                    eprintln!("bench_full_index reload failed");
                    return;
                }

                if xf::generate_embeddings(&storage, false).is_err() {
                    eprintln!("bench_full_index generate embeddings failed");
                    return;
                }
                black_box(engine.doc_count());
            },
        );
    });

    group.finish();
}

fn bench_embedding_generation(c: &mut Criterion) {
    let state = match build_indexed_state(false) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_embedding_generation setup failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("embedding_generation");
    group.measurement_time(Duration::from_secs(12));
    group.sample_size(20);

    group.bench_function("hash_embedder", |b| {
        b.iter(|| {
            if state.storage.clear_embeddings().is_err() {
                eprintln!("bench_embedding_generation clear embeddings failed");
            }
            if xf::generate_embeddings(&state.storage, false).is_err() {
                eprintln!("bench_embedding_generation generate embeddings failed");
            }
        });
    });

    group.finish();
}

fn bench_fts_indexing(c: &mut Criterion) {
    let mut state = match build_indexed_state(false) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_fts_indexing setup failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("fts_indexing");
    group.measurement_time(Duration::from_secs(12));
    group.sample_size(30);

    group.bench_function("rebuild_fts", |b| {
        b.iter(|| {
            if state.storage.rebuild_fts_tables().is_err() {
                eprintln!("bench_fts_indexing rebuild fts failed");
            }
        });
    });

    group.finish();
}

// ============================================================================
// Stats Benchmarks (perf corpus)
// ============================================================================

fn bench_stats_basic(c: &mut Criterion) {
    let state = match build_indexed_state(false) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_stats_basic setup failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("stats_basic");
    group.measurement_time(Duration::from_secs(8));
    group.sample_size(100);

    group.bench_function("stats", |b| {
        b.iter(|| {
            if let Ok(archive_stats) = state.storage.get_stats() {
                black_box(archive_stats);
            } else {
                eprintln!("bench_stats_basic stats failed");
            }
        });
    });

    group.finish();
}

fn bench_stats_detailed(c: &mut Criterion) {
    let state = match build_indexed_state(false) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("bench_stats_detailed setup failed: {err}");
            return;
        }
    };

    let mut group = c.benchmark_group("stats_detailed");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    group.bench_function("stats_detailed", |b| {
        b.iter(|| {
            let archive_stats = match state.storage.get_stats() {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("bench_stats_detailed stats failed: {err}");
                    return;
                }
            };
            let temporal = match TemporalStats::compute(&state.storage) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("bench_stats_detailed temporal failed: {err}");
                    return;
                }
            };
            let engagement = match EngagementStats::compute(&state.storage, 5) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("bench_stats_detailed engagement failed: {err}");
                    return;
                }
            };
            let content = match ContentStats::compute(&state.storage, 5) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("bench_stats_detailed content failed: {err}");
                    return;
                }
            };

            black_box((archive_stats, temporal, engagement, content));
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(
    name = search_benches;
    config = Criterion::default().significance_level(0.05).noise_threshold(0.02);
    targets =
        bench_hybrid_search_cold,
        bench_hybrid_search_warm,
        bench_lexical_search,
        bench_semantic_search,
        bench_search_pagination
);

criterion_group!(
    name = indexing_benches;
    config = Criterion::default().significance_level(0.05);
    targets =
        bench_full_index,
        bench_embedding_generation,
        bench_fts_indexing
);

criterion_group!(
    name = stats_benches;
    config = Criterion::default().significance_level(0.05);
    targets =
        bench_stats_basic,
        bench_stats_detailed
);

criterion_main!(search_benches, indexing_benches, stats_benches);
