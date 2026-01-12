# Performance Baseline

## Latest Results (Release Build)

Date: 2026-01-12T17:30:00Z (UTC)

### Environment

- Commit: `c5b1f1bd083f30b80f66ac7062bbbbd18a4fb392`
- Binary: `./target/release/xf` (release build with LTO + opt-level=z)
- OS: Linux threadripperje 6.17.0-8-generic x86_64
- CPU: AMD Ryzen Threadripper PRO 5975WX 32-Cores (64 threads)
- Memory: 499 GiB total
- Rust: `rustc 1.94.0-nightly (fecb335cb 2026-01-07)`

### Dataset

- Corpus: `tests/fixtures/perf_corpus`
- Total records: 17,500
  - Tweets: 10,000
  - Likes: 5,000
  - DMs: 2,000 (100 conversations)
  - Grok messages: 500
- Total indexed: 17,000 documents

### Latency Baselines (Release Build, 20 runs, warm cache)

| Command | p50 | p95 | p99 |
| --- | ---:| ---:| ---:|
| `xf search "rust" --limit 100` (hybrid) | 69.5 ms | 74.6 ms | 74.6 ms |
| `xf search "rust" --mode lexical --limit 100` | 9.9 ms | 11.2 ms | 11.2 ms |
| `xf search "rust" --mode semantic --limit 100` | 72.3 ms | 75.0 ms | 75.0 ms |

### Indexing Baseline (Release Build, 5 runs)

| p50 | p95 | p99 |
| ---:| ---:| ---:|
| 819.4 ms | 834.8 ms | 834.8 ms |

**Breakdown per data type (from indexing output):**
- Tweets: ~290 ms
- Likes: ~75 ms
- DMs: ~60 ms
- Grok: ~9 ms

### Memory & CPU (Release Build)

**Indexing (17,500 records):**
- Elapsed time: 0.78s
- User time: 1.38s (parallelization: 256% CPU utilization)
- System time: 0.63s
- Max RSS: 94,532 KB (~92.3 MB)

**Hybrid Search:**
- Elapsed time: 0.06s
- User time: 0.03s
- System time: 0.03s
- Max RSS: 47,772 KB (~46.7 MB)

### Performance vs Targets

| Metric | Target | Actual | Status |
| --- | --- | --- | --- |
| Hybrid search latency | <50ms | 69.5ms (p50) | Above target |
| Lexical search latency | <20ms | 9.9ms (p50) | **PASS** |
| Semantic search latency | <30ms | 72.3ms (p50) | Above target |
| Indexing 17.5K docs | <120s | 0.82s | **PASS** |
| Memory (indexing) | <200MB | 92.3 MB | **PASS** |
| Memory (search) | <200MB | 46.7 MB | **PASS** |

### Type-Filtered Search (xf-80)

Type filtering allows searching specific document types (tweet, dm, like, grok).

**Hybrid Search with --types filter (20 runs, warm cache):**

| Filter | p50 | p95 | p99 |
| --- | ---:| ---:| ---:|
| No filter (all types) | 67.2 ms | 72.2 ms | 73.8 ms |
| --types tweet | 70.4 ms | 74.9 ms | 77.0 ms |
| --types dm | 67.6 ms | 74.8 ms | 78.7 ms |
| --types like | 67.5 ms | 72.8 ms | 73.0 ms |
| --types grok | 66.3 ms | 68.9 ms | 69.1 ms |

**Semantic Search with --types filter (20 runs, warm cache):**

| Filter | p50 | p95 | p99 |
| --- | ---:| ---:| ---:|
| No filter (all types) | 72.2 ms | 76.6 ms | 76.8 ms |
| --types tweet | 67.8 ms | 71.7 ms | 74.3 ms |
| --types dm | 63.3 ms | 65.3 ms | 66.4 ms |
| --types like | 65.3 ms | 101.7 ms | 109.9 ms |
| --types grok | 65.7 ms | 79.4 ms | 92.9 ms |

**Observation:** Type filtering does not significantly reduce search latency. The vector index currently loads all embeddings regardless of type filter, then filters results. Future optimization could pre-filter embeddings by type to reduce memory scanning.

### Notes

- Hybrid and semantic search are still above the 50ms target. The bottleneck appears to be vector index loading from SQLite on each search. Further optimization with persistent mmap'd vector index may be needed.
- Lexical search is very fast (sub-10ms) and meets targets.
- Indexing is extremely fast (sub-1s) and well under the 120s target.
- Memory usage is reasonable at ~92 MB for indexing and ~47 MB for search.
- CPU parallelization during indexing is effective (256% CPU utilization on multi-core).
- Type filtering does not currently improve latency - embeddings are loaded fully then filtered.

---

## Previous Results (Debug Build)

Date: 2026-01-12T09:43:27Z (UTC)

### Environment

- Commit: `0895cd25a6466af52c1b620c83f3913c7e9e82e1`
- Binary: `./target/debug/xf` (debug build)
- OS: Linux threadripperje 6.17.0-8-generic x86_64
- CPU: AMD Ryzen Threadripper PRO 5975WX 32-Cores (64 threads)
- Memory: 499 GiB total, 231 GiB available (from `free -h`)
- Rust: `rustc 1.94.0-nightly (fecb335cb 2026-01-07)`

## Dataset

- Corpus: `tests/fixtures/perf_corpus`
- Total records: 17,500
  - Tweets: 10,000
  - Likes: 5,000
  - DMs: 2,000 (100 conversations)
  - Grok messages: 500
- Corpus manifest: `tests/fixtures/perf_corpus/corpus_manifest.json` (seed 42, scale 1.0)

## Setup

Environment variables used:

- `XF_DB=/tmp/tmp.ddnoeAjz9Y/xf.db`
- `XF_INDEX=/tmp/tmp.ddnoeAjz9Y/index`

Initial index run (creates DB/index + embeddings):

```
./target/debug/xf --quiet --no-color index tests/fixtures/perf_corpus
```

## Latency Baselines (20 runs, 3 warmups)

Commands were executed with the env vars above and stdout/stderr suppressed.
All values are in milliseconds.

| Command | p50 | p95 | p99 |
| --- | ---:| ---:| ---:|
| `xf search "rust" --limit 100` | 573.615 | 683.179 | 683.179 |
| `xf search "rust" --mode lexical --limit 100` | 37.415 | 47.515 | 47.515 |
| `xf search "rust" --mode semantic --limit 100` | 598.709 | 677.148 | 677.148 |
| `xf stats` | 35.752 | 39.869 | 39.869 |
| `xf stats --detailed` | 128.186 | 154.713 | 154.713 |

## Indexing Baseline (5 runs, 1 warmup)

Command executed with a fresh temp dir per run (no `--force` needed):

```
./target/debug/xf --quiet --no-color index tests/fixtures/perf_corpus
```

| p50 | p95 | p99 |
| ---:| ---:| ---:|
| 2834.647 ms | 2943.959 ms | 2943.959 ms |

## Memory & CPU

Command:

```
/usr/bin/time -v env XF_DB=/tmp/tmp.ddnoeAjz9Y/xf.db XF_INDEX=/tmp/tmp.ddnoeAjz9Y/index \
  ./target/debug/xf --quiet --no-color search "rust" --limit 100
```

Key results:

- Elapsed time: 0.55 s
- User time: 0.23 s
- Sys time: 0.32 s
- Max RSS: 63,364 kB (~61.9 MB)

## perf stat (optional)

Command:

```
perf stat -d env XF_DB=/tmp/tmp.ddnoeAjz9Y/xf.db XF_INDEX=/tmp/tmp.ddnoeAjz9Y/index \
  ./target/debug/xf --quiet --no-color search "rust" --limit 100
```

Selected counters:

- task-clock: 589,933,059
- cycles: 2,505,988,030
- instructions: 416,755,135
- branches: 95,546,129
- branch-misses: 2,451,975 (2.57%)
- L1-dcache-loads: 126,766,148
- L1-dcache-load-misses: 7,731,224 (6.10%)
- time elapsed: 0.6069 s

## Notes

- Benchmarks were run on a local dev machine with a debug build.
- All runs used the deterministic perf corpus and isolated temp DB/index paths.
- Semantic search is driven by the hash embedder (same as indexing).
