# xf Performance Guide

This document covers performance characteristics, optimization strategies, and benchmarking for xf.

## System Investigation & Optimization Plan (xf-9rf)

This plan is self-contained and executable without external context. It covers architecture review,
correctness/risk audit, and a performance investigation/optimization program with reproducible
commands, test oracles, and rollback guidance.

### Objectives

- Map end-to-end data flow (parser → storage → search → CLI output).
- Identify correctness, reliability, and security risks in critical paths.
- Establish baseline performance (p50/p95/p99 latency, throughput, peak RSS).
- Profile CPU, allocation, and I/O hot paths with reproducible commands.
- Deliver a ranked optimization backlog using (Impact × Confidence) / Effort.
- Add regression guardrails (bench/e2e/perf scripts with detailed logging).

### Structured Task List (with Dependencies, Estimates, and Acceptance Criteria)

| ID | Task | Depends On | Estimate | Acceptance Criteria |
|---|---|---|---|---|
| T0 | Environment + dataset baseline capture | — | 0.5h | Document dataset path, size, OS, CPU, RAM, Rust toolchain, and git revision used for baselines. |
| T1 | Architecture walk-through (parser/storage/search/CLI) | T0 | 1.5h | Updated architecture notes include data flow, module boundaries, and I/O points. |
| T2 | Correctness + reliability audit of critical paths | T1 | 2.0h | Issue list with risk classification + repro steps; each issue has a test/validation plan. |
| T3 | Baseline performance measurements | T0 | 2.0h | p50/p95/p99 for index/search; throughput and peak RSS captured with commands and logs. |
| T4 | Profiling + bottleneck validation | T3 | 2.0h | CPU/allocation/I/O hotspots identified with supporting traces. |
| T5 | Optimization backlog + ranking | T4 | 1.5h | Backlog ranked with (Impact × Confidence) / Effort and explicit rollback plan per item. |
| T6 | Regression guardrails | T2, T3 | 2.0h | E2E/perf scripts with detailed logging; unit/integration tests for fixes. |
| T7 | Summary + next steps | T5, T6 | 1.0h | Summary includes outcomes, residual risks, and next experiments. |

### Equivalence Oracles (Correctness Guardrails)

All optimizations must preserve these outputs for identical inputs:

- `xf index` on fixture archive: identical total document counts, unchanged stored metadata.
- `xf search "rust" --format json`: identical JSON shape and result set ordering for equal scores.
- `xf search "rust" --types tweet --sort date`: ordering consistent with timestamps.
- `xf stats` (basic + detailed): totals and derived metrics unchanged.
- `xf export tweets --format csv`: identical CSV header + row count.
- `xf doctor`: same health check statuses for the same archive/index.

Each change must include at least one of:
- unit test (logic), or
- integration test (DB/search), or
- e2e/perf script with detailed logging (command, stdout/stderr, exit code, timing, env).

### Performance Workflow (Reproducible Commands)

Baseline metrics (example commands; adjust paths):

```bash
# Index baseline
time target/release/xf index /path/to/archive --db /tmp/xf.db --index /tmp/xf_index

# Search latency samples (repeat N times for p50/p95/p99)
for i in {1..50}; do target/release/xf search "rust" --db /tmp/xf.db --index /tmp/xf_index >/tmp/xf.out; done

# RSS sampling (Linux)
/usr/bin/time -v target/release/xf search "rust" --db /tmp/xf.db --index /tmp/xf_index >/tmp/xf.out
```

Profiling:

```bash
# CPU
perf record --call-graph=dwarf target/release/xf search "rust" --db /tmp/xf.db --index /tmp/xf_index
perf report

# Allocations (if available)
valgrind --tool=massif target/release/xf index /path/to/archive --db /tmp/xf.db --index /tmp/xf_index
```

All profiling runs must log: command, stdout/stderr, exit code, elapsed time, and environment.

### Optimization Backlog (Ranked by (Impact × Confidence) / Effort)

Scores use a 1–5 scale; higher is better. These are hypotheses to validate via profiling.

| Rank | Candidate | Impact | Confidence | Effort | Score | Rationale |
|---|---|---:|---:|---:|---:|---|
| 1 | Reduce Tantivy commit overhead by tuning writer memory and commit cadence | 5 | 3 | 2 | 7.5 | Indexing hot path dominated by commit/merge; likely win for large archives. |
| 2 | Avoid repeated JSON serialization in hot loops (metadata strings) | 3 | 4 | 2 | 6.0 | Indexing builds many small JSON strings; pre-allocate or reuse buffers. |
| 3 | Optimize prefix generation for large text fields (cap per-doc tokens) | 3 | 3 | 2 | 4.5 | Prefix generation may dominate long text; reduce workload while preserving recall. |
| 4 | Batch SQLite inserts with prepared statement reuse in tight loops | 3 | 3 | 3 | 3.0 | Already using transactions; further batching might help large archives. |
| 5 | Add FTS fallback for search when Tantivy index is missing | 2 | 4 | 3 | 2.7 | Improves resilience but not performance; user-visible reliability benefit. |
| 6 | Avoid extra allocations in CLI formatting (string joins/wrap) | 2 | 3 | 2 | 3.0 | Small wins; low risk. |
| 7 | Tighten parsing by streaming JS wrapper removal for huge files | 4 | 2 | 5 | 1.6 | High impact but high effort; only if profiling shows parser bottleneck. |

### Minimal-Diff Policy (Performance Changes)

- One performance lever per change.
- No unrelated refactors.
- Provide before/after metrics and a proof sketch for output equivalence.

### Rollback Guidance

For any risky change:
- Keep the change isolated to a single commit.
- Provide a clear rollback note: `git revert <commit>` with justification.
- Re-run the baseline equivalence oracles after rollback to confirm restoration.

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
