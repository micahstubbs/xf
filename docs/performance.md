# xf Performance Guide

This document covers performance characteristics, optimization strategies, and benchmarking for xf.

## Performance Budgets

xf uses explicit performance budgets defined in `src/perf.rs` to ensure consistent latency:

| Operation | Target | Warning | Panic |
|-----------|--------|---------|-------|
| Simple search | 1ms | 5ms | 50ms |
| Phrase search | 2ms | 10ms | 100ms |
| Boolean search | 3ms | 15ms | 150ms |
| Prefix search | 2ms | 10ms | 100ms |
| Statistics query | 5ms | 20ms | 200ms |
| Single record lookup | 1ms | 5ms | 50ms |
| Batch 1K records | 50ms | 200ms | 2000ms |
| Index 1K tweets | 100ms | 500ms | 5000ms |
| Parser 10K tweets | 200ms | 1000ms | 10000ms |

## Optimization Strategies

### 1. Search Performance

#### Tantivy Configuration

```rust
// Optimal index settings for xf
let index_settings = IndexSettings {
    sort_by_field: None,  // BM25 ranking, not field sorting
    docstore_compression: Compressor::Lz4,  // Fast decompression
    docstore_blocksize: 16384,  // 16KB blocks
};
```

#### Query Optimization

- **Simple queries**: Use term queries for exact matches
- **Phrase queries**: Indexed with positions for exact phrase matching
- **Prefix queries**: Use edge n-grams for prefix completion
- **Boolean queries**: Combine with AND/OR/NOT operators

#### Caching

xf caches:
- Tantivy searcher (reused across queries)
- SQLite prepared statements
- Recent search results (configurable cache size)

### 2. Indexing Performance

#### Parallel Processing

```rust
// Enable parallel parsing with rayon
use rayon::prelude::*;

tweets.par_iter()
    .map(|tweet| process_tweet(tweet))
    .collect()
```

Configuration:
- `XF_THREADS=0` - Auto-detect CPU cores (default)
- `XF_THREADS=N` - Use N threads explicitly

#### Memory Management

```rust
// Buffer size for Tantivy indexer
let buffer_size_mb = config.indexing.buffer_size_mb;  // Default: 256MB
```

- Larger buffers = fewer commits = faster indexing
- Trade-off: memory usage vs indexing speed
- Recommendation: Use 256MB for archives under 100K tweets

#### Batch Operations

```rust
// Efficient batch insert for SQLite
storage.insert_tweets_batch(&tweets)?;  // Single transaction
```

### 3. Storage Performance

#### SQLite Configuration

```sql
-- WAL mode for concurrent reads
PRAGMA journal_mode = WAL;

-- Increase cache size
PRAGMA cache_size = -64000;  -- 64MB

-- Memory-mapped I/O
PRAGMA mmap_size = 268435456;  -- 256MB
```

#### Prepared Statements

All frequent queries use prepared statements:
- `get_tweet_by_id`
- `search_tweets`
- `get_statistics`

#### FTS5 Virtual Tables

```sql
-- Full-text search fallback
CREATE VIRTUAL TABLE fts_tweets USING fts5(
    tweet_id,
    full_text,
    content='tweets',
    content_rowid='rowid',
    tokenize='porter unicode61'
);
```

## Benchmarking

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark group
cargo bench --bench search_perf -- search_

# Run with custom sample size
cargo bench --bench search_perf -- --sample-size 100

# Save baseline
cargo bench -- --save-baseline main

# Compare against baseline
cargo bench -- --baseline main
```

### Benchmark Groups

1. **Search Benchmarks**
   - `search_simple_query`: Basic term search
   - `search_phrase_query`: Exact phrase matching
   - `search_boolean_query`: AND/OR/NOT combinations
   - `search_prefix_query`: Prefix/autocomplete

2. **Indexing Benchmarks**
   - `index_single_tweet`: Single document indexing
   - `index_batch_100`: Batch of 100 tweets
   - `index_batch_1000`: Batch of 1000 tweets

3. **Storage Benchmarks**
   - `storage_get_by_id`: Single record lookup
   - `storage_search`: FTS5 search
   - `storage_statistics`: Aggregate queries

4. **Scalability Benchmarks**
   - `search_varying_corpus`: Different corpus sizes

### Interpreting Results

```
search_simple_query     time:   [234.56 µs 245.67 µs 256.78 µs]
                        change: [-5.2345% -2.1234% +0.9876%] (p = 0.12)
                        No change in performance detected.
```

- **time**: [min, median, max] across samples
- **change**: Percentage change from baseline
- **p-value**: Statistical significance (< 0.05 = significant)

## Profiling

### CPU Profiling

```bash
# Using perf (Linux)
perf record --call-graph=dwarf cargo run --release -- search "query"
perf report

# Using Instruments (macOS)
cargo instruments -t "Time Profiler" -- search "query"

# Using flamegraph
cargo flamegraph -- search "query"
```

### Memory Profiling

```bash
# Using heaptrack
heaptrack cargo run --release -- index ~/archive

# Using Valgrind massif
valgrind --tool=massif target/release/xf index ~/archive
```

### I/O Profiling

```bash
# Using strace (Linux)
strace -e trace=read,write,open target/release/xf search "query"

# Using dtruss (macOS)
sudo dtruss target/release/xf search "query"
```

## Common Performance Issues

### 1. Slow First Query

**Symptom**: First search is slow, subsequent ones are fast.

**Cause**: Cold cache, Tantivy index loading.

**Solution**: This is expected behavior. The index is memory-mapped on first access.

### 2. Slow Phrase Searches

**Symptom**: Phrase queries are 5-10x slower than term queries.

**Cause**: Position lookups required for phrase matching.

**Solution**: Consider using fuzzy matching for autocomplete instead of phrase prefixes.

### 3. High Memory Usage During Indexing

**Symptom**: Memory usage spikes during `xf index`.

**Cause**: Large buffer sizes, parallel parsing.

**Solution**: Reduce `XF_BUFFER_MB` or `XF_THREADS`:

```bash
XF_BUFFER_MB=128 XF_THREADS=2 xf index ~/archive
```

### 4. Slow Statistics Queries

**Symptom**: `xf stats` takes several seconds.

**Cause**: Full table scans for aggregate queries.

**Solution**: Ensure indexes exist on frequently queried columns. Check with:

```bash
sqlite3 ~/.local/share/xf/xf.db ".indices"
```

## Performance Tuning Checklist

### For Search-Heavy Workloads

- [ ] Increase cache size: `config.search.cache_size = 5000`
- [ ] Enable highlighting selectively (expensive for long texts)
- [ ] Use type filters to reduce search scope
- [ ] Consider fuzzy matching trade-offs

### For Large Archives (100K+ tweets)

- [ ] Increase buffer size: `XF_BUFFER_MB=512`
- [ ] Use SSD storage for index and database
- [ ] Enable parallel parsing (default)
- [ ] Consider incremental indexing (future feature)

### For Resource-Constrained Systems

- [ ] Reduce threads: `XF_THREADS=2`
- [ ] Reduce buffer: `XF_BUFFER_MB=64`
- [ ] Disable parallel parsing: `config.indexing.parallel = false`
- [ ] Use SQLite FTS5 fallback instead of Tantivy

## Comparing Tantivy vs SQLite FTS5

| Aspect | Tantivy | SQLite FTS5 |
|--------|---------|-------------|
| Query latency | <1ms | 5-50ms |
| Ranking | BM25 (excellent) | BM25 (good) |
| Memory usage | Higher | Lower |
| Disk usage | Higher | Lower |
| Phrase queries | Fast | Fast |
| Prefix queries | Fast (with n-grams) | Moderate |

xf uses Tantivy as primary and FTS5 as fallback for compatibility.
