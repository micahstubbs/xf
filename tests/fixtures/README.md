# Test Fixtures

This directory contains test fixtures for xf's performance benchmarks and isomorphism verification.

## Directory Structure

```
tests/fixtures/
├── perf_corpus/           # Standardized test corpus for benchmarks
│   ├── data/
│   │   ├── manifest.js    # Archive metadata (required by xf)
│   │   ├── tweets.js      # 10,000 synthetic tweets
│   │   ├── like.js        # 5,000 synthetic likes
│   │   ├── direct-messages.js  # 2,000 messages in 100 conversations
│   │   └── grok-chat-item.js   # 500 Grok messages
│   └── corpus_manifest.json    # Corpus metadata with SHA256 checksums
├── golden_outputs/        # Expected outputs for isomorphism verification
│   ├── search_lexical_machine.json
│   ├── search_hybrid_rust.json
│   ├── stats_basic.txt
│   └── stats_detailed.json
└── README.md              # This file
```

## Test Corpus (perf_corpus)

### Purpose

The test corpus provides a reproducible, deterministic dataset for:
1. Performance benchmarks (criterion.rs)
2. Baseline measurements
3. Isomorphism verification (output consistency)
4. Regression testing

### Regenerating the Corpus

The corpus is generated deterministically from a seed:

```bash
# Generate default corpus (17,500 records)
python3 scripts/generate_perf_corpus.py --seed 42

# Generate smaller corpus for quick tests
python3 scripts/generate_perf_corpus.py --seed 42 --scale 0.1

# Generate larger corpus for stress tests
python3 scripts/generate_perf_corpus.py --seed 42 --scale 5.0
```

### Corpus Characteristics

| File | Records | Content |
|------|---------|---------|
| tweets.js | 10,000 | Tweets with varied text, hashtags, mentions, engagement metrics |
| like.js | 5,000 | Liked tweets with text and URLs |
| direct-messages.js | 2,000 | Messages in 100 conversations |
| grok-chat-item.js | 500 | Grok AI conversation messages |

Features:
- **Deterministic**: Same seed produces identical output
- **Diverse content**: Includes unicode, emoji, CJK, RTL text
- **Realistic distributions**: Engagement metrics, date ranges, reply chains
- **No PII**: All data is synthetic

### Verification

The corpus checksum can be verified using the manifest:

```bash
cat tests/fixtures/perf_corpus/corpus_manifest.json
```

## Golden Outputs

### Purpose

Golden outputs capture expected search/stats results for isomorphism verification.
This ensures that code changes don't inadvertently alter output behavior.

### Usage

```bash
# Verify current outputs match golden files
./scripts/verify_isomorphism.sh

# Update golden files after intentional changes
./scripts/verify_isomorphism.sh --update
```

### When to Update Golden Files

Update golden files when:
- Search ranking algorithm changes intentionally
- Output format changes intentionally
- New data types are added to results

Do NOT update if:
- Output changes unexpectedly (investigate first!)
- Tests fail after unrelated changes

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark group
cargo bench --bench search_perf

# Run with baseline comparison
cargo bench -- --baseline main
```

## Adding New Fixtures

1. Add fixture files to the appropriate directory
2. Update corpus generator if adding new data types
3. Add golden outputs for new command variations
4. Document changes in this README
