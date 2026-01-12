# Performance Baseline (2026-01-12)

Baseline captured on 2026-01-12T01:39:10-05:00 using:

- Commit: ea9b9b731f3ba0afc973e575a89de1582bae3912
- Binary: `target/release/xf` (release + debuginfo, frame pointers)
- Archive: `/data/projects/my_twitter_data`
- Baseline index: `/tmp/xf-baseline-1768199809` (`XF_DB` + `XF_INDEX`)

Dataset counts (from index run):

- Tweets: 12,297
- Likes: 43,960
- DMs: 6,676
- Grok: 3,912
- Embeddings: 66,580
- Total docs indexed: 66,662

## Latency (20 runs each)

All timings in milliseconds (ms).

```
hybrid     p50 262.34  p95 267.21  p99 267.21  min 250.53  max 275.15
lexical    p50   7.31  p95   7.62  p99   7.62  min   6.62  max   9.58
semantic   p50 258.31  p95 268.42  p99 268.42  min 248.93  max 270.57
stats      p50   6.50  p95   7.73  p99   7.73  min   5.43  max   8.28
stats -d   p50  80.53  p95  85.18  p99  85.18  min  75.54  max  85.56
```

Index end-to-end (3 runs):

```
p50 9199.09  p95 9199.09  p99 9199.09  min 9157.99  max 9210.64
```

## Memory (max RSS)

`/usr/bin/time -v`:

- Search (hybrid): 191,984 KB
- Index: 508,640 KB

## perf stat (summary)

Search (hybrid):

- Elapsed: 0.274s
- Instructions: 1.52B
- Cycles: 1.09B
- L1 dcache miss: 5.34%

Index:

- Elapsed: 9.308s
- Instructions: 145.56B
- Cycles: 81.57B
- L1 dcache miss: 2.20%

## perf report (hotspot)

Hybrid search is dominated by loading embeddings from SQLite:

- `Storage::load_all_embeddings` accounts for ~70% of samples
  (SQLite row iteration + allocations).
