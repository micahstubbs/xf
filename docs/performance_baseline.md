# Performance Baseline

Date: 2026-01-12T09:43:27Z (UTC)

## Environment

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
